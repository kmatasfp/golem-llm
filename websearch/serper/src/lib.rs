mod client;
mod conversions;

use std::cell::RefCell;

use crate::client::{SearchRequest, SerperSearchApi};
use crate::conversions::{params_to_request, response_to_results, validate_search_params};
use golem_web_search::durability::Durablewebsearch;
use golem_web_search::durability::ExtendedwebsearchGuest;
use golem_web_search::golem::web_search::web_search::{
    Guest, GuestSearchSession, SearchError, SearchMetadata, SearchParams, SearchResult,
    SearchSession,
};

use golem_web_search::LOGGING_STATE;

#[derive(Debug, Clone, PartialEq, golem_rust::FromValueAndType, golem_rust::IntoValue)]
pub struct SerperReplayState {
    pub api_key: String,
    pub current_page: u32,
    pub metadata: SearchMetadata,
    pub finished: bool,
}

struct SerperSearch {
    client: SerperSearchApi,
    request: SearchRequest,
    params: SearchParams,
    metadata: SearchMetadata,
    current_page: u32, // 1-based
}

impl SerperSearch {
    fn new(client: SerperSearchApi, request: SearchRequest, params: SearchParams) -> Self {
        Self {
            client,
            request,
            params: params.clone(),
            metadata: SearchMetadata {
                query: params.query,
                total_results: None,
                search_time_ms: None,
                safe_search: None,
                language: None,
                region: None,
                next_page_token: None,
                rate_limits: None,
                current_page: 1,
            },
            current_page: 1, // 1-based
        }
    }
    fn next_page(&mut self) -> Result<(Vec<SearchResult>, bool), SearchError> {
        let request =
            crate::conversions::params_to_request(self.params.clone(), self.current_page)?;
        let response = self.client.search(request)?;
        let (results, metadata) = response_to_results(response, &self.params, self.current_page);

        // Determine if more results are available
        let num_results = self.request.num.unwrap_or(10);
        let finished = results.len() < (num_results as usize);

        // Update metadata for this page
        self.metadata = metadata;
        self.metadata.current_page = self.current_page;

        if !finished {
            self.current_page += 1;
            self.metadata.next_page_token = Some(self.current_page.to_string());
        } else {
            self.metadata.next_page_token = None;
        }

        Ok((results, finished))
    }
    fn get_metadata(&self) -> Option<SearchMetadata> {
        Some(self.metadata.clone())
    }
}

// Create a wrapper that implements GuestSearchSession properly
struct SerperSearchSession(RefCell<SerperSearch>);

impl SerperSearchSession {
    fn new(search: SerperSearch) -> Self {
        Self(RefCell::new(search))
    }
}

impl GuestSearchSession for SerperSearchSession {
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

struct SerperSearchComponent;

impl SerperSearchComponent {
    const API_KEY_VAR: &'static str = "SERPER_API_KEY";

    fn get_api_key() -> Result<String, SearchError> {
        std::env::var(Self::API_KEY_VAR).map_err(|_| {
            SearchError::BackendError("SERPER_API_KEY environment variable not set".to_string())
        })
    }

    fn create_client() -> Result<SerperSearchApi, SearchError> {
        let api_key = Self::get_api_key()?;
        Ok(SerperSearchApi::new(api_key))
    }

    fn execute_search(
        params: SearchParams,
    ) -> Result<(Vec<SearchResult>, SearchMetadata), SearchError> {
        validate_search_params(&params)?;

        let client = Self::create_client()?;
        let request = params_to_request(params.clone(), 1)?;

        let response = client.search(request)?;
        let (results, metadata) = response_to_results(response, &params, 1);

        Ok((results, metadata))
    }

    fn start_search_session(params: SearchParams) -> Result<SerperSearchSession, SearchError> {
        validate_search_params(&params)?;

        let client = Self::create_client()?;
        let request = params_to_request(params.clone(), 1)?;

        let search = SerperSearch::new(client, request, params);
        Ok(SerperSearchSession::new(search))
    }
}

impl Guest for SerperSearchComponent {
    type SearchSession = SerperSearchSession;

    fn start_search(params: SearchParams) -> Result<SearchSession, SearchError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        match Self::start_search_session(params) {
            Ok(session) => Ok(SearchSession::new(session)),
            Err(err) => Err(err),
        }
    }

    fn search_once(
        params: SearchParams,
    ) -> Result<(Vec<SearchResult>, Option<SearchMetadata>), SearchError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        let (results, metadata) = Self::execute_search(params)?;
        Ok((results, Some(metadata)))
    }
}

impl ExtendedwebsearchGuest for SerperSearchComponent {
    type ReplayState = SerperReplayState;

    fn unwrapped_search_session(params: SearchParams) -> Result<Self::SearchSession, SearchError> {
        let client = Self::create_client()?;
        let request = crate::conversions::params_to_request(params.clone(), 1)?;
        let search = SerperSearch::new(client, request, params);
        Ok(SerperSearchSession::new(search))
    }

    fn session_to_state(session: &Self::SearchSession) -> Self::ReplayState {
        let mut search = session.0.borrow_mut();
        let (_, finished) = search.next_page().unwrap_or((vec![], true));
        SerperReplayState {
            api_key: search.client.api_key().to_string(),
            current_page: search.current_page,
            metadata: search.metadata.clone(),
            finished,
        }
    }

    fn session_from_state(
        state: &Self::ReplayState,
        params: SearchParams,
    ) -> Result<Self::SearchSession, SearchError> {
        let client = SerperSearchApi::new(state.api_key.clone());
        let request = crate::conversions::params_to_request(params.clone(), state.current_page)?;
        let mut search = SerperSearch::new(client, request, params);
        search.current_page = state.current_page;
        search.metadata = state.metadata.clone();
        if state.finished {
            let _ = search.next_page();
        }
        Ok(SerperSearchSession::new(search))
    }
}

type DurableSerperComponent = Durablewebsearch<SerperSearchComponent>;
golem_web_search::export_websearch!(DurableSerperComponent with_types_in golem_web_search);
