use golem_web_search::error::from_reqwest_error;
use golem_web_search::golem::web_search::web_search::SearchError;
use log::trace;
use reqwest::{Client, Method, Response};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

const BASE_URL: &str = "https://google.serper.dev/search";

/// The Serper Search API client for Google-powered web search.
pub struct SerperSearchApi {
    api_key: String,
    client: Client,
}

impl SerperSearchApi {
    pub fn new(api_key: String) -> Self {
        let client = Client::builder()
            .user_agent("Golem-Web-Search-Serper/1.0")
            .build()
            .expect("Failed to initialize HTTP client");

        Self { api_key, client }
    }

    pub fn search(&self, request: SearchRequest) -> Result<SearchResponse, SearchError> {
        trace!("Sending request to Serper Search API: {request:?}");

        let response: Response = self
            .client
            .request(Method::POST, BASE_URL)
            .header("X-API-KEY", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .map_err(|err| from_reqwest_error("Request failed", err))?;

        parse_response(response)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub q: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gl: Option<String>, // Country code (e.g., "us", "uk", "in")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hl: Option<String>, // Language code (e.g., "en", "es", "fr")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num: Option<u32>, // Number of results (1-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<u32>, // Starting index for pagination
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe: Option<String>, // Safe search: "active", "off"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tbm: Option<String>, // Search type: "isch" for images, "nws" for news
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tbs: Option<String>, // Time-based search filters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autocorrect: Option<bool>, // Enable/disable autocorrect
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub organic: Vec<SearchResult>,
    #[serde(rename = "peopleAlsoAsk")]
    pub people_also_ask: Option<Vec<PeopleAlsoAsk>>,
    #[serde(rename = "relatedSearches")]
    pub related_searches: Option<Vec<RelatedSearch>>,
    pub images: Option<Vec<ImageResult>>,
    pub news: Option<Vec<NewsResult>>,
    #[serde(rename = "answerBox")]
    pub answer_box: Option<AnswerBox>,
    #[serde(rename = "knowledgeGraph")]
    pub knowledge_graph: Option<KnowledgeGraph>,
    #[serde(rename = "searchParameters")]
    pub search_parameters: SearchParameters,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub link: String,
    pub snippet: String,
    #[serde(rename = "displayLink")]
    pub display_link: Option<String>,
    pub position: u32,
    pub date: Option<String>,
    #[serde(rename = "sitelinks")]
    pub site_links: Option<Vec<SiteLink>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteLink {
    pub title: String,
    pub link: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeopleAlsoAsk {
    pub question: String,
    pub answer: String,
    pub title: String,
    pub link: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedSearch {
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageResult {
    pub title: String,
    #[serde(rename = "imageUrl")]
    pub image_url: String,
    #[serde(rename = "imageWidth")]
    pub image_width: Option<u32>,
    #[serde(rename = "imageHeight")]
    pub image_height: Option<u32>,
    #[serde(rename = "thumbnailUrl")]
    pub thumbnail_url: Option<String>,
    pub source: String,
    pub link: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsResult {
    pub title: String,
    pub link: String,
    pub snippet: String,
    pub date: String,
    pub source: String,
    #[serde(rename = "imageUrl")]
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerBox {
    pub title: Option<String>,
    pub answer: String,
    pub link: Option<String>,
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeGraph {
    pub title: String,
    #[serde(rename = "type")]
    pub kg_type: Option<String>,
    pub website: Option<String>,
    #[serde(rename = "imageUrl")]
    pub image_url: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "descriptionSource")]
    pub description_source: Option<String>,
    #[serde(rename = "descriptionLink")]
    pub description_link: Option<String>,
    pub attributes: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchParameters {
    pub q: String,
    #[serde(rename = "type")]
    pub search_type: String,
    pub engine: String,
    pub gl: Option<String>,
    pub hl: Option<String>,
    pub num: Option<u32>,
    pub start: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub message: String,
    pub error: Option<String>,
}

fn parse_response<T: DeserializeOwned + Debug>(response: Response) -> Result<T, SearchError> {
    let status = response.status();
    if status.is_success() {
        let body = response
            .json::<T>()
            .map_err(|err| from_reqwest_error("Failed to decode response body", err))?;

        trace!("Received response from Serper Search API: {body:?}");
        Ok(body)
    } else {
        // Try to parse error response
        match response.json::<ErrorResponse>() {
            Ok(error_body) => {
                trace!("Received {status} response from Serper Search API: {error_body:?}");

                let search_error = match status.as_u16() {
                    400 => SearchError::InvalidQuery,
                    401 => SearchError::BackendError("Invalid API key".to_string()),
                    403 => SearchError::BackendError("API access forbidden".to_string()),
                    429 => SearchError::RateLimited(60), // Default to 60 seconds
                    500 => SearchError::BackendError("Server error".to_string()),
                    _ => SearchError::BackendError(format!(
                        "Request failed with {}: {}",
                        status, error_body.message
                    )),
                };

                Err(search_error)
            }
            Err(_) => {
                // Fallback for non-JSON error responses
                Err(SearchError::BackendError(format!(
                    "Request failed with status {status}"
                )))
            }
        }
    }
}
