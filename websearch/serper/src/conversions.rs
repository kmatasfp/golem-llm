use crate::client::{ SearchRequest, SearchResponse, SearchResult as SerperSearchResult };
use golem_web_search::golem::web_search::types::{ ImageResult, SafeSearchLevel, TimeRange };
use golem_web_search::golem::web_search::web_search::{
    SearchParams,
    SearchResult,
    SearchMetadata,
    SearchError,
};

pub fn params_to_request(params: SearchParams) -> Result<SearchRequest, SearchError> {
    // Validate query
    if params.query.trim().is_empty() {
        return Err(SearchError::InvalidQuery);
    }

    // Convert region to Google country code
    let gl = params.region.map(|region| {
        // Convert common region formats to Google country codes
        match region.to_lowercase().as_str() {
            "us" | "usa" | "united states" => "us".to_string(),
            "uk" | "gb" | "united kingdom" => "uk".to_string(),
            "in" | "india" => "in".to_string(),
            "ca" | "canada" => "ca".to_string(),
            "au" | "australia" => "au".to_string(),
            "de" | "germany" => "de".to_string(),
            "fr" | "france" => "fr".to_string(),
            "jp" | "japan" => "jp".to_string(),
            "br" | "brazil" => "br".to_string(),
            "mx" | "mexico" => "mx".to_string(),
            _ => region, // Pass through as-is for other codes
        }
    });

    // Convert language to Google language code
    let hl = params.language.map(|lang| {
        match lang.to_lowercase().as_str() {
            "english" | "en" => "en".to_string(),
            "spanish" | "es" => "es".to_string(),
            "french" | "fr" => "fr".to_string(),
            "german" | "de" => "de".to_string(),
            "italian" | "it" => "it".to_string(),
            "portuguese" | "pt" => "pt".to_string(),
            "russian" | "ru" => "ru".to_string(),
            "japanese" | "ja" => "ja".to_string(),
            "korean" | "ko" => "ko".to_string(),
            "chinese" | "zh" => "zh".to_string(),
            "hindi" | "hi" => "hi".to_string(),
            "arabic" | "ar" => "ar".to_string(),
            _ => lang, // Pass through as-is for other codes
        }
    });

    // Convert safe search level
    let safe = params.safe_search.map(|level| {
        match level {
            SafeSearchLevel::Off => "off".to_string(),
            SafeSearchLevel::Medium | SafeSearchLevel::High => "active".to_string(),
        }
    });

    // Convert time range to Google time-based search filter
    let tbs = params.time_range.map(|range| {
        match range {
            TimeRange::Day => "qdr:d".to_string(), // Past day
            TimeRange::Week => "qdr:w".to_string(), // Past week
            TimeRange::Month => "qdr:m".to_string(), // Past month
            TimeRange::Year => "qdr:y".to_string(), // Past year
        }
    });

    // Determine search type based on include_images
    let tbm = if params.include_images == Some(true) {
        Some("isch".to_string()) // Image search
    } else {
        None // Web search (default)
    };

    // Handle domain filtering by modifying the query
    let mut query = params.query.clone();

    if let Some(include_domains) = &params.include_domains {
        if !include_domains.is_empty() {
            // Add site: operators for included domains
            let site_filters: Vec<String> = include_domains
                .iter()
                .map(|domain| format!("site:{}", domain))
                .collect();
            query = format!("{} ({})", query, site_filters.join(" OR "));
        }
    }

    if let Some(exclude_domains) = &params.exclude_domains {
        if !exclude_domains.is_empty() {
            // Add -site: operators for excluded domains
            let exclude_filters: Vec<String> = exclude_domains
                .iter()
                .map(|domain| format!("-site:{}", domain))
                .collect();
            query = format!("{} {}", query, exclude_filters.join(" "));
        }
    }

    Ok(SearchRequest {
        q: query,
        gl,
        hl,
        num: params.max_results,
        start: None, // Will be set during pagination
        safe,
        tbm,
        tbs,
        autocorrect: Some(true), // Enable autocorrect by default
    })
}

