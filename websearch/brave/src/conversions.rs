use crate::client::{ SearchRequest, SearchResponse, WebResult, ImageResult as BraveImageResult };
use golem_web_search::golem::web_search::types::{ ImageResult, SafeSearchLevel, TimeRange };
use golem_web_search::golem::web_search::web_search::{
    SearchError,
    SearchMetadata,
    SearchParams,
    SearchResult,
};
use log::{ trace, warn };

const ALLOWED_COUNTRIES: &[&str] = &[
    "AR",
    "AU",
    "AT",
    "BE",
    "BR",
    "CA",
    "CL",
    "DK",
    "FI",
    "FR",
    "DE",
    "HK",
    "IN",
    "ID",
    "IT",
    "JP",
    "KR",
    "MY",
    "MX",
    "NL",
    "NZ",
    "NO",
    "CN",
    "PL",
    "PT",
    "PH",
    "RU",
    "SA",
    "ZA",
    "ES",
    "SE",
    "CH",
    "TW",
    "TR",
    "GB",
    "US",
    "ALL",
];
const ALLOWED_UI_LANGS: &[&str] = &[
    "es-AR",
    "en-AU",
    "de-AT",
    "nl-BE",
    "fr-BE",
    "pt-BR",
    "en-CA",
    "fr-CA",
    "es-CL",
    "da-DK",
    "fi-FI",
    "fr-FR",
    "de-DE",
    "zh-HK",
    "en-IN",
    "en-ID",
    "it-IT",
    "ja-JP",
    "ko-KR",
    "en-MY",
    "es-MX",
    "nl-NL",
    "en-NZ",
    "no-NO",
    "zh-CN",
    "pl-PL",
    "en-PH",
    "ru-RU",
    "en-ZA",
    "es-ES",
    "sv-SE",
    "fr-CH",
    "de-CH",
    "zh-TW",
    "tr-TR",
    "en-GB",
    "en-US",
    "es-US",
];
const ALLOWED_RESULT_FILTERS: &[&str] = &[
    "discussions",
    "faq",
    "infobox",
    "news",
    "query",
    "videos",
    "web",
    "summarizer",
    "locations",
    "rich",
];

pub fn params_to_request(params: SearchParams) -> Result<SearchRequest, SearchError> {
    // Enhanced query validation
    let query = params.query.trim();
    if query.is_empty() {
        return Err(SearchError::InvalidQuery);
    }

    if query.len() > 400 {
        warn!("Query too long: {} characters", query.len());
        return Err(SearchError::InvalidQuery);
    }

    let safesearch = params.safe_search.map(|level| {
        match level {
            SafeSearchLevel::Off => "off".to_string(),
            SafeSearchLevel::Medium => "moderate".to_string(),
            SafeSearchLevel::High => "strict".to_string(),
        }
    });

    let freshness = params.time_range.map(|range| {
        match range {
            TimeRange::Day => "pd".to_string(),
            TimeRange::Week => "pw".to_string(),
            TimeRange::Month => "pm".to_string(),
            TimeRange::Year => "py".to_string(),
        }
    });

    // Validate max_results
    let count = params.max_results.map(|c| {
        if c > 20 {
            warn!("Max results {} exceeds API limit, capping at 20", c);
            20
        } else if c == 0 {
            warn!("Max results is 0, using default of 10");
            10
        } else {
            c
        }
    });

    // Handle domain exclusions in query (Brave API supports site: operator)
    let mut final_query = query.to_string();
    if let Some(exclude_domains) = &params.exclude_domains {
        for domain in exclude_domains {
            if !domain.trim().is_empty() {
                final_query.push_str(&format!(" -site:{}", domain.trim()));
            }
        }
    }

    // Validate and set country
    let country = params.region.as_ref().and_then(|region| {
        let region_up = region.to_uppercase();
        if ALLOWED_COUNTRIES.contains(&region_up.as_str()) {
            Some(region_up)
        } else {
            warn!("Invalid region code for Brave: {}", region);
            None
        }
    });

    // Validate and set ui_lang and search_lang (never both)
    let (ui_lang, search_lang) = match params.language.as_deref() {
        Some(lang) if ALLOWED_UI_LANGS.contains(&lang) => (Some(lang.to_string()), None),
        Some(lang) if lang.len() == 2 && lang.chars().all(|c| c.is_ascii_alphabetic()) =>
            (None, Some(lang.to_string())),
        _ => (None, None),
    };

    // Validate and set result_filter
    let result_filter = build_result_filter(&params);
    let result_filter = result_filter.and_then(|rf| {
        if ALLOWED_RESULT_FILTERS.contains(&rf.as_str()) {
            Some(rf)
        } else {
            warn!("Invalid result_filter for Brave: {}", rf);
            None
        }
    });

    Ok(SearchRequest {
        q: final_query,
        count,
        offset: None, // Will be set for pagination
        country,
        search_lang,
        ui_lang,
        safesearch,
        freshness,
        result_filter,
        goggles_id: None,
        units: None,
        spellcheck: None,
        extra_snippets: None,
    })
}

