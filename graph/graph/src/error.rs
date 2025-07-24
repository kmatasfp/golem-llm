use crate::golem::graph::errors::GraphError;
use crate::golem::graph::types::ElementId;

/// Enhanced error mapping utilities for database providers
pub mod mapping {
    use super::*;

    /// Request error classification for network-level errors
    pub fn classify_request_error(err: &dyn std::error::Error) -> GraphError {
        let error_msg = err.to_string();

        // Check for timeout conditions
        if error_msg.contains("timeout") || error_msg.contains("timed out") {
            return GraphError::Timeout;
        }

        // Check for connection issues
        if error_msg.contains("connection") || error_msg.contains("connect") {
            if error_msg.contains("refused") || error_msg.contains("unreachable") {
                return GraphError::ServiceUnavailable(format!("Service unavailable: {err}"));
            }
            return GraphError::ConnectionFailed(format!("Connection failed: {err}"));
        }

        // Check for DNS/network issues
        if error_msg.contains("dns") || error_msg.contains("resolve") {
            return GraphError::ConnectionFailed(format!("DNS resolution failed: {err}"));
        }

        // Default case
        GraphError::ConnectionFailed(format!("Request failed: {err}"))
    }

    /// Extract element ID from error message or error body
    pub fn extract_element_id_from_message(message: &str) -> Option<ElementId> {
        // Look for patterns like "collection/key" or just "key"
        if let Ok(re) = regex::Regex::new(r"([a-zA-Z0-9_]+/[a-zA-Z0-9_-]+)") {
            if let Some(captures) = re.captures(message) {
                if let Some(matched) = captures.get(1) {
                    return Some(ElementId::StringValue(matched.as_str().to_string()));
                }
            }
        }

        // Look for quoted strings that might be IDs
        if let Ok(re) = regex::Regex::new(r#""([^"]+)""#) {
            if let Some(captures) = re.captures(message) {
                if let Some(matched) = captures.get(1) {
                    let id_str = matched.as_str();
                    if id_str.contains('/') || id_str.len() > 3 {
                        return Some(ElementId::StringValue(id_str.to_string()));
                    }
                }
            }
        }

        None
    }

    /// Extract element ID from structured error response
    pub fn extract_element_id_from_error_body(error_body: &serde_json::Value) -> Option<ElementId> {
        // Try to find document ID in various fields
        if let Some(doc_id) = error_body.get("_id").and_then(|v| v.as_str()) {
            return Some(ElementId::StringValue(doc_id.to_string()));
        }

        if let Some(doc_key) = error_body.get("_key").and_then(|v| v.as_str()) {
            return Some(ElementId::StringValue(doc_key.to_string()));
        }

        if let Some(handle) = error_body.get("documentHandle").and_then(|v| v.as_str()) {
            return Some(ElementId::StringValue(handle.to_string()));
        }

        if let Some(element_id) = error_body.get("element_id").and_then(|v| v.as_str()) {
            return Some(ElementId::StringValue(element_id.to_string()));
        }

        None
    }
}

impl<'a> From<&'a GraphError> for GraphError {
    fn from(e: &'a GraphError) -> GraphError {
        e.clone()
    }
}

/// Creates a GraphError from a reqwest error with context
pub fn from_reqwest_error(details: impl AsRef<str>, err: reqwest::Error) -> GraphError {
    if err.is_timeout() {
        GraphError::Timeout
    } else if err.is_request() {
        GraphError::ConnectionFailed(format!("{}: {}", details.as_ref(), err))
    } else if err.is_decode() {
        GraphError::InternalError(format!(
            "{}: Failed to decode response - {}",
            details.as_ref(),
            err
        ))
    } else {
        GraphError::InternalError(format!("{}: {}", details.as_ref(), err))
    }
}

/// Enhance error with element ID information when available
pub fn enhance_error_with_element_id(
    error: GraphError,
    error_body: &serde_json::Value,
) -> GraphError {
    match &error {
        GraphError::InternalError(msg) if msg.contains("Document not found") => {
            if let Some(element_id) = mapping::extract_element_id_from_error_body(error_body) {
                GraphError::ElementNotFound(element_id)
            } else {
                error
            }
        }
        GraphError::ConstraintViolation(msg) if msg.contains("Unique constraint violated") => {
            if let Some(element_id) = mapping::extract_element_id_from_error_body(error_body) {
                GraphError::DuplicateElement(element_id)
            } else {
                error
            }
        }
        _ => error,
    }
}