pub fn response_to_results(
    response: SearchResponse,
    original_params: &SearchParams,
    start_index: u32
) -> (Vec<SearchResult>, Option<SearchMetadata>) {
    let mut results = Vec::new();

    // If we have an answer box, create a special result for it
    if let Some(answer_box) = &response.answer_box {
        let answer_result = SearchResult {
            title: answer_box.title.clone().unwrap_or_else(|| "Answer".to_string()),
            url: answer_box.link.clone().unwrap_or_else(|| "https://google.com".to_string()),
            snippet: answer_box.answer.clone(),
            display_url: Some("google.com".to_string()),
            source: Some("Google Answer Box".to_string()),
            score: Some(1.0), // Highest score for answer box
            html_snippet: None,
            date_published: None,
            images: None,
            content_chunks: Some(vec![answer_box.answer.clone()]),
        };
        results.push(answer_result);
    }

    // Process organic search results
    for item in &response.organic {
        results.push(
            serper_result_to_search_result(
                item,
                original_params.include_images.unwrap_or(false),
                &response.images
            )
        );
    }

    // Add image results if requested and available
    if original_params.include_images == Some(true) {
        if let Some(images) = &response.images {
            for (index, img) in images.iter().enumerate() {
                let image_result = SearchResult {
                    title: img.title.clone(),
                    url: img.link.clone(),
                    snippet: format!("Image from {}", img.source),
                    display_url: extract_domain(&img.link),
                    source: Some(img.source.clone()),
                    score: Some((0.8 - (index as f32) * 0.05) as f64), // Slightly lower score for images
                    html_snippet: None,
                    date_published: None,
                    images: Some(
                        vec![ImageResult {
                            url: img.image_url.clone(),
                            description: Some(img.title.clone()),
                        }]
                    ),
                    content_chunks: None,
                };
                results.push(image_result);
            }
        }
    }

    let metadata = create_search_metadata(&response, original_params, start_index);
    (results, Some(metadata))
}

fn serper_result_to_search_result(
    item: &SerperSearchResult,
    include_images: bool,
    response_images: &Option<Vec<crate::client::ImageResult>>
) -> SearchResult {
    let mut images = None;
    let mut content_chunks = None;

    // Extract images if requested and available
    if include_images {
        if let Some(img_results) = response_images {
            if !img_results.is_empty() {
                // Take first few images related to this result
                images = Some(
                    img_results
                        .iter()
                        .take(3) // Limit to 3 images per result
                        .map(|img| ImageResult {
                            url: img.image_url.clone(),
                            description: Some(img.title.clone()),
                        })
                        .collect()
                );
            }
        }
    }

    // Create content chunks from snippet and site links
    let mut chunks = Vec::new();

    // Add main snippet
    if !item.snippet.is_empty() {
        chunks.push(item.snippet.clone());
    }

    // Add site links content if available
    if let Some(site_links) = &item.site_links {
        for link in site_links {
            chunks.push(format!("{}: {}", link.title, link.link));
        }
    }

    if !chunks.is_empty() {
        content_chunks = Some(chunks);
    }

    // Calculate score based on position (higher position = lower score)
    let score = 1.0 - ((item.position as f32) - 1.0) * 0.05;

    SearchResult {
        title: item.title.clone(),
        url: item.link.clone(),
        snippet: item.snippet.clone(),
        display_url: item.display_link.clone().or_else(|| extract_domain(&item.link)),
        source: extract_domain(&item.link),
        score: Some(score.max(0.1) as f64), // Ensure minimum score
        html_snippet: None,
        date_published: item.date.clone(),
        images,
        content_chunks,
    }
}

fn extract_domain(url: &str) -> Option<String> {
    if let Ok(parsed_url) = url::Url::parse(url) {
        parsed_url.host_str().map(|host| host.to_string())
    } else {
        None
    }
}

fn create_search_metadata(
    response: &SearchResponse,
    params: &SearchParams,
    start_index: u32
) -> SearchMetadata {
    // Serper doesn't provide total results count directly, so we estimate
    let total_results = if response.organic.len() >= (params.max_results.unwrap_or(10) as usize) {
        Some(1000000u64) // Conservative estimate for Google results
    } else {
        Some((start_index as u64) + (response.organic.len() as u64))
    };

    // Generate next page token if there are more results available
    let next_page_token = if response.organic.len() >= (params.max_results.unwrap_or(10) as usize) {
        Some((start_index + params.max_results.unwrap_or(10)).to_string())
    } else {
        None
    };

    SearchMetadata {
        query: params.query.clone(),
        total_results,
        search_time_ms: None, // Serper doesn't provide search time
        safe_search: params.safe_search,
        language: params.language.clone(),
        region: params.region.clone(),
        next_page_token,
        rate_limits: None,
    }
}

pub fn validate_search_params(params: &SearchParams) -> Result<(), SearchError> {
    if params.query.trim().is_empty() {
        return Err(SearchError::InvalidQuery);
    }

    if let Some(max_results) = params.max_results {
        if max_results > 100 {
            return Err(
                SearchError::UnsupportedFeature(
                    "max_results cannot exceed 100 for Serper Search".to_string()
                )
            );
        }
        if max_results == 0 {
            return Err(SearchError::InvalidQuery);
        }
    }

    // Serper supports most features, but validate specific constraints
    if let Some(region) = &params.region {
        if region.len() > 10 {
            return Err(SearchError::InvalidQuery);
        }
    }

    if let Some(language) = &params.language {
        if language.len() > 10 {
            return Err(SearchError::InvalidQuery);
        }
    }

    Ok(())
}
