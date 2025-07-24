use golem_graph::error::from_reqwest_error;
use golem_graph::golem::graph::errors::GraphError;
use log::trace;
use reqwest::{Client, Response};
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Clone)]
pub struct JanusGraphApi {
    endpoint: String,
    client: Client,
    session_id: String,
}

impl JanusGraphApi {
    pub fn new(
        host: &str,
        port: u16,
        _username: Option<&str>,
        _password: Option<&str>,
    ) -> Result<Self, GraphError> {
        trace!("Initializing JanusGraphApi for host: {host}, port: {port}");
        let endpoint = format!("http://{host}:{port}/gremlin");
        let client = Client::builder()
            .build()
            .expect("Failed to initialize HTTP client");
        let session_id = Uuid::new_v4().to_string();
        Ok(JanusGraphApi {
            endpoint,
            client,
            session_id,
        })
    }

    pub fn new_with_session(
        host: &str,
        port: u16,
        _username: Option<&str>,
        _password: Option<&str>,
        session_id: String,
    ) -> Result<Self, GraphError> {
        trace!(
            "Initializing JanusGraphApi with session for host: {host}, port: {port}, session_id: {session_id}"
        );
        let endpoint = format!("http://{host}:{port}/gremlin");
        let client = Client::builder()
            .build()
            .expect("Failed to initialize HTTP client");
        Ok(JanusGraphApi {
            endpoint,
            client,
            session_id,
        })
    }

    pub fn commit(&self) -> Result<(), GraphError> {
        trace!("Commit transaction");
        self.execute("g.tx().commit()", None)?;
        self.execute("g.tx().open()", None)?;
        Ok(())
    }

    pub fn execute(&self, gremlin: &str, bindings: Option<Value>) -> Result<Value, GraphError> {
        trace!("Execute Gremlin query: {gremlin}");
        let bindings = bindings.unwrap_or_else(|| json!({}));
        let request_body = json!({
            "gremlin": gremlin,
            "bindings": bindings,
            "session": self.session_id,
            "processor": "session",
            "op": "eval",

        });

        trace!("[JanusGraphApi] DEBUG - Full request details:");
        trace!("[JanusGraphApi] Endpoint: {}", self.endpoint);
        trace!("[JanusGraphApi] Session ID: {}", self.session_id);
        trace!("[JanusGraphApi] Gremlin Query: {gremlin}");
        trace!(
            "[JanusGraphApi] Request Body: {}",
            serde_json::to_string_pretty(&request_body)
                .unwrap_or_else(|_| "Failed to serialize".to_string())
        );

        let body_string = serde_json::to_string(&request_body).map_err(|e| {
            GraphError::InternalError(format!("Failed to serialize request body: {e}"))
        })?;

        trace!(
            "[JanusGraphApi] Sending POST request to: {} with body length: {}",
            self.endpoint,
            body_string.len()
        );
        let response = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("Content-Length", body_string.len().to_string())
            .body(body_string)
            .send()
            .map_err(|e| {
                log::error!("[JanusGraphApi] ERROR - Request failed: {e}");
                from_reqwest_error("JanusGraph request failed", e)
            })?;

        log::info!(
            "[JanusGraphApi] Got response with status: {}",
            response.status()
        );
        Self::handle_response(response)
    }

    fn _read(&self, gremlin: &str, bindings: Option<Value>) -> Result<Value, GraphError> {
        trace!("Read Gremlin query: {gremlin}");
        let bindings = bindings.unwrap_or_else(|| json!({}));
        let request_body = json!({
            "gremlin": gremlin,
            "bindings": bindings,
        });

        let body_string = serde_json::to_string(&request_body).map_err(|e| {
            GraphError::InternalError(format!("Failed to serialize request body: {e}"))
        })?;

        let response = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("Content-Length", body_string.len().to_string())
            .body(body_string)
            .send()
            .map_err(|e| from_reqwest_error("JanusGraph read request failed", e))?;
        Self::handle_response(response)
    }

    pub fn close_session(&self) -> Result<(), GraphError> {
        trace!("Close session: {}", self.session_id);
        let request_body = json!({
            "session": self.session_id,
            "op": "close",
            "processor": "session"
        });

        let body_string = serde_json::to_string(&request_body).map_err(|e| {
            GraphError::InternalError(format!("Failed to serialize request body: {e}"))
        })?;

        let response = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("Content-Length", body_string.len().to_string())
            .body(body_string)
            .send()
            .map_err(|e| from_reqwest_error("JanusGraph close session failed", e))?;
        Self::handle_response(response).map(|_| ())
    }

    pub fn session_id(&self) -> &str {
        trace!("Get session ID: {}", self.session_id);
        &self.session_id
    }

