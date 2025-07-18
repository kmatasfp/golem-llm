mod client;
mod conversions;

use std::cell::RefCell;

use crate::client::{GoogleSearchApi, SearchRequest};
use crate::conversions::{params_to_request, response_to_results, validate_search_params};
use golem_web_search::golem::web_search::web_search::{
    Guest, GuestSearchSession, SearchError, SearchMetadata, SearchParams, SearchResult,
    SearchSession,
};

use golem_web_search::LOGGING_STATE;

struct GoogleSearch {
    client: GoogleSearchApi,
    request: SearchRequest,
    params: SearchParams,
    finished: bool,
    metadata: Option<SearchMetadata>,
}

impl GoogleSearch {
    fn new(client: GoogleSearchApi, request: SearchRequest, params: SearchParams) -> Self {
        Self {
            client,
            request,
            params,
            finished: false,
            metadata: None,
        }
    }

    fn next_page(&mut self) -> Result<Vec<SearchResult>, SearchError> {
        if self.finished {
            return Ok(vec![]);
        }

        let response = self.client.search(self.request.clone())?;
        let (results, metadata) = response_to_results(response, &self.params);

        self.metadata = metadata;
        self.finished = true;

        Ok(results)
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
        search.next_page()
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
        let request = params_to_request(params.clone())?;

        let response = client.search(request)?;
        let (results, metadata) = response_to_results(response, &params);

        Ok((results, metadata))
    }

    fn start_search_session(params: SearchParams) -> Result<GoogleSearchSession, SearchError> {
        validate_search_params(&params)?;

        let client = Self::create_client()?;
        let request = params_to_request(params.clone())?;

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

golem_web_search::export_websearch!(GoogleCustomSearchComponent with_types_in golem_web_search);
