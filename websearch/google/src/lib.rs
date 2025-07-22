mod client;
mod conversions;

use std::cell::RefCell;

use crate::client::{GoogleSearchApi, NextPage, SearchRequest};
use crate::conversions::{params_to_request, response_to_results, validate_search_params};
use golem_web_search::durability::Durablewebsearch;
use golem_web_search::durability::ExtendedwebsearchGuest;
use golem_web_search::golem::web_search::web_search::{
    Guest, GuestSearchSession, SearchError, SearchMetadata, SearchParams, SearchResult,
    SearchSession,
};

use golem_web_search::LOGGING_STATE;

#[derive(Debug, Clone, PartialEq, golem_rust::FromValueAndType, golem_rust::IntoValue)]
pub struct GoogleReplayState {
    pub api_key: String,
    pub search_engine_id: String,
    pub next_page_token: Option<String>,
    pub metadata: Option<SearchMetadata>,
    pub finished: bool,
}

struct GoogleSearch {
    client: GoogleSearchApi,
    request: SearchRequest,
    params: SearchParams,
    metadata: Option<SearchMetadata>,
    next_page: Option<NextPage>,
}

impl GoogleSearch {
    fn new(client: GoogleSearchApi, request: SearchRequest, params: SearchParams) -> Self {
        Self {
            client,
            request,
            params,
            metadata: None,
            next_page: None,
        }
    }

    fn next_page(&mut self) -> Result<(Vec<SearchResult>, bool), SearchError> {
        // Update request with current start index
        let mut request = self.request.clone();
        let current_start = if let Some(next_page) = &self.next_page {
            request.start = Some(next_page.start_index);
            next_page.start_index
        } else {
            1
        };

        let response = self.client.search(request)?;
        let (results, metadata) =
            response_to_results(response.clone(), &self.params, current_start);

        let finished = response.next_page.is_none();
        self.next_page = response.next_page;
        self.metadata = Some(metadata);
        Ok((results, finished))
    }

    fn get_metadata(&self) -> Option<SearchMetadata> {
        self.metadata.clone()
    }
}

// Create a wrapper that implements GuestSearchSession properly
struct GoogleSearchSession(RefCell<GoogleSearch>);

impl GoogleSearchSession {
    fn new(search: GoogleSearch) -> Self {
        Self(RefCell::new(search))
    }
}

impl GuestSearchSession for GoogleSearchSession {
    fn next_page(&self) -> Result<Vec<SearchResult>, SearchError> {
        let mut search = self.0.borrow_mut();
        search.next_page().map(|(results, _)| results)
    }

    fn get_metadata(&self) -> Option<SearchMetadata> {
        let search = self.0.borrow();
        search.get_metadata()
    }
}

struct GoogleCustomSearchComponent;

impl GoogleCustomSearchComponent {
    const API_KEY_VAR: &'static str = "GOOGLE_API_KEY";
    const SEARCH_ENGINE_ID_VAR: &'static str = "GOOGLE_SEARCH_ENGINE_ID";

    fn create_client() -> Result<GoogleSearchApi, SearchError> {
        let api_key = std::env::var(Self::API_KEY_VAR).map_err(|_| {
            SearchError::BackendError("GOOGLE_API_KEY environment variable not set".to_string())
        })?;

        let search_engine_id = std::env::var(Self::SEARCH_ENGINE_ID_VAR).map_err(|_| {
            SearchError::BackendError(
                "GOOGLE_SEARCH_ENGINE_ID environment variable not set".to_string(),
            )
        })?;

        Ok(GoogleSearchApi::new(api_key, search_engine_id))
    }

    fn execute_search(
        params: SearchParams,
    ) -> Result<(Vec<SearchResult>, Option<SearchMetadata>), SearchError> {
        validate_search_params(&params)?;

        let client = Self::create_client()?;
        let request = params_to_request(params.clone(), 1)?;

        let response = client.search(request)?;
        let (results, metadata) = response_to_results(response, &params, 1);

        Ok((results, Some(metadata)))
    }

    fn start_search_session(params: SearchParams) -> Result<GoogleSearchSession, SearchError> {
        validate_search_params(&params)?;

        let client = Self::create_client()?;
        let request = params_to_request(params.clone(), 1)?;

        let search = GoogleSearch::new(client, request, params);
        Ok(GoogleSearchSession::new(search))
    }
}

impl Guest for GoogleCustomSearchComponent {
    type SearchSession = GoogleSearchSession;

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
        Self::execute_search(params)
    }
}

impl ExtendedwebsearchGuest for GoogleCustomSearchComponent {
    type ReplayState = GoogleReplayState;

    fn unwrapped_search_session(params: SearchParams) -> Result<Self::SearchSession, SearchError> {
        let client = Self::create_client()?;
        let request = crate::conversions::params_to_request(params.clone(), 1)?;
        let search = GoogleSearch::new(client, request, params);
        Ok(GoogleSearchSession::new(search))
    }

    fn session_to_state(session: &Self::SearchSession) -> Self::ReplayState {
        let mut search = session.0.borrow_mut();
        let (_, finished) = search.next_page().unwrap_or_else(|_| (vec![], true));
        GoogleReplayState {
            api_key: search.client.api_key().to_string(),
            search_engine_id: search.client.search_engine_id().to_string(),
            next_page_token: search.next_page.as_ref().map(|p| p.start_index.to_string()),
            metadata: search.metadata.clone(),
            finished,
        }
    }

    fn session_from_state(
        state: &Self::ReplayState,
        params: SearchParams,
    ) -> Result<Self::SearchSession, SearchError> {
        let client = GoogleSearchApi::new(state.api_key.clone(), state.search_engine_id.clone());
        let request = crate::conversions::params_to_request(params.clone(), 1)?;
        let mut search = GoogleSearch::new(client, request, params);
        search.next_page = state
            .next_page_token
            .as_ref()
            .and_then(|t| t.parse().ok())
            .map(|start_index| NextPage { start_index });
        search.metadata = state.metadata.clone();
        if state.finished {
            let _ = search.next_page();
        }

        Ok(GoogleSearchSession::new(search))
    }
}

type DurableGoogleComponent = Durablewebsearch<GoogleCustomSearchComponent>;
golem_web_search::export_websearch!(DurableGoogleComponent with_types_in golem_web_search);