fn build_result_filter(params: &SearchParams) -> Option<String> {
    // Only add allowed result filters
    // Remove 'images' as it's not supported by Brave
    if params.include_images == Some(true) {
        // Brave does not support 'images' as a result_filter
        // If you want images, you must handle them differently
        None
    } else if matches!(params.time_range, Some(TimeRange::Day)) {
        Some("news".to_string())
    } else {
        None
    }
}

pub fn response_to_results(
    response: SearchResponse,
    original_params: &SearchParams
) -> (Vec<SearchResult>, Option<SearchMetadata>) {
    let mut results = Vec::new();

    trace!("Processing response with type: {}", response.response_type);

    // Process web results with better error handling
    if let Some(ref web_results) = response.web {
        trace!("Processing {} web results", web_results.results.len());
        for (index, item) in web_results.results.iter().enumerate() {
            if
                let Ok(result) = web_result_to_search_result(
                    item,
                    index,
                    original_params.include_images.unwrap_or(false)
                )
            {
                results.push(result);
            } else {
                warn!("Failed to convert web result at index {}", index);
            }
        }
    }

    // Process image results if requested
    if original_params.include_images == Some(true) {
        if let Some(ref image_results) = response.images {
            trace!("Processing {} image results", image_results.results.len());
            for (index, item) in image_results.results.iter().enumerate() {
                if let Ok(result) = image_result_to_search_result(item, index + results.len()) {
                    results.push(result);
                } else {
                    warn!("Failed to convert image result at index {}", index);
                }
            }
        }
    }

    let metadata = create_search_metadata(&response, original_params);
    (results, Some(metadata))
}

fn web_result_to_search_result(
    item: &WebResult,
    index: usize,
    include_images: bool
) -> Result<SearchResult, SearchError> {
    // Validate required fields
    if item.title.is_empty() || item.url.is_empty() {
        return Err(SearchError::BackendError("Invalid result: missing title or URL".to_string()));
    }

    let mut images = None;
    let mut content_chunks = None;

    // Extract images if requested and available
    if include_images {
        if let Some(thumbnail) = &item.thumbnail {
            if !thumbnail.src.is_empty() {
                images = Some(
                    vec![ImageResult {
                        url: thumbnail.src.clone(),
                        description: Some("Thumbnail".to_string()),
                    }]
                );
            }
        }
    }

    // Extract content chunks from various sources
    let mut chunks = Vec::new();

    if let Some(extra_snippets) = &item.extra_snippets {
        chunks.extend(
            extra_snippets
                .iter()
                .filter(|s| !s.trim().is_empty())
                .cloned()
        );
    }

    if let Some(subpages) = &item.subpages {
        for subpage in subpages {
            if !subpage.description.trim().is_empty() {
                chunks.push(subpage.description.clone());
            }
        }
    }

    if let Some(deep_results) = &item.deep_results {
        if let Some(deep_results_list) = &deep_results.results {
            for deep_result in deep_results_list {
                if !deep_result.description.trim().is_empty() {
                    chunks.push(deep_result.description.clone());
                }
            }
        }
    }

    if !chunks.is_empty() {
        content_chunks = Some(chunks);
    }

    // Calculate score based on multiple factors
    let score = calculate_result_score(index, item);

    Ok(SearchResult {
        title: item.title.clone(),
        url: item.url.clone(),
        snippet: item.description.clone(),
        display_url: item.meta_url.as_ref().map(|meta| meta.hostname.clone()),
        source: item.meta_url.as_ref().map(|meta| meta.hostname.clone()),
        score: Some(score.into()),
        html_snippet: None, // Brave doesn't provide HTML snippets
        date_published: item.date.clone(),
        images,
        content_chunks,
    })
}

