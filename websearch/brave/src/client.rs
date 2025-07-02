use golem_web_search::error::{ from_reqwest_error, error_from_status };
use golem_web_search::golem::web_search::web_search::SearchError;
use log::{ trace, warn };
use reqwest::{ Client, Response };
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::{ Deserialize, Serialize };
use std::fmt::Debug;
use std::time::Duration;

const BASE_URL: &str = "https://api.search.brave.com/res/v1/web/search";

/// The Brave Search API client for web search.
pub struct BraveSearchApi {
    api_key: String,
    client: Client,
}

impl BraveSearchApi {
    /// Creates a new BraveSearchApi client with the provided API key
    pub fn new(api_key: String) -> Self {
        let client = Client::builder()
            .user_agent("Golem-Web-Search/1.0")
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to initialize HTTP client");

        Self { api_key, client }
    }

    /// Performs a search using the Brave Search API
    pub fn search(&self, request: SearchRequest) -> Result<SearchResponse, SearchError> {
        // Validate request before sending
        self.validate_request(&request)?;

        trace!("Sending request to Brave Search API: {request:?}");

        // Build URL using reqwest's built-in URL builder for better encoding
        let mut url = reqwest::Url
            ::parse(BASE_URL)
            .map_err(|e| SearchError::BackendError(format!("Invalid base URL: {}", e)))?;

        {
            let mut query_pairs = url.query_pairs_mut();
            query_pairs.append_pair("q", &request.q);
            if let Some(count) = request.count {
                if count > 0 && count <= 20 {
                    // Brave API limit
                    query_pairs.append_pair("count", &count.to_string());
                }
            }

            if let Some(offset) = request.offset {
                query_pairs.append_pair("offset", &offset.to_string());
            }

            if let Some(ref country) = request.country {
                if !country.is_empty() && country.len() == 2 {
                    // ISO country codes are 2 letters
                    query_pairs.append_pair("country", country);
                }
            }

            if let Some(ref search_lang) = request.search_lang {
                if !search_lang.is_empty() {
                    query_pairs.append_pair("search_lang", search_lang);
                }
            }

            if let Some(ref ui_lang) = request.ui_lang {
                if !ui_lang.is_empty() {
                    query_pairs.append_pair("ui_lang", ui_lang);
                }
            }

            if let Some(ref safesearch) = request.safesearch {
                if ["off", "moderate", "strict"].contains(&safesearch.as_str()) {
                    query_pairs.append_pair("safesearch", safesearch);
                }
            }

            if let Some(ref freshness) = request.freshness {
                if ["pd", "pw", "pm", "py"].contains(&freshness.as_str()) {
                    query_pairs.append_pair("freshness", freshness);
                }
            }

            if let Some(ref result_filter) = request.result_filter {
                if !result_filter.is_empty() {
                    query_pairs.append_pair("result_filter", result_filter);
                }
            }

            if let Some(ref goggles_id) = request.goggles_id {
                if !goggles_id.is_empty() {
                    query_pairs.append_pair("goggles_id", goggles_id);
                }
            }

            if let Some(ref units) = request.units {
                if ["metric", "imperial"].contains(&units.as_str()) {
                    query_pairs.append_pair("units", units);
                }
            }

            if let Some(spellcheck) = request.spellcheck {
                query_pairs.append_pair("spellcheck", &spellcheck.to_string());
            }

            if let Some(extra_snippets) = request.extra_snippets {
                query_pairs.append_pair("extra_snippets", &extra_snippets.to_string());
            }
        }

        trace!("Final URL: {}", url.as_str());

        let response: Response = self.client
            .request(Method::GET, url)
            .header("X-Subscription-Token", &self.api_key)
            .header("Accept", "application/json")
            .header("User-Agent", "Golem-Web-Search/1.0")
            .send()
            .map_err(|err| {
                warn!("Request failed: {}", err);
                from_reqwest_error("Request failed", err)
            })?;

        parse_response(response)
    }

    /// Validates the search request parameters
    fn validate_request(&self, request: &SearchRequest) -> Result<(), SearchError> {
        // Validate query
        if request.q.trim().is_empty() {
            return Err(SearchError::InvalidQuery);
        }

        if request.q.len() > 400 {
            // Brave API query length limit
            return Err(SearchError::InvalidQuery);
        }

        // Validate count
        if let Some(count) = request.count {
            if count == 0 || count > 20 {
                return Err(SearchError::InvalidQuery);
            }
        }

        // Validate offset
        if let Some(offset) = request.offset {
            if offset > 9980 {
                // Brave API offset limit
                return Err(SearchError::InvalidQuery);
            }
        }

        // Validate country code
        if let Some(ref country) = request.country {
            if !country.is_empty() && country.len() != 2 {
                return Err(SearchError::InvalidQuery);
            }
        }

        Ok(())
    }
}