    fn handle_response(response: Response) -> Result<Value, GraphError> {
        let status = response.status();
        let status_code = status.as_u16();

        if status.is_success() {
            let response_body: Value = response.json().map_err(|e| {
                GraphError::InternalError(format!("Failed to parse response body: {e}"))
            })?;
            Ok(response_body)
        } else {
            let error_body: Value = response.json().map_err(|e| {
                GraphError::InternalError(format!("Failed to read error response: {e}"))
            })?;

            let error_msg = error_body
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");

            Err(Self::map_janusgraph_error(
                status_code,
                error_msg,
                &error_body,
            ))
        }
    }

    fn map_janusgraph_error(
        status_code: u16,
        message: &str,
        error_body: &serde_json::Value,
    ) -> GraphError {
        if let Some(status) = error_body.get("status") {
            if let Some(status_obj) = status.as_object() {
                if let Some(code) = status_obj.get("code").and_then(|c| c.as_u64()) {
                    return Self::from_gremlin_status_code(code as u16, message, error_body);
                }
            }
        }

        let msg_lower = message.to_lowercase();

        if (msg_lower.contains("groovy") || msg_lower.contains("gremlin"))
            && (msg_lower.contains("syntax") || msg_lower.contains("compilation"))
        {
            return GraphError::InvalidQuery(format!("Gremlin syntax error: {message}"));
        }

        if msg_lower.contains("vertex") && msg_lower.contains("not found") {
            return GraphError::InternalError(format!("JanusGraph vertex not found: {message}"));
        }

        if msg_lower.contains("edge") && msg_lower.contains("not found") {
            return GraphError::InternalError(format!("JanusGraph edge not found: {message}"));
        }

        if msg_lower.contains("property") && msg_lower.contains("not found") {
            return GraphError::SchemaViolation(format!(
                "JanusGraph property not found: {message}"
            ));
        }

        if msg_lower.contains("timeout") || msg_lower.contains("timed out") {
            return GraphError::Timeout;
        }

        if msg_lower.contains("transaction") {
            return GraphError::TransactionFailed(format!(
                "JanusGraph transaction error: {message}"
            ));
        }

        Self::map_janusgraph_http_status(status_code, message, error_body)
    }

    /// Gremlin server status code mapping
    fn from_gremlin_status_code(
        code: u16,
        message: &str,
        _error_body: &serde_json::Value,
    ) -> GraphError {
        match code {
            // Gremlin server status codes
            200 => {
                GraphError::InternalError(format!("Unexpected success in error context: {message}"))
            }
            204 => GraphError::InternalError(format!("No content: {message}")),
            206 => GraphError::InternalError(format!("Partial content: {message}")),
            401 => GraphError::AuthenticationFailed(format!(
                "Gremlin authentication failed: {message}"
            )),
            403 => {
                GraphError::AuthorizationFailed(format!("Gremlin authorization failed: {message}"))
            }
            407 => GraphError::AuthenticationFailed(format!(
                "Gremlin authentication challenge: {message}"
            )),
            498 => GraphError::InvalidQuery(format!("Gremlin malformed request: {message}")),
            499 => {
                GraphError::InvalidQuery(format!("Gremlin invalid request arguments: {message}"))
            }
            500 => GraphError::InternalError(format!("Gremlin server error: {message}")),
            597 => GraphError::InvalidQuery(format!("Gremlin script evaluation error: {message}")),
            598 => GraphError::Timeout,
            599 => GraphError::InternalError(format!("Gremlin serialization error: {message}")),

            _ => GraphError::InternalError(format!("Gremlin error [{code}]: {message}")),
        }
    }

    fn map_janusgraph_http_status(
        status: u16,
        message: &str,
        error_body: &serde_json::Value,
    ) -> GraphError {
        match status {
            // Authentication and Authorization
            401 => GraphError::AuthenticationFailed(format!(
                "JanusGraph authentication failed: {message}"
            )),
            403 => GraphError::AuthorizationFailed(format!(
                "JanusGraph authorization failed: {message}"
            )),

            // Client errors
            400 => GraphError::InvalidQuery(format!("JanusGraph bad request: {message}")),
            404 => {
                GraphError::ServiceUnavailable(format!("JanusGraph endpoint not found: {message}"))
            }
            409 => GraphError::TransactionConflict,
            429 => {
                GraphError::ResourceExhausted(format!("JanusGraph rate limit exceeded: {message}"))
            }

            // Server errors
            500 => {
                GraphError::InternalError(format!("JanusGraph internal server error: {message}"))
            }
            502 => GraphError::ServiceUnavailable(format!("JanusGraph bad gateway: {message}")),
            503 => {
                GraphError::ServiceUnavailable(format!("JanusGraph service unavailable: {message}"))
            }
            504 => GraphError::Timeout,

            // Default fallback with debug info
            _ => {
                let debug_info = format!(
                    "JanusGraph HTTP error [{}]: {} | Error body sample: {}",
                    status,
                    message,
                    error_body.to_string().chars().take(200).collect::<String>()
                );
                GraphError::InternalError(debug_info)
            }
        }
    }
}
