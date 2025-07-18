mod client;
mod conversions;

use crate::client::{ CustomSearchApi, SearchRequest };
use crate::conversions::{ response_to_results, params_to_request, validate_search_params };
use golem_web_search::durability::{ ExtendedwebsearchGuest };
use golem_web_search::golem::web_search::web_search::{
    Guest,
    GuestSearchSession,
    SearchError,
    SearchMetadata,
    SearchParams,
    SearchResult,
    SearchSession,
};
use golem_web_search::session_stream::{ GuestSearchStream, SearchStreamState };
use golem_web_search::LOGGING_STATE;
use golem_rust::wasm_rpc::Pollable;
use log::trace;
use std::cell::{ Ref, RefCell, RefMut };
use golem_web_search::event_source::error::EventSourceSearchError;

struct GoogleSearchStream {
    _api: RefCell<Option<CustomSearchApi>>,
    _current_request: RefCell<Option<SearchRequest>>,
    _current_start: RefCell<u32>,
    _original_params: RefCell<Option<SearchParams>>,
    finished: RefCell<bool>,
    failure: Option<EventSourceSearchError>,
    _last_metadata: RefCell<Option<SearchMetadata>>,
}

impl GoogleSearchStream {
    pub fn new(
        api: CustomSearchApi,
        request: SearchRequest,
        params: SearchParams
    ) -> GuestSearchStream<Self> {
        GuestSearchStream::new(GoogleSearchStream {
            _api: RefCell::new(Some(api)),
            _current_request: RefCell::new(Some(request)),
            _current_start: RefCell::new(1),
            _original_params: RefCell::new(Some(params)),
            finished: RefCell::new(false),
            failure: None,
            _last_metadata: RefCell::new(None),
        })
    }

    pub fn _failed(error: EventSourceSearchError) -> GuestSearchStream<Self> {
        GuestSearchStream::new(GoogleSearchStream {
            _api: RefCell::new(None),
            _current_request: RefCell::new(None),
            _current_start: RefCell::new(1),
            _original_params: RefCell::new(None),
            finished: RefCell::new(true),
            failure: Some(error),
            _last_metadata: RefCell::new(None),
        })
    }
}

impl SearchStreamState for GoogleSearchStream {
    fn failure(&self) -> &Option<EventSourceSearchError> {
        &self.failure
    }

    fn is_finished(&self) -> bool {
        *self.finished.borrow()
    }

    fn set_finished(&self) {
        *self.finished.borrow_mut() = true;
    }

    fn stream(
        &self
    ) -> Ref<
        Option<
            Box<
                dyn golem_web_search::event_source::stream::WebsearchStream<
                    Item = golem_web_search::event_source::types::WebsearchStreamEntry,
                    Error = golem_web_search::event_source::error::StreamError<reqwest::Error>
                >
            >
        >
    > {
        unimplemented!()
    }

    fn stream_mut(
        &self
    ) -> RefMut<
        Option<
            Box<
                dyn golem_web_search::event_source::stream::WebsearchStream<
                    Item = golem_web_search::event_source::types::WebsearchStreamEntry,
                    Error = golem_web_search::event_source::error::StreamError<reqwest::Error>
                > +
                    '_
            >
        >
    > {
        unimplemented!()
    }
}

struct GoogleCustomSearchComponent;

impl GoogleCustomSearchComponent {
    const API_KEY_VAR: &'static str = "GOOGLE_API_KEY";
    const SEARCH_ENGINE_ID_VAR: &'static str = "GOOGLE_SEARCH_ENGINE_ID";

    fn create_client() -> Result<CustomSearchApi, SearchError> {
        let api_key = std::env
            ::var(Self::API_KEY_VAR)
            .map_err(|_|
                SearchError::BackendError("GOOGLE_API_KEY environment variable not set".to_string())
            )?;

        let search_engine_id = std::env
            ::var(Self::SEARCH_ENGINE_ID_VAR)
            .map_err(|_|
                SearchError::BackendError(
                    "GOOGLE_SEARCH_ENGINE_ID environment variable not set".to_string()
                )
            )?;

        Ok(CustomSearchApi::new(api_key, search_engine_id))
    }

