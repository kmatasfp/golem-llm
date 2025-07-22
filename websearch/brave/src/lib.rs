mod client;
mod conversions;

use std::cell::RefCell;

use crate::client::{BraveSearchApi, SearchRequest};
use crate::conversions::{params_to_request, response_to_results, validate_search_params};
use golem_web_search::durability::Durablewebsearch;
use golem_web_search::durability::ExtendedwebsearchGuest;
use golem_web_search::golem::web_search::web_search::{
    Guest, GuestSearchSession, SearchError, SearchMetadata, SearchParams, SearchResult,
    SearchSession,
};

use golem_web_search::LOGGING_STATE;

// Define a custom ReplayState struct
#[derive(Debug, Clone, PartialEq, golem_rust::FromValueAndType, golem_rust::IntoValue)]
pub struct BraveReplayState {
    pub api_key: String,
    pub current_offset: u32,
    pub metadata: Option<SearchMetadata>,
    pub finished: bool,
}

struct BraveSearch {
    client: BraveSearchApi,
    request: SearchRequest,
    params: SearchParams,
    metadata: Option<SearchMetadata>,
    current_offset: u32,
}

impl BraveSearch {
    fn new(client: BraveSearchApi, request: SearchRequest, params: SearchParams) -> Self {
        Self {
            client,
            request,
            params,
            metadata: None,
            current_offset: 0,
        }
    }

    fn next_page(&mut self) -> Result<(Vec<SearchResult>, bool), SearchError> {
        // Update request with current offset
        let mut request = self.request.clone();
        request.offset = Some(self.current_offset);

        let response = self.client.search(request)?;
        let (results, metadata) = response_to_results(response, &self.params, self.current_offset);

        // Always increment current_offset after a page fetch
        self.current_offset += 1;

        // Check if more results are available
        let count = self.request.count.unwrap_or(10);
        let has_more_results = results.len() == (count as usize);
        let has_next_page = metadata.next_page_token.is_some();
        let finished = !has_more_results || !has_next_page;

        self.metadata = Some(metadata);
        Ok((results, finished))
    }

    fn get_metadata(&self) -> Option<SearchMetadata> {
        self.metadata.clone()
    }
}

// Create a wrapper that implements GuestSearchSession properly
struct BraveSearchSession(RefCell<BraveSearch>);

impl BraveSearchSession {
    fn new(search: BraveSearch) -> Self {
        Self(RefCell::new(search))
    }
}

impl GuestSearchSession for BraveSearchSession {
    fn next_page(&self) -> Result<Vec<SearchResult>, SearchError> {
        let mut search = self.0.borrow_mut();
        let (results, _) = search.next_page()?;
        Ok(results)
    }

    fn get_metadata(&self) -> Option<SearchMetadata> {
        let search = self.0.borrow();
        search.get_metadata()
    }
}

struct BraveSearchComponent;

impl BraveSearchComponent {
    const API_KEY_VAR: &'static str = "BRAVE_API_KEY";

    fn create_client() -> Result<BraveSearchApi, SearchError> {
        let api_key = Self::get_api_key()?;
        Ok(BraveSearchApi::new(api_key))
    }

    fn get_api_key() -> Result<String, SearchError> {
        std::env::var(Self::API_KEY_VAR).map_err(|_| {
            SearchError::BackendError("BRAVE_API_KEY environment variable not set".to_string())
        })
    }

    fn execute_search(
        params: SearchParams,
        _api_key: String,
    ) -> Result<(Vec<SearchResult>, SearchMetadata), SearchError> {
        validate_search_params(&params)?;

        let client = Self::create_client()?;
        let request = params_to_request(params.clone(), 0)?;

        let response = client.search(request)?;
        let (results, metadata) = response_to_results(response, &params, 0);

        Ok((results, metadata))
    }

    fn start_search_session(
        params: SearchParams,
        _api_key: String,
    ) -> Result<BraveSearchSession, SearchError> {
        validate_search_params(&params)?;

        let client = Self::create_client()?;
        let request = params_to_request(params.clone(), 0)?;

        let search = BraveSearch::new(client, request, params);
        Ok(BraveSearchSession::new(search))
    }
}

impl Guest for BraveSearchComponent {
    type SearchSession = BraveSearchSession;

    fn start_search(params: SearchParams) -> Result<SearchSession, SearchError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        match Self::start_search_session(params, Self::get_api_key()?) {
            Ok(session) => Ok(SearchSession::new(session)),
            Err(err) => Err(err),
        }
    }

    fn search_once(
        params: SearchParams,
    ) -> Result<(Vec<SearchResult>, Option<SearchMetadata>), SearchError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        let (results, metadata) = Self::execute_search(params, Self::get_api_key()?)?;
        Ok((results, Some(metadata)))
    }
}

// ExtendedwebsearchGuest implementation
impl ExtendedwebsearchGuest for BraveSearchComponent {
    type ReplayState = BraveReplayState;

    fn unwrapped_search_session(params: SearchParams) -> Result<Self::SearchSession, SearchError> {
        let api_key = Self::get_api_key()?;
        let client = BraveSearchApi::new(api_key.clone());
        let request = crate::conversions::params_to_request(params.clone(), 0)?;
        let search = BraveSearch::new(client, request, params);
        Ok(BraveSearchSession::new(search))
    }

    fn session_to_state(session: &Self::SearchSession) -> Self::ReplayState {
        let mut search = session.0.borrow_mut();
        let (_, finished) = search.next_page().unwrap_or((vec![], true));
        BraveReplayState {
            api_key: search.client.api_key().clone(),
            current_offset: search.current_offset,
            metadata: search.metadata.clone(),
            finished,
        }
    }

    fn session_from_state(
        state: &Self::ReplayState,
        params: SearchParams,
    ) -> Result<Self::SearchSession, SearchError> {
        let client = BraveSearchApi::new(state.api_key.clone());
        let request = crate::conversions::params_to_request(params.clone(), 0)?;
        let mut search = BraveSearch::new(client, request, params);
        search.current_offset = state.current_offset;
        search.metadata = state.metadata.clone();
        if state.finished {
            let _ = search.next_page();
        }
        Ok(BraveSearchSession::new(search))
    }
}

type DurableBraveComponent = Durablewebsearch<BraveSearchComponent>;
golem_web_search::export_websearch!(DurableBraveComponent with_types_in golem_web_search);