// Request and Response Structures

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    /// The search query term
    pub q: String,
    /// Number of search results to return (1-20)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// The zero-based offset for pagination
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    /// Country code for results (2-letter ISO code)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    /// Search language
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_lang: Option<String>,
    /// User interface language
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_lang: Option<String>,
    /// Safe search setting: "off", "moderate", "strict"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safesearch: Option<String>,
    /// Time-based filtering: "pd" (past day), "pw" (past week), "pm" (past month), "py" (past year)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freshness: Option<String>,
    /// Result type filtering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_filter: Option<String>,
    /// Goggles ID for custom search lens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goggles_id: Option<String>,
    /// Unit system: "metric" or "imperial"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    /// Enable spellcheck
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spellcheck: Option<bool>,
    /// Include extra snippets in results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_snippets: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    #[serde(rename = "type")]
    pub response_type: String,
    pub query: QueryInfo,
    pub mixed: Option<MixedResults>,
    pub web: Option<WebResults>,
    pub images: Option<ImageResults>,
    pub videos: Option<VideoResults>,
    pub news: Option<NewsResults>,
    pub locations: Option<LocationResults>,
    pub discussions: Option<DiscussionResults>,
    pub infobox: Option<InfoboxResults>,
    pub faq: Option<FaqResults>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryInfo {
    pub original: String,
    pub show_strict_warning: bool,
    pub is_navigational: bool,
    pub is_news_breaking: bool,
    pub spellcheck_off: bool,
    pub country: String,
    pub bad_results: bool,
    pub should_fallback: bool,
    pub postal_code: Option<String>,
    pub city: Option<String>,
    pub header_country: Option<String>,
    pub more_results_available: bool,
    pub custom_location_label: Option<String>,
    pub reddit_cluster: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixedResults {
    #[serde(rename = "type")]
    pub result_type: String,
    pub main: Vec<MixedResult>,
    pub top: Vec<MixedResult>,
    pub side: Vec<MixedResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixedResult {
    #[serde(rename = "type")]
    pub result_type: String,
    #[serde(default)]
    pub index: u32,
    pub all: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebResults {
    #[serde(rename = "type")]
    pub result_type: String,
    pub results: Vec<WebResult>,
    pub family_friendly: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebResult {
    #[serde(rename = "type", default)]
    pub result_type: String,
    pub title: String,
    pub url: String,
    pub description: String,
    pub date: Option<String>,
    pub extra_snippets: Option<Vec<String>>,
    pub language: Option<String>,
    pub family_friendly: bool,
    pub profile: Option<ProfileInfo>,
    pub subpages: Option<Vec<SubpageInfo>>,
    pub deep_results: Option<DeepResults>,
    pub thumbnail: Option<ThumbnailInfo>,
    pub age: Option<String>,
    pub page_age: Option<String>,
    pub page_fetched: Option<String>,
    pub is_source_local: bool,
    pub is_source_both: bool,
    pub meta_url: Option<MetaUrl>,
    pub cluster: Option<Vec<ClusterResult>>,
    pub faq: Option<FaqInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileInfo {
    pub name: String,
    pub url: String,
    pub long_name: String,
    pub img: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubpageInfo {
    pub title: String,
    pub url: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepResults {
    pub buttons: Option<Vec<ButtonResult>>,
    pub results: Option<Vec<DeepResult>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButtonResult {
    #[serde(rename = "type", default)]
    pub result_type: String,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepResult {
    #[serde(rename = "type", default)]
    pub result_type: String,
    pub title: String,
    pub url: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThumbnailInfo {
    pub src: String,
    pub original: Option<String>,
    #[serde(default)]
    pub logo: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaUrl {
    pub scheme: String,
    pub netloc: String,
    pub hostname: String,
    pub favicon: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterResult {
    #[serde(rename = "type", default)]
    pub result_type: String,
    pub title: String,
    pub url: String,
    pub description: String,
    pub date: Option<String>,
    pub language: Option<String>,
    pub family_friendly: bool,
    pub age: Option<String>,
    pub page_age: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaqInfo {
    pub results: Vec<FaqResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageResults {
    #[serde(rename = "type")]
    pub result_type: String,
    pub results: Vec<ImageResult>,
    pub mutated_query: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageResult {
    #[serde(rename = "type", default)]
    pub result_type: String,
    pub title: String,
    pub url: String,
    pub source: String,
    pub thumbnail: ThumbnailInfo,
    pub properties: Option<ImageProperties>,
    pub meta_url: Option<MetaUrl>,
    pub age: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageProperties {
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub content_size: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoResults {
    #[serde(rename = "type")]
    pub result_type: String,
    pub results: Vec<VideoResult>,
    pub mutated_query: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoResult {
    #[serde(rename = "type", default)]
    pub result_type: String,
    pub title: String,
    pub url: String,
    pub description: String,
    pub date: Option<String>,
    pub duration: Option<String>,
    pub views: Option<String>,
    pub thumbnail: Option<ThumbnailInfo>,
    pub uploader: Option<String>,
    pub publisher: Option<String>,
    pub meta_url: Option<MetaUrl>,
    pub age: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsResults {
    #[serde(rename = "type")]
    pub result_type: String,
    pub results: Vec<NewsResult>,
    pub mutated_query: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsResult {
    #[serde(rename = "type", default)]
    pub result_type: String,
    pub title: String,
    pub url: String,
    pub description: String,
    pub date: Option<String>,
    pub thumbnail: Option<ThumbnailInfo>,
    pub language: Option<String>,
    pub family_friendly: bool,
    pub breaking: bool,
    pub age: Option<String>,
    pub meta_url: Option<MetaUrl>,
    pub cluster: Option<Vec<ClusterResult>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationResults {
    #[serde(rename = "type")]
    pub result_type: String,
    pub results: Vec<LocationResult>,
    pub mutated_query: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationResult {
    #[serde(rename = "type", default)]
    pub result_type: String,
    pub title: String,
    pub url: String,
    pub description: String,
    pub coordinates: Option<[f64; 2]>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
    pub city: Option<String>,
    pub phone: Option<String>,
    pub thumbnail: Option<ThumbnailInfo>,
    pub meta_url: Option<MetaUrl>,
    pub rating: Option<f32>,
    pub rating_count: Option<u32>,
    pub is_claimed: Option<bool>,
    pub reviews: Option<Vec<ReviewResult>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewResult {
    pub comment: String,
    pub date: Option<String>,
    pub rating: Option<f32>,
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscussionResults {
    #[serde(rename = "type")]
    pub result_type: String,
    pub results: Vec<DiscussionResult>,
    pub mutated_query: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscussionResult {
    #[serde(rename = "type", default)]
    pub result_type: String,
    pub title: String,
    pub url: String,
    pub description: String,
    pub date: Option<String>,
    pub forum: Option<String>,
    pub num_answers: Option<u32>,
    pub score: Option<f32>,
    pub is_question: bool,
    pub thumbnail: Option<ThumbnailInfo>,
    pub meta_url: Option<MetaUrl>,
    pub age: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfoboxResults {
    #[serde(rename = "type")]
    pub result_type: String,
    pub results: Vec<InfoboxResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfoboxResult {
    #[serde(rename = "type", default)]
    pub result_type: String,
    pub title: String,
    pub url: String,
    pub description: String,
    pub long_desc: Option<String>,
    pub thumbnail: Option<ThumbnailInfo>,
    pub attributes: Option<Vec<AttributeInfo>>,
    pub profiles: Option<Vec<ProfileInfo>>,
    pub website_url: Option<String>,
    pub meta_url: Option<MetaUrl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeInfo {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaqResults {
    #[serde(rename = "type")]
    pub result_type: String,
    pub results: Vec<FaqResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaqResult {
    #[serde(rename = "type", default)]
    pub result_type: String,
    pub question: String,
    pub answer: String,
    pub title: String,
    pub url: String,
    pub meta_url: Option<MetaUrl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
}

// Enhanced error parsing with better debugging
fn parse_response<T: DeserializeOwned + Debug>(response: Response) -> Result<T, SearchError> {
    let status = response.status();
    let url = response.url().clone();

    trace!("Response status: {} for URL: {}", status, url);

    if status.is_success() {
        let body_text = response.text().map_err(|err| {
            warn!("Failed to read response body: {}", err);
            from_reqwest_error("Failed to read response body", err)
        })?;
        trace!("Brave raw body: {}", body_text);
        let body = serde_json::from_str::<T>(&body_text).map_err(|err| {
            warn!("Failed to decode response body: {}", err);
            SearchError::BackendError(format!("Failed to decode response body: {}", err))
        })?;

        trace!("Received successful response from Brave Search API");
        Ok(body)
    } else {
        // Try to get the response body as text for better debugging
        match response.text() {
            Ok(body_text) => {
                warn!("Received {} response from Brave Search API. Body: {}", status, body_text);

                // Try to parse as ErrorResponse first
                if let Ok(error_body) = serde_json::from_str::<ErrorResponse>(&body_text) {
                    Err(error_from_status(status, Some(error_body.message)))
                } else {
                    // If we can't parse the error, include the raw body
                    Err(
                        SearchError::BackendError(
                            format!("Request failed with status {}: {}", status, body_text)
                        )
                    )
                }
            }
            Err(_) => {
                Err(SearchError::BackendError(format!("Request failed with status {}", status)))
            }
        }
    }
}