fn image_result_to_search_result(
    item: &BraveImageResult,
    index: usize
) -> Result<SearchResult, SearchError> {
    if item.title.is_empty() || item.url.is_empty() {
        return Err(
            SearchError::BackendError("Invalid image result: missing title or URL".to_string())
        );
    }

    let images = Some(
        vec![ImageResult {
            url: item.url.clone(),
            description: Some(
                if let Some(properties) = &item.properties {
                    format!("{}x{}", properties.width, properties.height)
                } else {
                    "Image".to_string()
                }
            ),
        }]
    );

    Ok(SearchResult {
        title: item.title.clone(),
        url: item.source.clone(),
        snippet: format!("Image: {}", item.title),
        display_url: item.meta_url.as_ref().map(|meta| meta.hostname.clone()),
        source: item.meta_url.as_ref().map(|meta| meta.hostname.clone()),
        score: Some((1.0 - (index as f32) * 0.01).clamp(0.0, 1.0).into()),
        html_snippet: None,
        date_published: item.age.clone(),
        images,
        content_chunks: None,
    })
}

fn calculate_result_score(index: usize, item: &WebResult) -> f32 {
    let mut score = 1.0 - (index as f32) * 0.05; // Base score decreases with position

    // Quality indicators
    if item.family_friendly {
        score += 0.05;
    }

    if item.is_source_local {
        score += 0.03;
    }

    if item.extra_snippets.is_some() {
        score += 0.02;
    }

    if item.subpages.is_some() {
        score += 0.02;
    }

    if item.thumbnail.is_some() {
        score += 0.01;
    }

    // Boost for recent content
    if let Some(age) = &item.age {
        if age.contains("hour") || age.contains("minute") {
            score += 0.05;
        } else if age.contains("day") {
            score += 0.02;
        }
    }

    score.clamp(0.0, 1.0)
}

fn create_search_metadata(response: &SearchResponse, params: &SearchParams) -> SearchMetadata {
    let more_results_available = response.query.more_results_available;

    let total_results = if more_results_available {
        // Conservative estimate for pagination
        Some(params.max_results.unwrap_or(10) * 10)
    } else {
        // Count actual results
        let web_count = response.web
            .as_ref()
            .map(|w| w.results.len() as u32)
            .unwrap_or(0);
        let image_count = if params.include_images == Some(true) {
            response.images
                .as_ref()
                .map(|i| i.results.len() as u32)
                .unwrap_or(0)
        } else {
            0
        };
        Some(web_count + image_count)
    };

    SearchMetadata {
        query: params.query.clone(),
        total_results: total_results.map(|x| x as u64),
        search_time_ms: None, // Brave API doesn't provide search time
        safe_search: params.safe_search,
        language: params.language.clone(),
        region: params.region.clone(),
        next_page_token: if more_results_available {
            Some("next".to_string())
        } else {
            None
        },
        rate_limits: None, // Could be extracted from response headers if available
    }
}

pub fn _create_pagination_request(original_request: SearchRequest, offset: u32) -> SearchRequest {
    // Validate offset
    let safe_offset = if offset > 9980 { 9980 } else { offset };

    SearchRequest {
        offset: Some(safe_offset),
        ..original_request
    }
}

pub fn _extract_next_page_offset(
    response: &SearchResponse,
    current_offset: u32,
    count: u32
) -> Option<u32> {
    if response.query.more_results_available {
        let next_offset = current_offset + count;
        if next_offset <= 9980 {
            // Brave API limit
            Some(next_offset)
        } else {
            None
        }
    } else {
        None
    }
}

pub fn validate_search_params(params: &SearchParams) -> Result<(), SearchError> {
    // Query validation
    if params.query.trim().is_empty() {
        return Err(SearchError::InvalidQuery);
    }

    if params.query.len() > 400 {
        return Err(SearchError::InvalidQuery);
    }

    // Max results validation
    if let Some(max_results) = params.max_results {
        if max_results == 0 || max_results > 20 {
            return Err(SearchError::InvalidQuery);
        }
    }

    // Language validation
    if let Some(ref language) = params.language {
        if
            !language.is_empty() &&
            (language.len() != 2 || !language.chars().all(|c| c.is_ascii_alphabetic()))
        {
            return Err(SearchError::InvalidQuery);
        }
    }

    // Region validation
    if let Some(ref region) = params.region {
        if
            !region.is_empty() &&
            (region.len() != 2 || !region.chars().all(|c| c.is_ascii_alphabetic()))
        {
            return Err(SearchError::InvalidQuery);
        }
    }

    Ok(())
}
