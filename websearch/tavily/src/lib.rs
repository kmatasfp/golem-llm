mod client;
mod conversions;

use std::cell::RefCell;

use crate::client::{SearchRequest, TavilySearchApi};
use crate::conversions::{params_to_request, response_to_results, validate_search_params};
use golem_web_search::durability::Durablewebsearch;
use golem_web_search::durability::ExtendedwebsearchGuest;
use golem_web_search::golem::web_search::web_search::{
    Guest, GuestSearchSession, SearchError, SearchMetadata, SearchParams, SearchResult,
    SearchSession,
};

use golem_web_search::LOGGING_STATE;

#[derive(Debug, Clone, PartialEq, golem_rust::FromValueAndType, golem_rust::IntoValue)]
pub struct TavilyReplayState {
    pub api_key: String,
    pub current_page: u32,
    pub metadata: Option<SearchMetadata>,
}

struct TavilySearch {
    client: TavilySearchApi,
    params: SearchParams,
    finished: bool,
    metadata: Option<SearchMetadata>,
    current_page: u32,
}

impl TavilySearch {
    fn new(client: TavilySearchApi, _request: SearchRequest, params: SearchParams) -> Self {
        Self {
            client,
            params,
            finished: false,
            metadata: None,
            current_page: 0,
        }
    }

    fn next_page(&mut self) -> Result<Vec<SearchResult>, SearchError> {
        if self.finished {
            return Ok(vec![]);
        }

        let api_key = std::env::var("TAVILY_API_KEY").unwrap_or_default();
        let request =
            crate::conversions::params_to_request(self.params.clone(), api_key, self.current_page)?;

        let response = self.client.search(request)?;
        let (results, metadata) = response_to_results(response, &self.params, self.current_page);

        // Check if more results are available
        if let Some(ref meta) = metadata {
            if meta.next_page_token.is_none() {
                self.finished = true;
            } else {
                self.current_page += 1;
            }
        } else {
            self.finished = true;
        }

        self.metadata = metadata;
        Ok(results)
    }

    fn get_metadata(&self) -> Option<SearchMetadata> {
        self.metadata.clone()
    }
}

// Create a wrapper that implements GuestSearchSession properly
struct TavilySearchSession(RefCell<TavilySearch>);

impl TavilySearchSession {
    fn new(search: TavilySearch) -> Self {
        Self(RefCell::new(search))
    }
}

impl GuestSearchSession for TavilySearchSession {
    fn next_page(&self) -> Result<Vec<SearchResult>, SearchError> {
        let mut search = self.0.borrow_mut();
        search.next_page()
    }
    fn get_metadata(&self) -> Option<SearchMetadata> {
        let search = self.0.borrow();
        search.get_metadata()
    }
}

struct TavilySearchComponent;

impl TavilySearchComponent {
    const API_KEY_VAR: &'static str = "TAVILY_API_KEY";

    fn create_client() -> Result<TavilySearchApi, SearchError> {
        let api_key = Self::get_api_key()?;
        Ok(TavilySearchApi::new(api_key))
    }

    fn get_api_key() -> Result<String, SearchError> {
        std::env::var(Self::API_KEY_VAR).map_err(|_| {
            SearchError::BackendError("TAVILY_API_KEY environment variable not set".to_string())
        })
    }

    fn execute_search(
        params: SearchParams,
    ) -> Result<(Vec<SearchResult>, Option<SearchMetadata>), SearchError> {
        validate_search_params(&params)?;

        let client = Self::create_client()?;
        let api_key = Self::get_api_key()?;
        let request = params_to_request(params.clone(), api_key, 1)?;

        let response = client.search(request)?;
        let (results, metadata) = response_to_results(response, &params, 1);

        // Unwrap the metadata Option since we know it should be Some
        Ok((results, metadata))
    }

    fn start_search_session(params: SearchParams) -> Result<TavilySearchSession, SearchError> {
        validate_search_params(&params)?;

        let client = Self::create_client()?;
        let api_key = Self::get_api_key()?;
        let request = params_to_request(params.clone(), api_key, 1)?;

        let search = TavilySearch::new(client, request, params);
        Ok(TavilySearchSession::new(search))
    }
}

impl Guest for TavilySearchComponent {
    type SearchSession = TavilySearchSession;

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

impl ExtendedwebsearchGuest for TavilySearchComponent {
    type ReplayState = TavilyReplayState;

    fn unwrapped_search_session(params: SearchParams) -> Result<Self::SearchSession, SearchError> {
        let client = Self::create_client()?;
        let api_key = Self::get_api_key()?;
        let request = crate::conversions::params_to_request(params.clone(), api_key, 1)?;
        let search = TavilySearch::new(client, request, params);
        Ok(TavilySearchSession::new(search))
    }

    fn session_to_state(session: &Self::SearchSession) -> Self::ReplayState {
        let search = session.0.borrow();
        TavilyReplayState {
            api_key: search.client.api_key().to_string(),
            current_page: search.current_page,
            metadata: search.metadata.clone(),
        }
    }

    fn session_from_state(
        state: &Self::ReplayState,
        params: SearchParams,
    ) -> Result<Self::SearchSession, SearchError> {
        let client = TavilySearchApi::new(state.api_key.clone());
        let request =
            crate::conversions::params_to_request(params.clone(), state.api_key.clone(), 1)?;
        let mut search = TavilySearch::new(client, request, params);
        search.current_page = state.current_page;
        search.metadata = state.metadata.clone();

        Ok(TavilySearchSession::new(search))
    }
}

type DurableTavilyComponent = Durablewebsearch<TavilySearchComponent>;
golem_web_search::export_websearch!(DurableTavilyComponent with_types_in golem_web_search);

impl From<SearchParams> for TavilyReplayState {
    fn from(_params: SearchParams) -> Self {
        TavilyReplayState {
            api_key: String::new(), // Not used in real replay, only for macro compatibility
            current_page: 0,
            metadata: None,
        }
    }
}

impl TavilySearchApi {
    pub fn api_key(&self) -> &String {
        &self.api_key
    }
}
