mod client;
mod conversions;

use crate::client::{SearchRequest, SerperSearchApi};
use crate::conversions::{params_to_request, response_to_results, validate_search_params};
use golem_rust::wasm_rpc::Pollable;
use golem_web_search::durability::ExtendedwebsearchGuest;
use golem_web_search::event_source::error::EventSourceSearchError;
use golem_web_search::golem::web_search::web_search::{
    Guest, GuestSearchSession, SearchError, SearchMetadata, SearchParams, SearchResult,
    SearchSession,
};
use golem_web_search::session_stream::{GuestSearchStream, SearchStreamState};
use golem_web_search::LOGGING_STATE;
use log::trace;
use std::cell::{Ref, RefCell, RefMut};

struct SerperSearchStream {
    _api: RefCell<Option<SerperSearchApi>>,
    _current_request: RefCell<Option<SearchRequest>>,
    _original_params: RefCell<Option<SearchParams>>,
    _current_start_index: RefCell<u32>,
    _last_metadata: RefCell<Option<SearchMetadata>>,
    _has_more_results: RefCell<bool>,
    finished: RefCell<bool>,
    failure: Option<EventSourceSearchError>,
}

impl SerperSearchStream {
    pub fn new(
        api: SerperSearchApi,
        request: SearchRequest,
        params: SearchParams,
    ) -> GuestSearchStream<Self> {
        GuestSearchStream::new(SerperSearchStream {
            _api: RefCell::new(Some(api)),
            _current_request: RefCell::new(Some(request)),
            _original_params: RefCell::new(Some(params)),
            _current_start_index: RefCell::new(0),
            finished: RefCell::new(false),
            failure: None,
            _last_metadata: RefCell::new(None),
            _has_more_results: RefCell::new(true),
        })
    }

    pub fn _failed(error: EventSourceSearchError) -> GuestSearchStream<Self> {
        GuestSearchStream::new(SerperSearchStream {
            _api: RefCell::new(None),
            _current_request: RefCell::new(None),
            _original_params: RefCell::new(None),
            _current_start_index: RefCell::new(0),
            finished: RefCell::new(true),
            failure: Some(error),
            _last_metadata: RefCell::new(None),
            _has_more_results: RefCell::new(false),
        })
    }
}

impl SearchStreamState for SerperSearchStream {
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
        &self,
    ) -> Ref<
        Option<
            Box<
                dyn golem_web_search::event_source::stream::WebsearchStream<
                    Item = golem_web_search::event_source::types::WebsearchStreamEntry,
                    Error = golem_web_search::event_source::error::StreamError<reqwest::Error>,
                >,
            >,
        >,
    > {
        unimplemented!()
    }

    fn stream_mut(
        &self,
    ) -> RefMut<
        Option<
            Box<
                dyn golem_web_search::event_source::stream::WebsearchStream<
                        Item = golem_web_search::event_source::types::WebsearchStreamEntry,
                        Error = golem_web_search::event_source::error::StreamError<reqwest::Error>,
                    > + '_,
            >,
        >,
    > {
        unimplemented!()
    }
}

struct SerperSearchComponent;

impl SerperSearchComponent {
    const API_KEY_VAR: &'static str = "SERPER_API_KEY";

    fn create_client() -> Result<SerperSearchApi, SearchError> {
        let api_key = std::env::var(Self::API_KEY_VAR).map_err(|_| {
            SearchError::BackendError("SERPER_API_KEY environment variable not set".to_string())
        })?;

        Ok(SerperSearchApi::new(api_key))
    }

    fn execute_search(
        params: SearchParams,
    ) -> Result<(Vec<SearchResult>, Option<SearchMetadata>), SearchError> {
        validate_search_params(&params)?;

        let client = Self::create_client()?;
        let mut request = params_to_request(params.clone())?;
        request.start = Some(0);
        trace!("Executing one-shot Serper Search: {request:?}");

        match client.search(request) {
            Ok(response) => {
                let (results, metadata) = response_to_results(response, &params, 0);
                Ok((results, metadata))
            }
            Err(err) => Err(err),
        }
    }

    fn start_search_session(
        params: SearchParams,
    ) -> Result<GuestSearchStream<SerperSearchStream>, SearchError> {
        validate_search_params(&params)?;

        let client = Self::create_client()?;
        let request = params_to_request(params.clone())?;

        Ok(SerperSearchStream::new(client, request, params))
    }
}

pub struct SerperSearchSession(GuestSearchStream<SerperSearchStream>);

impl Guest for SerperSearchComponent {
    type SearchSession = SerperSearchSession;

    fn start_search(params: SearchParams) -> Result<SearchSession, SearchError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());

        match Self::start_search_session(params) {
            Ok(session) => Ok(SearchSession::new(SerperSearchSession(session))),
            Err(err) => Err(err),
        }
    }

    fn search_once(
        params: SearchParams,
    ) -> Result<(Vec<SearchResult>, Option<SearchMetadata>), SearchError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());

        Self::execute_search(params)
    }
}

impl ExtendedwebsearchGuest for SerperSearchComponent {
    fn unwrapped_search_session(params: SearchParams) -> Result<SerperSearchSession, SearchError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());

        Self::start_search_session(params).map(SerperSearchSession)
    }

    fn subscribe(session: &Self::SearchSession) -> Pollable {
        session.0.subscribe()
    }
}

impl GuestSearchSession for SerperSearchSession {
    fn next_page(&self) -> Result<Vec<SearchResult>, SearchError> {
        let stream = self.0.state();
        // Check if the stream has failed
        if let Some(error) = stream.failure() {
            return Err(SearchError::BackendError(format!(
                "Stream failed: {error:?}"
            )));
        }
        if stream.is_finished() {
            return Ok(vec![]);
        }
        let api_ref = stream._api.borrow();
        let request_ref = stream._current_request.borrow();
        let params_ref = stream._original_params.borrow();
        let start_index_ref = stream._current_start_index.borrow();
        let api = match api_ref.as_ref() {
            Some(api) => api,
            None => {
                stream.set_finished();
                return Err(SearchError::BackendError(
                    "API client not available".to_string(),
                ));
            }
        };
        let mut request = match request_ref.as_ref() {
            Some(req) => req.clone(),
            None => {
                stream.set_finished();
                return Err(SearchError::BackendError(
                    "Request not available".to_string(),
                ));
            }
        };
        let params = match params_ref.as_ref() {
            Some(p) => p,
            None => {
                stream.set_finished();
                return Err(SearchError::BackendError(
                    "Original params not available".to_string(),
                ));
            }
        };
        request.start = Some(*start_index_ref);
        trace!("Executing paginated Serper Search: {request:?}");
        match api.search(request.clone()) {
            Ok(response) => {
                let (results, metadata) = response_to_results(response, params, *start_index_ref);
                let max_results = params.max_results.unwrap_or(10);
                let new_start = *start_index_ref + max_results;
                drop(start_index_ref);
                *stream._current_start_index.borrow_mut() = new_start;
                if let Some(meta) = metadata.as_ref() {
                    *stream._last_metadata.borrow_mut() = Some(meta.clone());
                }
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

golem_web_search::export_websearch!(SerperSearchComponent with_types_in golem_web_search);
