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
    pub metadata: Option<SearchMetadata>,
    pub finished: bool,
    pub all_results: Vec<SearchResult>,
    pub current_page: u32,
}

struct TavilySearch {
    client: TavilySearchApi,
    params: SearchParams,
    all_results: Vec<SearchResult>,
    page_size: u32,
    current_page: u32,
    metadata: Option<SearchMetadata>,
}

impl TavilySearch {
    fn new(client: TavilySearchApi, _request: SearchRequest, params: SearchParams) -> Self {
        let page_size = params.max_results.unwrap_or(10);
        Self {
            client,
            params,
            all_results: Vec::new(),
            page_size,
            current_page: 0,
            metadata: None,
        }
    }

    fn fetch_all_results(&mut self) -> Result<(), SearchError> {
        let api_key = std::env::var("TAVILY_API_KEY").unwrap_or_default();
        let request = crate::conversions::params_to_request(self.params.clone(), api_key, 0)?;
        let response = self.client.search(request)?;
        let (results, metadata) = response_to_results(response, &self.params, 0);
        self.all_results = results;
        self.metadata = Some(metadata);
        Ok(())
    }

    fn next_page(&mut self) -> Result<(Vec<SearchResult>, bool), SearchError> {
        if self.all_results.is_empty() {
            self.fetch_all_results()?;
        }
        let start = (self.current_page * self.page_size) as usize;
        let end = (((self.current_page + 1) * self.page_size) as usize).min(self.all_results.len());
        let page_results = if start < self.all_results.len() {
            self.all_results[start..end].to_vec()
        } else {
            Vec::new()
        };
        // Update metadata for this page
        if let Some(metadata) = &mut self.metadata {
            metadata.current_page = self.current_page;
            metadata.next_page_token = if end < self.all_results.len() {
                Some((self.current_page + 1).to_string())
            } else {
                None
            };
        }

        self.current_page += 1;
        let finished = end >= self.all_results.len();
        Ok((page_results, finished))
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
        let (results, _) = search.next_page()?;
        Ok(results)
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
    ) -> Result<(Vec<SearchResult>, SearchMetadata), SearchError> {
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
        let (results, metadata) = Self::execute_search(params)?;
        Ok((results, Some(metadata)))
    }
}

impl ExtendedwebsearchGuest for TavilySearchComponent {
    type ReplayState = TavilyReplayState;
    fn unwrapped_search_session(params: SearchParams) -> Result<Self::SearchSession, SearchError> {
        let client = Self::create_client()?;
        let api_key = Self::get_api_key()?;
        let request = crate::conversions::params_to_request(params.clone(), api_key, 0)?;
        let search = TavilySearch::new(client, request, params);
        Ok(TavilySearchSession::new(search))
    }
    fn session_to_state(session: &Self::SearchSession) -> Self::ReplayState {
        let mut search = session.0.borrow_mut();
        let (_, finished) = search.next_page().unwrap_or((vec![], true));
        TavilyReplayState {
            api_key: search.client.api_key().to_string(),
            metadata: search.metadata.clone(),
            finished,
            all_results: search.all_results.clone(),
            current_page: search.current_page,
        }
    }
    fn session_from_state(
        state: &Self::ReplayState,
        params: SearchParams,
    ) -> Result<Self::SearchSession, SearchError> {
        let client = TavilySearchApi::new(state.api_key.clone());
        let request =
            crate::conversions::params_to_request(params.clone(), state.api_key.clone(), 0)?;
        let mut search = TavilySearch::new(client, request, params);
        search.metadata = state.metadata.clone();
        search.all_results = state.all_results.clone();
        search.current_page = state.current_page;
        if state.finished {
            let _ = search.next_page();
        }
        Ok(TavilySearchSession::new(search))
    }
}

type DurableTavilyComponent = Durablewebsearch<TavilySearchComponent>;
golem_web_search::export_websearch!(DurableTavilyComponent with_types_in golem_web_search);
