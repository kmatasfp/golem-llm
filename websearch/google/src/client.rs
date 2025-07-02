use golem_web_search::error::{ from_reqwest_error };
use golem_web_search::golem::web_search::web_search::SearchError;
use log::trace;
use reqwest::{ Client, Method, Response };
use serde::de::DeserializeOwned;
use serde::{ Deserialize, Serialize };
use std::fmt::Debug;

const BASE_URL: &str = "https://www.googleapis.com/customsearch/v1";

/// The Google Custom Search API client for web search.
pub struct CustomSearchApi {
    api_key: String,
    search_engine_id: String,
    client: Client,
}

impl CustomSearchApi {
    pub fn new(api_key: String, search_engine_id: String) -> Self {
        let client = Client::builder().build().expect("Failed to initialize HTTP client");
        Self {
            api_key,
            search_engine_id,
            client,
        }
    }

    pub fn search(&self, request: SearchRequest) -> Result<SearchResponse, SearchError> {
        trace!("Sending request to Google Custom Search API: {request:?}");

        let mut url = format!("{BASE_URL}?key={}&cx={}", self.api_key, self.search_engine_id);

        url.push_str(&format!("&q={}", urlencoding::encode(&request.q)));

        if let Some(num) = request.num {
            url.push_str(&format!("&num={}", num));
        }

        if let Some(start) = request.start {
            url.push_str(&format!("&start={}", start));
        }

        if let Some(safe) = &request.safe {
            url.push_str(&format!("&safe={}", safe));
        }

        if let Some(lr) = &request.lr {
            url.push_str(&format!("&lr={}", lr));
        }

        if let Some(gl) = &request.gl {
            url.push_str(&format!("&gl={}", gl));
        }

        if let Some(date_restrict) = &request.date_restrict {
            url.push_str(&format!("&dateRestrict={}", date_restrict));
        }

        if let Some(site_search) = &request.site_search {
            url.push_str(&format!("&siteSearch={}", urlencoding::encode(site_search)));
        }

        if let Some(site_search_filter) = &request.site_search_filter {
            url.push_str(&format!("&siteSearchFilter={}", site_search_filter));
        }

        if request.img_type.is_some() || request.img_size.is_some() {
            url.push_str("&searchType=image");

            if let Some(img_type) = &request.img_type {
                url.push_str(&format!("&imgType={}", img_type));
            }

            if let Some(img_size) = &request.img_size {
                url.push_str(&format!("&imgSize={}", img_size));
            }
        }

        let response: Response = self.client
            .request(Method::GET, &url)
            .send()
            .map_err(|err| from_reqwest_error("Request failed", err))?;

        parse_response(response)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub q: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_restrict: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_search: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_search_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub img_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub img_size: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<SearchUrl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queries: Option<SearchQueries>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<SearchContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_information: Option<SearchInformation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<SearchItem>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchUrl {
    #[serde(rename = "type")]
    pub url_type: String,
    pub template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQueries {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<Vec<QueryInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page: Option<Vec<QueryInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_page: Option<Vec<QueryInfo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryInfo {
    pub title: String,
    #[serde(rename = "totalResults")]
    pub total_results: String,
    #[serde(rename = "searchTerms")]
    pub search_terms: String,
    pub count: u32,
    #[serde(rename = "startIndex")]
    pub start_index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cx: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchContext {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facets: Option<Vec<Vec<ContextFacet>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextFacet {
    pub label: String,
    pub anchor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchInformation {
    #[serde(rename = "searchTime")]
    pub search_time: f64,
    #[serde(rename = "formattedSearchTime")]
    pub formatted_search_time: String,
    #[serde(rename = "totalResults")]
    pub total_results: String,
    #[serde(rename = "formattedTotalResults")]
    pub formatted_total_results: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchItem {
    pub kind: String,
    pub title: String,
    #[serde(rename = "htmlTitle")]
    pub html_title: String,
    pub link: String,
    #[serde(rename = "displayLink")]
    pub display_link: String,
    pub snippet: String,
    #[serde(rename = "htmlSnippet")]
    pub html_snippet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formatted_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html_formatted_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pagemap: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<Label>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageInfo {
    #[serde(rename = "contextLink")]
    pub context_link: String,
    pub height: u32,
    pub width: u32,
    #[serde(rename = "byteSize")]
    pub byte_size: u32,
    #[serde(rename = "thumbnailLink")]
    pub thumbnail_link: String,
    #[serde(rename = "thumbnailHeight")]
    pub thumbnail_height: u32,
    #[serde(rename = "thumbnailWidth")]
    pub thumbnail_width: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "label_with_op")]
    pub label_with_op: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: ErrorResponseDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponseDetails {
    pub code: u32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

fn parse_response<T: DeserializeOwned + Debug>(response: Response) -> Result<T, SearchError> {
    let status = response.status();
    if status.is_success() {
        let body = response
            .json::<T>()
            .map_err(|err| from_reqwest_error("Failed to decode response body", err))?;

        trace!("Received response from Google Custom Search API: {body:?}");

        Ok(body)
    } else {
        let error_body = response
            .json::<ErrorResponse>()
            .map_err(|err| from_reqwest_error("Failed to receive error response body", err))?;

        trace!("Received {status} response from Google Custom Search API: {error_body:?}");

        let search_error = match error_body.error.code {
            400 => SearchError::InvalidQuery,
            429 => SearchError::RateLimited(60), // Default to 60 seconds
            _ =>
                SearchError::BackendError(
                    format!("Request failed with {}: {}", status, error_body.error.message)
                ),
        };

        Err(search_error)
    }
}
