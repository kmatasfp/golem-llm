use golem_graph::golem::graph::errors::GraphError;
use serde_json::{json, Value};
use ureq::{Agent, Response};
use uuid::Uuid;

#[derive(Clone)]
pub struct JanusGraphApi {
    endpoint: String,
    agent: Agent,
    session_id: String,
}

impl JanusGraphApi {
    pub fn new(
        host: &str,
        port: u16,
        _username: Option<&str>,
        _password: Option<&str>,
    ) -> Result<Self, GraphError> {
        let endpoint = format!("http://{}:{}/gremlin", host, port);
        let agent = Agent::new();
        // one session per Api
        let session_id = Uuid::new_v4().to_string();
        Ok(JanusGraphApi { endpoint, agent, session_id })
      
    }

    pub fn new_with_session(
        host: &str,
        port: u16,
        _username: Option<&str>,
        _password: Option<&str>,
        session_id: String,
    ) -> Result<Self, GraphError> {
        let endpoint = format!("http://{}:{}/gremlin", host, port);
        let agent = Agent::new();
        Ok(JanusGraphApi { endpoint, agent, session_id })
    }

    pub fn commit(&self) -> Result<(), GraphError> {
        // explicit commit in the same session
        self.execute("g.tx().commit()", None)?;
        self.execute("g.tx().open()", None)?;
        Ok(())
    }

    pub fn execute(&self, gremlin: &str, bindings: Option<Value>) -> Result<Value, GraphError> {
        let bindings = bindings.unwrap_or_else(|| json!({}));
        // now include both session and op:"eval"
        let request_body = json!({
            "gremlin": gremlin,
            "bindings": bindings,
            "session": self.session_id,
            "processor": "session",
            "op": "eval",
            
        });

        eprintln!("[JanusGraphApi] Executing Gremlin: {}\nBindings: {}", gremlin, bindings);
        let resp_result = self
            .agent
            .post(&self.endpoint)
            .set("Content-Type", "application/json")
            .send_string(&request_body.to_string());

        let resp = match resp_result {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let body = r.into_string().unwrap_or_default();
                return Err(GraphError::InvalidQuery(format!("HTTP {}: {}", code, body)));
            }
            Err(e) => return Err(GraphError::ConnectionFailed(e.to_string())),
        };

        Self::handle_response(resp)
    }

    fn _read(&self, gremlin: &str, bindings: Option<Value>) -> Result<Value, GraphError> {
        let bindings = bindings.unwrap_or_else(|| json!({}));
        let request_body = json!({
            "gremlin": gremlin,
            "bindings": bindings,
        });
        let resp = self.agent
            .post(&self.endpoint)
            .set("Content-Type", "application/json")
            .send_string(&request_body.to_string())
            .map_err(|e| GraphError::ConnectionFailed(e.to_string()))?;
        Self::handle_response(resp)
    }

    pub fn close_session(&self) -> Result<(), GraphError> {
        let request_body = json!({
            "session": self.session_id,
            "op": "close",
            "processor": "session"
        });
        let resp = self.agent
            .post(&self.endpoint)
            .set("Content-Type", "application/json")
            .send_string(&request_body.to_string())
            .map_err(|e| GraphError::ConnectionFailed(e.to_string()))?;
        Self::handle_response(resp).map(|_| ())
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    fn handle_response(response: Response) -> Result<Value, GraphError> {
        let status = response.status();
        let body = response.into_string()
            .map_err(|e| GraphError::InternalError(e.to_string()))?;
        if status < 400 {
            serde_json::from_str(&body)
                .map_err(|e| GraphError::InternalError(e.to_string()))
        } else {
            Err(GraphError::InvalidQuery(format!("{}: {}", status, body)))
        }
    }
}
