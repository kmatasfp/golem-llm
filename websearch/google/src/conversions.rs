use crate::client::{ SearchItem, SearchRequest, SearchResponse };
use golem_web_search::golem::web_search::types::{ ImageResult, SafeSearchLevel, TimeRange };
use golem_web_search::golem::web_search::web_search::{
    SearchError,
    SearchMetadata,
    SearchParams,
    SearchResult,
};

pub fn params_to_request(params: SearchParams) -> Result<SearchRequest, SearchError> {
    // Validate query
    if params.query.trim().is_empty() {
        return Err(SearchError::InvalidQuery);
    }

    let safe = params.safe_search.map(|level| {
        match level {
            SafeSearchLevel::Off => "off".to_string(),
            SafeSearchLevel::Medium => "medium".to_string(),
            SafeSearchLevel::High => "high".to_string(),
        }
    });

    let date_restrict = params.time_range.map(|range| {
        match range {
            TimeRange::Day => "d1".to_string(),
            TimeRange::Week => "w1".to_string(),
            TimeRange::Month => "m1".to_string(),
            TimeRange::Year => "y1".to_string(),
        }
    });

    let site_search = if let Some(domains) = &params.include_domains {
        if !domains.is_empty() { Some(format!("site:{}", domains.join(" OR site:"))) } else { None }
    } else {
        None
    };

    let site_search_filter = if params.exclude_domains.is_some() {
        Some("e".to_string()) // Exclude sites
    } else if params.include_domains.is_some() {
        Some("i".to_string()) // Include sites only
    } else {
        None
    };

    // Handle excluded domains by modifying the query
    let mut query = params.query.clone();
    if let Some(exclude_domains) = &params.exclude_domains {
        for domain in exclude_domains {
            query.push_str(&format!(" -site:{}", domain));
        }
    }

    Ok(SearchRequest {
        q: query,
        num: params.max_results,
        start: None, // Will be set for pagination
        safe,
        lr: params.language.map(|lang| format!("lang_{}", lang)),
        gl: params.region,
        date_restrict,
        site_search,
        site_search_filter,
        img_type: None, // Set based on include_images
        img_size: None,
    })
}

pub fn response_to_results(
    response: SearchResponse,
    original_params: &SearchParams
) -> (Vec<SearchResult>, Option<SearchMetadata>) {
    let mut results = Vec::new();

    if let Some(ref items) = response.items {
        for item in items {
            results.push(
                item_to_search_result(item.clone(), original_params.include_images.unwrap_or(false))
            );
        }
    }

    let metadata = create_search_metadata(&response, original_params);

    (results, Some(metadata))
}

fn item_to_search_result(item: SearchItem, include_images: bool) -> SearchResult {
    let mut images = None;
    let mut content_chunks = None;

    // Extract images if requested
    if include_images {
        if let Some(image_info) = item.image {
            images = Some(
                vec![ImageResult {
                    url: image_info.context_link,
                    description: Some(format!("{}x{}", image_info.width, image_info.height)),
                }]
            );
        }

        // Also check pagemap for additional images
        if let Some(pagemap) = &item.pagemap {
            if let Some(cse_images) = pagemap.get("cse_image") {
                if let Some(cse_images_array) = cse_images.as_array() {
                    let mut pagemap_images = Vec::new();
                    for img in cse_images_array {
                        if let Some(src) = img.get("src").and_then(|s| s.as_str()) {
                            pagemap_images.push(ImageResult {
                                url: src.to_string(),
                                description: None,
                            });
                        }
                    }
                    if !pagemap_images.is_empty() {
                        images = Some(pagemap_images);
                    }
                }
            }
        }
    }

    // Extract content chunks from pagemap if available
    if let Some(pagemap) = &item.pagemap {
        let mut chunks = Vec::new();

        // Extract metatags
        if let Some(metatags) = pagemap.get("metatags") {
            if let Some(metatags_array) = metatags.as_array() {
                for meta in metatags_array {
                    if let Some(description) = meta.get("og:description").and_then(|d| d.as_str()) {
                        chunks.push(description.to_string());
                    }
                    if let Some(description) = meta.get("description").and_then(|d| d.as_str()) {
                        chunks.push(description.to_string());
                    }
                }
            }
        }

        // Extract webpage content if available
        if let Some(webpage) = pagemap.get("webpage") {
            if let Some(webpage_array) = webpage.as_array() {
                for page in webpage_array {
                    if let Some(description) = page.get("description").and_then(|d| d.as_str()) {
                        chunks.push(description.to_string());
                    }
                }
            }
        }

        if !chunks.is_empty() {
            content_chunks = Some(chunks);
        }
    }

    SearchResult {
        title: item.title,
        url: item.link.clone(),
        snippet: item.snippet,
        display_url: Some(item.display_link),
        source: extract_source_from_url(&item.link),
        score: None, // Google doesn't provide explicit scores
        html_snippet: Some(item.html_snippet),
        date_published: extract_date_from_pagemap(&item.pagemap),
        images,
        content_chunks,
    }
}

