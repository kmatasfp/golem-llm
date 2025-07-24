use golem_graph::error::from_reqwest_error;
use golem_graph::error::mapping::map_http_status;
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

            // Use centralized error mapping
            Err(map_http_status(status_code, error_msg, &error_body))
        }
    }
}