    fn execute_search(
        params: SearchParams
    ) -> Result<(Vec<SearchResult>, Option<SearchMetadata>), SearchError> {
        validate_search_params(&params)?;

        let client = Self::create_client()?;
        let request = params_to_request(params.clone())?;

        trace!("Executing one-shot Google Search: {:?}", request);

        match client.search(request.clone()) {
            Ok(response) => {
                let (results, metadata) = response_to_results(response, &params);
                Ok((results, metadata))
            }
            Err(err) => Err(err),
        }
    }

    fn start_search_session(
        params: SearchParams
    ) -> Result<GuestSearchStream<GoogleSearchStream>, SearchError> {
        validate_search_params(&params)?;

        let client = Self::create_client()?;
        let request = params_to_request(params.clone())?;

        Ok(GoogleSearchStream::new(client, request, params))
    }
}

pub struct GoogleSearchSession(GuestSearchStream<GoogleSearchStream>);

impl Guest for GoogleCustomSearchComponent {
    type SearchSession = GoogleSearchSession;

    fn start_search(params: SearchParams) -> Result<SearchSession, SearchError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        match Self::start_search_session(params) {
            Ok(session) => Ok(SearchSession::new(GoogleSearchSession(session))),
            Err(err) => Err(err),
        }
    }

    fn search_once(
        params: SearchParams
    ) -> Result<(Vec<SearchResult>, Option<SearchMetadata>), SearchError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());

        Self::execute_search(params)
    }
}

impl ExtendedwebsearchGuest for GoogleCustomSearchComponent {
    fn unwrapped_search_session(params: SearchParams) -> Result<GoogleSearchSession, SearchError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());

        Self::start_search_session(params).map(GoogleSearchSession)
    }

    fn subscribe(session: &Self::SearchSession) -> Pollable {
        session.0.subscribe()
    }
}

impl GuestSearchSession for GoogleSearchSession {
    fn next_page(&self) -> Result<Vec<SearchResult>, SearchError> {
        let stream = self.0.state();

        // Check if the stream has failed
        if let Some(error) = stream.failure() {
            return Err(SearchError::BackendError(format!("Stream failed: {:?}", error)));
        }

        // Check if the stream is finished
        if stream.is_finished() {
            return Ok(vec![]); // Return empty results if finished
        }

        // Get the API client and current request
        let api_ref = stream._api.borrow();
        let request_ref = stream._current_request.borrow();
        let current_start_ref = stream._current_start.borrow();
        let params_ref = stream._original_params.borrow();

        let api = match api_ref.as_ref() {
            Some(api) => api,
            None => {
                stream.set_finished();
                return Err(SearchError::BackendError("API client not available".to_string()));
            }
        };

        let mut request = match request_ref.as_ref() {
            Some(req) => req.clone(),
            None => {
                stream.set_finished();
                return Err(SearchError::BackendError("Request not available".to_string()));
            }
        };

        let params = match params_ref.as_ref() {
            Some(p) => p,
            None => {
                stream.set_finished();
                return Err(SearchError::BackendError("Original params not available".to_string()));
            }
        };

        // Update the start parameter for pagination
        request.start = Some(*current_start_ref);

        trace!("Executing paginated Google Search: {:?}", request);

        // Execute the search
        match api.search(request.clone()) {
            Ok(response) => {
                let (results, metadata) = response_to_results(response, params);

                // Update pagination state
                let max_results = params.max_results.unwrap_or(10);
                let new_start = *current_start_ref + max_results;

                // Update the current start for next page
                drop(current_start_ref);
                *stream._current_start.borrow_mut() = new_start;

                // Store metadata if available
                if let Some(meta) = metadata.as_ref() {
                    *stream._last_metadata.borrow_mut() = Some(meta.clone());
                }

                // Check if we should mark as finished
                if results.len() < (max_results as usize) {
                    stream.set_finished();
                }

                Ok(results)
            }
            Err(err) => {
                stream.set_finished();
                Err(err)
            }
        }
    }

    fn get_metadata(&self) -> Option<SearchMetadata> {
        let stream = self.0.state();
        stream._last_metadata.borrow().clone()
    }
}
golem_web_search::export_websearch!(GoogleCustomSearchComponent with_types_in golem_web_search);
