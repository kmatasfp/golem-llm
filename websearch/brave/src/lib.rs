mod client;
mod conversions;

use crate::client::{ BraveSearchApi, SearchRequest };
use crate::conversions::{
    _create_pagination_request,
    _extract_next_page_offset,
    params_to_request,
    response_to_results,
    validate_search_params,
};
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
use log::trace;
use std::cell::{ RefCell };

use golem_rust::wasm_rpc::Pollable;
use golem_web_search::durability::ExtendedwebsearchGuest;
use golem_web_search::event_source::error::EventSourceSearchError;

struct BraveSearchComponent;

impl BraveSearchComponent {
    const API_KEY_VAR: &'static str = "BRAVE_API_KEY";

    fn create_client() -> Result<BraveSearchApi, SearchError> {
        let api_key = std::env
            ::var(Self::API_KEY_VAR)
            .map_err(|_| {
                SearchError::BackendError("BRAVE_API_KEY environment variable not set".to_string())
            })?;

        Ok(BraveSearchApi::new(api_key))
    }

    fn start_search_session(
        params: SearchParams
    ) -> Result<GuestSearchStream<BraveSearchStream>, SearchError> {
        validate_search_params(&params)?;

        let client = Self::create_client()?;
        let request = params_to_request(params.clone())?;

        Ok(BraveSearchStream::new(client, request, params))
    }

    fn execute_search(
        params: SearchParams
    ) -> Result<(Vec<SearchResult>, Option<SearchMetadata>), SearchError> {
        validate_search_params(&params)?;

        let client = Self::create_client()?;
        let request = params_to_request(params.clone())?;

        trace!("Executing one-shot Brave Search: {:?}", request);

        match client.search(request) {
            Ok(response) => {
                let (results, metadata) = response_to_results(response, &params);
                Ok((results, metadata))
            }
            Err(err) => Err(err),
        }
    }
}

struct BraveSearchStream {
    _api: RefCell<Option<BraveSearchApi>>,
    _current_request: RefCell<Option<SearchRequest>>,
    _current_offset: RefCell<u32>,
    _original_params: RefCell<Option<SearchParams>>,
    finished: RefCell<bool>,
    failure: Option<EventSourceSearchError>,
    _last_metadata: RefCell<Option<SearchMetadata>>,
}

impl BraveSearchStream {
    pub fn new(
        api: BraveSearchApi,
        request: SearchRequest,
        params: SearchParams
    ) -> GuestSearchStream<Self> {
        GuestSearchStream::new(BraveSearchStream {
            _api: RefCell::new(Some(api)),
            _current_request: RefCell::new(Some(request)),
            _current_offset: RefCell::new(0),
            _original_params: RefCell::new(Some(params)),
            finished: RefCell::new(false),
            failure: None,
            _last_metadata: RefCell::new(None),
        })
    }

    pub fn _failed(error: EventSourceSearchError) -> GuestSearchStream<Self> {
        GuestSearchStream::new(BraveSearchStream {
            _api: RefCell::new(None),
            _current_request: RefCell::new(None),
            _current_offset: RefCell::new(0),
            _original_params: RefCell::new(None),
            finished: RefCell::new(true),
            failure: Some(error),
            _last_metadata: RefCell::new(None),
        })
    }

    pub fn _next_page(&self) -> Result<Vec<SearchResult>, SearchError> {
        if self.is_finished() {
            if let Some(error) = self.failure() {
                return Err(error.clone().into());
            }
            return Ok(Vec::new());
        }

        let api = self._api.borrow();
        let request = self._current_request.borrow();
        let params = self._original_params.borrow();
        let current_offset = *self._current_offset.borrow();

        if
            let (Some(api), Some(request), Some(params)) = (
                api.as_ref(),
                request.as_ref(),
                params.as_ref(),
            )
        {
            trace!("Executing Brave Search with offset: {}", current_offset);

            let paginated_request = _create_pagination_request(request.clone(), current_offset);

            match api.search(paginated_request) {
                Ok(response) => {
                    let (results, metadata) = response_to_results(response.clone(), params);

                    *self._last_metadata.borrow_mut() = metadata;

                    let current_count = request.count.unwrap_or(20);
                    if
                        let Some(next_offset) = _extract_next_page_offset(
                            &response,
                            current_offset,
                            current_count
                        )
                    {
                        *self._current_offset.borrow_mut() = next_offset;
                    } else {
                        self.set_finished();
                    }

                    Ok(results)
                }
                Err(err) => {
                    self.set_finished();
                    Err(err)
                }
            }
        } else {
            Err(SearchError::BackendError("Session not properly initialized".to_string()))
        }
    }
    pub fn _get_metadata(&self) -> Option<SearchMetadata> {
        self._last_metadata.borrow().clone()
    }
}

impl SearchStreamState for BraveSearchStream {
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
    ) -> std::cell::Ref<
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
    ) -> std::cell::RefMut<
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

pub struct BraveSearchSession(GuestSearchStream<BraveSearchStream>);

impl Guest for BraveSearchComponent {
    type SearchSession = BraveSearchSession;

    fn start_search(params: SearchParams) -> Result<SearchSession, SearchError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());

        match Self::start_search_session(params) {
            Ok(session) => Ok(SearchSession::new(BraveSearchSession(session))),
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

impl ExtendedwebsearchGuest for BraveSearchComponent {
    fn unwrapped_search_session(params: SearchParams) -> Result<BraveSearchSession, SearchError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());

        Self::start_search_session(params).map(BraveSearchSession)
    }

    fn subscribe(session: &Self::SearchSession) -> Pollable {
        session.0.subscribe()
    }
}

impl GuestSearchSession for BraveSearchSession {
    fn next_page(&self) -> Result<Vec<SearchResult>, SearchError> {
        let stream = self.0.state();
        stream._next_page()
    }
    fn get_metadata(&self) -> Option<SearchMetadata> {
        let stream = self.0.state();
        stream._get_metadata()
    }
}

golem_web_search::export_websearch!(BraveSearchComponent with_types_in golem_web_search);
