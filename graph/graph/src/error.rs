use crate::golem::graph::errors::GraphError;
use crate::golem::graph::types::ElementId;

/// Enhanced error mapping utilities for database providers
pub mod mapping {
    use super::*;
    use std::collections::HashMap;

    /// Database-agnostic error mapper that can be specialized by providers
    pub struct ErrorMapper {
        pub database_type: String,
        error_code_mappings: HashMap<i64, fn(&str) -> GraphError>,
    }

    impl ErrorMapper {
        /// Create a new error mapper for a specific database type
        pub fn new(database_type: String) -> Self {
            Self {
                database_type,
                error_code_mappings: HashMap::new(),
            }
        }

        /// Register a database-specific error code mapping
        pub fn register_error_code(&mut self, error_code: i64, mapper: fn(&str) -> GraphError) {
            self.error_code_mappings.insert(error_code, mapper);
        }

        /// Map a database error to GraphError using registered mappings
        pub fn map_database_error(
            &self,
            error_code: i64,
            message: &str,
            error_body: &serde_json::Value,
        ) -> GraphError {
            if let Some(mapper) = self.error_code_mappings.get(&error_code) {
                mapper(message)
            } else {
                self.map_generic_error(error_code, message, error_body)
            }
        }

        /// Generic error mapping fallback
        fn map_generic_error(
            &self,
            error_code: i64,
            message: &str,
            _error_body: &serde_json::Value,
        ) -> GraphError {
            GraphError::InternalError(format!(
                "{} error [{}]: {}",
                self.database_type, error_code, message
            ))
        }
    }

    /// HTTP status code to GraphError mapping
    pub fn map_http_status(
        status: u16,
        message: &str,
        error_body: &serde_json::Value,
    ) -> GraphError {
        match status {
            // Authentication and Authorization
            401 => GraphError::AuthenticationFailed(message.to_string()),
            403 => GraphError::AuthorizationFailed(message.to_string()),

            // Client errors
            400 => {
                if is_query_error(message) {
                    GraphError::InvalidQuery(format!("Bad request - invalid query: {message}"))
                } else if is_property_type_error(message) {
                    GraphError::InvalidPropertyType(format!(
                        "Bad request - invalid property: {message}"
                    ))
                } else if is_schema_violation(message, error_body) {
                    GraphError::SchemaViolation(format!("Schema violation: {message}"))
                } else if is_constraint_violation(message) {
                    GraphError::ConstraintViolation(format!("Constraint violation: {message}"))
                } else {
                    GraphError::InternalError(format!("Bad request: {message}"))
                }
            }
            404 => {
                if let Some(element_id) = extract_element_id_from_message(message) {
                    GraphError::ElementNotFound(element_id)
                } else {
                    GraphError::InternalError(format!("Resource not found: {message}"))
                }
            }
            409 => {
                if is_duplicate_error(message) {
                    if let Some(element_id) = extract_element_id_from_message(message) {
                        GraphError::DuplicateElement(element_id)
                    } else {
                        GraphError::ConstraintViolation(format!(
                            "Duplicate constraint violation: {message}"
                        ))
                    }
                } else {
                    GraphError::TransactionConflict
                }
            }
            412 => GraphError::ConstraintViolation(format!("Precondition failed: {message}")),
            422 => GraphError::SchemaViolation(format!("Unprocessable entity: {message}")),
            429 => GraphError::ResourceExhausted(format!("Too many requests: {message}")),

            // Server errors
            500 => GraphError::InternalError(format!("Internal server error: {message}")),
            502 => GraphError::ServiceUnavailable(format!("Bad gateway: {message}")),
            503 => GraphError::ServiceUnavailable(format!("Service unavailable: {message}")),
            504 => GraphError::Timeout,
            507 => GraphError::ResourceExhausted(format!("Insufficient storage: {message}")),

            // Default fallback
            _ => GraphError::InternalError(format!("HTTP error [{status}]: {message}")),
        }
    }

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

    /// Check if message indicates a query syntax error
    fn is_query_error(message: &str) -> bool {
        let msg_lower = message.to_lowercase();
        msg_lower.contains("syntax")
            || msg_lower.contains("parse")
            || msg_lower.contains("query")
            || msg_lower.contains("invalid statement")
    }

    /// Check if message indicates a property type error
    fn is_property_type_error(message: &str) -> bool {
        let msg_lower = message.to_lowercase();
        msg_lower.contains("property")
            && (msg_lower.contains("type") || msg_lower.contains("invalid"))
    }

    /// Check if message indicates a schema violation
    fn is_schema_violation(message: &str, error_body: &serde_json::Value) -> bool {
        let msg_lower = message.to_lowercase();

        // Check for collection/schema related errors
        if msg_lower.contains("collection")
            && (msg_lower.contains("not found")
                || msg_lower.contains("does not exist")
                || msg_lower.contains("unknown"))
        {
            return true;
        }

        // Check for data type mismatches
        if msg_lower.contains("type")
            && (msg_lower.contains("mismatch") || msg_lower.contains("expected"))
        {
            return true;
        }

        // Check database-specific schema errors in error body
        if let Some(error_code) = error_body.get("code").and_then(|v| v.as_str()) {
            matches!(
                error_code,
                "schema_violation" | "collection_not_found" | "invalid_structure"
            )
        } else {
            false
        }
    }

    /// Check if message indicates a constraint violation
    fn is_constraint_violation(message: &str) -> bool {
        let msg_lower = message.to_lowercase();

        msg_lower.contains("constraint")
            || msg_lower.contains("unique")
            || msg_lower.contains("violation")
            || (msg_lower.contains("required") && msg_lower.contains("missing"))
            || msg_lower.contains("reference")
            || msg_lower.contains("foreign")
    }

    /// Check if message indicates a duplicate element error
    fn is_duplicate_error(message: &str) -> bool {
        let msg_lower = message.to_lowercase();

        msg_lower.contains("duplicate")
            || msg_lower.contains("already exists")
            || msg_lower.contains("conflict")
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
