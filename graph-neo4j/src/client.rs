use golem_graph::golem::graph::errors::GraphError;
use ureq::{Agent, Response};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::Value;

#[derive(Clone)]
pub(crate) struct Neo4jApi {
    base_url: String,
    database: String,
    auth_header: String,
    agent: Agent,
}

impl Neo4jApi {
    /// Pass in the database name instead of using "neo4j" everywhere.
    pub(crate) fn new(
        host: &str,
        port: u16,
        database: &str,
        username: &str,
        password: &str,
    ) -> Self {
        let base_url = format!("http://{}:{}", host, port);
        let auth = format!("{}:{}", username, password);
        let auth_header = format!("Basic {}", STANDARD.encode(auth.as_bytes()));
        let agent = Agent::new();   // ureq’s sync HTTP agent

        Neo4jApi {
            base_url,
            database: database.to_string(),
            auth_header,
            agent,
        }
    }

    /// Dynamically build the tx endpoint for the configured database.
    fn tx_endpoint(&self) -> String {
        format!("/db/{}/tx", self.database)
    }

    pub(crate) fn begin_transaction(&self) -> Result<String, GraphError> {
        let url = format!("{}{}", self.base_url, self.tx_endpoint());
        let resp = self
            .agent
            .post(&url)
            .set("Authorization", &self.auth_header)
            .call()
            .map_err(|e| GraphError::ConnectionFailed(e.to_string()))?;
        Self::ensure_success_and_get_location(resp)
    }

    pub(crate) fn execute_in_transaction(
        &self,
        tx_url: &str,
        statements: Value,
    ) -> Result<Value, GraphError> {
        println!("[Neo4jApi] Cypher request: {}", statements);
        let resp = self
            .agent
            .post(tx_url)
            .set("Authorization", &self.auth_header)
            .set("Content-Type", "application/json")
            .send_string(&statements.to_string())
            .map_err(|e| GraphError::ConnectionFailed(e.to_string()))?;
        let json = Self::ensure_success_and_json(resp)?;
        println!("[Neo4jApi] Cypher response: {}", json);
        Ok(json)
    }

    pub(crate) fn commit_transaction(&self, tx_url: &str) -> Result<(), GraphError> {
        let commit_url = format!("{}/commit", tx_url);
        let resp = self
            .agent
            .post(&commit_url)
            .set("Authorization", &self.auth_header)
            .call()
            .map_err(|e| GraphError::ConnectionFailed(e.to_string()))?;
        Self::ensure_success(resp).map(|_| ())
    }

    pub(crate) fn rollback_transaction(&self, tx_url: &str) -> Result<(), GraphError> {
        let resp = self
            .agent
            .delete(tx_url)
            .set("Authorization", &self.auth_header)
            .call()
            .map_err(|e| GraphError::ConnectionFailed(e.to_string()))?;
        Self::ensure_success(resp).map(|_| ())
    }

    // Helpers

    fn ensure_success(response: Response) -> Result<Response, GraphError> {
        if response.status() < 400 {
            Ok(response)
        } else {
             // pull the entire body as a string
             let text = response
             .into_string()
             .map_err(|e| GraphError::InternalError(e.to_string()))?;
         // then deserialize
         let err: Value = serde_json::from_str(&text)
             .map_err(|e| GraphError::InternalError(e.to_string()))?;
         Err(GraphError::TransactionFailed(err.to_string()))
        }
    }

    fn ensure_success_and_json(response: Response) -> Result<Value, GraphError> {
        let text = response
            .into_string()
            .map_err(|e| GraphError::InternalError(e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| GraphError::InternalError(e.to_string()))
    }

    fn ensure_success_and_get_location(response: Response) -> Result<String, GraphError> {
        if response.status() < 400 {
            response
                .header("Location")
                .map(|s| s.to_string())
                .ok_or_else(|| GraphError::InternalError("Missing Location header".into()))
        } else {
            let text = response
            .into_string()
            .map_err(|e| GraphError::InternalError(e.to_string()))?;
        let err: Value = serde_json::from_str(&text)
            .map_err(|e| GraphError::InternalError(e.to_string()))?;
        Err(GraphError::TransactionFailed(err.to_string()))
        }
    }
}