fn create_search_metadata(response: &SearchResponse, params: &SearchParams) -> SearchMetadata {
    let total_results = response.search_information
        .as_ref()
        .and_then(|info| info.total_results.parse::<u64>().ok());

    let search_time_ms = response.search_information.as_ref().map(|info| info.search_time * 1000.0); // Convert to milliseconds

    let next_page_token = response.queries
        .as_ref()
        .and_then(|q| q.next_page.as_ref())
        .and_then(|next| next.first())
        .map(|next_info| format!("start:{}", next_info.start_index));

    SearchMetadata {
        query: params.query.clone(),
        total_results,
        search_time_ms,
        safe_search: params.safe_search,
        language: params.language.clone(),
        region: params.region.clone(),
        next_page_token,
        rate_limits: None, // Google doesn't provide this in response
    }
}

fn extract_source_from_url(url: &str) -> Option<String> {
    if let Ok(parsed_url) = url::Url::parse(url) {
        parsed_url.host_str().map(|host| {
            // Remove www. prefix if present
            if let Some(stripped) = host.strip_prefix("www.") {
                stripped.to_string()
            } else {
                host.to_string()
            }
        })
    } else {
        None
    }
}

fn extract_date_from_pagemap(pagemap: &Option<serde_json::Value>) -> Option<String> {
    if let Some(pagemap) = pagemap {
        // Try to extract date from various metadata sources
        if let Some(metatags) = pagemap.get("metatags") {
            if let Some(metatags_array) = metatags.as_array() {
                for meta in metatags_array {
                    // Try different date fields
                    let date_fields = [
                        "article:published_time",
                        "article:modified_time",
                        "og:updated_time",
                        "date",
                        "publishdate",
                        "pubdate",
                    ];

                    for field in &date_fields {
                        if let Some(date) = meta.get(field).and_then(|d| d.as_str()) {
                            return Some(date.to_string());
                        }
                    }
                }
            }
        }

        // Try webpage section
        if let Some(webpage) = pagemap.get("webpage") {
            if let Some(webpage_array) = webpage.as_array() {
                for page in webpage_array {
                    if let Some(date) = page.get("datepublished").and_then(|d| d.as_str()) {
                        return Some(date.to_string());
                    }
                }
            }
        }
    }
    None
}

pub fn _create_pagination_request(original_request: SearchRequest, start: u32) -> SearchRequest {
    SearchRequest {
        start: Some(start),
        ..original_request
    }
}

pub fn _extract_next_page_start(response: &SearchResponse) -> Option<u32> {
    response.queries
        .as_ref()
        .and_then(|q| q.next_page.as_ref())
        .and_then(|next| next.first())
        .map(|next_info| next_info.start_index)
}

pub fn validate_search_params(params: &SearchParams) -> Result<(), SearchError> {
    if params.query.trim().is_empty() {
        return Err(SearchError::InvalidQuery);
    }

    if let Some(max_results) = params.max_results {
        if max_results > 100 {
            return Err(
                SearchError::UnsupportedFeature(
                    "max_results cannot exceed 100 for Google Custom Search".to_string()
                )
            );
        }
    }

    Ok(())
}
