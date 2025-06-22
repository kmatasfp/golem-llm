use base64::{engine::general_purpose, Engine as _};
use golem_graph::golem::graph::errors::GraphError;
use golem_graph::golem::graph::schema::{
    ContainerInfo, ContainerType, EdgeTypeDefinition, IndexDefinition, IndexType,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use ureq::{Agent, Response};

pub struct ArangoDbApi {
    base_url: String,
    agent: Agent,
    auth_header: String,
}

impl ArangoDbApi {
    pub fn new(host: &str, port: u16, username: &str, password: &str, database_name: &str) -> Self {
        let base_url = format!("http://{}:{}/_db/{}", host, port, database_name);
        let auth_header = format!(
            "Basic {}",
            general_purpose::STANDARD.encode(format!("{}:{}", username, password))
        );
        let agent = Agent::new();

        Self { base_url, agent, auth_header }
    }

    fn post(&self, endpoint: &str) -> ureq::Request {
        self.agent
            .post(&format!("{}{}", self.base_url, endpoint))
            .set("Authorization", &self.auth_header)
            .set("Content-Type", "application/json")
    }

    fn get(&self, endpoint: &str) -> ureq::Request {
        self.agent
            .get(&format!("{}{}", self.base_url, endpoint))
            .set("Authorization", &self.auth_header)
    }

    fn put(&self, endpoint: &str) -> ureq::Request {
        self.agent
            .put(&format!("{}{}", self.base_url, endpoint))
            .set("Authorization", &self.auth_header)
            .set("Content-Type", "application/json")
    }

    fn delete(&self, endpoint: &str) -> ureq::Request {
        self.agent
            .delete(&format!("{}{}", self.base_url, endpoint))
            .set("Authorization", &self.auth_header)
    }

    fn execute<T: DeserializeOwned>(&self, request: ureq::Request) -> Result<T, GraphError> {
        let resp_result = request.call();
        let resp = match resp_result {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let body = r.into_string().unwrap_or_default();
                return Err(self.map_error(code, &body));
            }
            Err(e) => return Err(GraphError::ConnectionFailed(e.to_string())),
        };
        self.handle_response(resp)
    }

    fn execute_json<T: DeserializeOwned>(&self, request: ureq::Request, body: &Value) -> Result<T, GraphError> {
        let body_str = body.to_string();
        let resp_result = request.send_string(&body_str);
        let resp = match resp_result {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let body = r.into_string().unwrap_or_default();
                return Err(self.map_error(code, &body));
            }
            Err(e) => return Err(GraphError::ConnectionFailed(e.to_string())),
        };
        self.handle_response(resp)
    }

    fn handle_response<T: DeserializeOwned>(&self, response: Response) -> Result<T, GraphError> {
        let status = response.status();
        let body_text = response.into_string().map_err(|e| {
            GraphError::InternalError(format!("Failed to read response body: {}", e))
        })?;
        
        let response_body: Value = serde_json::from_str(&body_text).map_err(|e| {
            GraphError::InternalError(format!("Failed to parse response body: {}", e))
        })?;

        if status >= 200 && status < 300 {
            if let Some(result) = response_body.get("result") {
                serde_json::from_value(result.clone()).map_err(|e| {
                    GraphError::InternalError(format!(
                        "Failed to deserialize successful response: {}",
                        e
                    ))
                })
            } else {
                serde_json::from_value(response_body).map_err(|e| {
                    GraphError::InternalError(format!(
                        "Failed to deserialize successful response: {}",
                        e
                    ))
                })
            }
        } else {
            let error_msg = response_body
                .get("errorMessage")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            Err(self.map_error(status, error_msg))
        }
    }

    fn map_error(&self, status: u16, message: &str) -> GraphError {
        match status {
            401 => GraphError::AuthenticationFailed(message.to_string()),
            403 => GraphError::AuthorizationFailed(message.to_string()),
            404 => {
                GraphError::InternalError(format!("Endpoint not found: {}", message))
            } // This might need more specific handling
            409 => GraphError::TransactionConflict,
            _ => GraphError::InternalError(format!("ArangoDB error: {} - {}", status, message)),
        }
    }

    pub fn begin_transaction(&self, read_only: bool) -> Result<String, GraphError> {
        let collections = if read_only {
            json!({ "read": [] })
        } else {
            json!({ "write": [] })
        };

        let body = json!({ "collections": collections });
        let request = self.post("/_api/transaction/begin");
        let result: Value = self.execute_json(request, &body)?;

        result
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                GraphError::InternalError("Missing transaction ID in response".to_string())
            })
    }

    pub fn begin_transaction_with_collections(&self, read_only: bool, collections: Vec<String>) -> Result<String, GraphError> {
        let collections_spec = if read_only {
            json!({ "read": collections })
        } else {
            json!({ "write": collections })
        };

        let body = json!({ "collections": collections_spec });
        let request = self.post("/_api/transaction/begin");
        let result: Value = self.execute_json(request, &body)?;

        result
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                GraphError::InternalError("Missing transaction ID in response".to_string())
            })
    }

    pub fn commit_transaction(&self, transaction_id: &str) -> Result<(), GraphError> {
        let endpoint = format!("/_api/transaction/{}", transaction_id);
        let request = self.put(&endpoint);
        let _: Value = self.execute(request)?;
        Ok(())
    }

    pub fn rollback_transaction(&self, transaction_id: &str) -> Result<(), GraphError> {
        let endpoint = format!("/_api/transaction/{}", transaction_id);
        let request = self.delete(&endpoint);
        let _: Value = self.execute(request)?;
        Ok(())
    }

    pub fn execute_in_transaction(
        &self,
        transaction_id: &str,
        query: Value,
    ) -> Result<Value, GraphError> {
        let request = self
            .post("/_api/cursor")
            .set("x-arango-trx-id", transaction_id);
        self.execute_json(request, &query)
    }

    pub fn ping(&self) -> Result<(), GraphError> {
        let request = self.get("/_api/version");
        let _: Value = self.execute(request)?;
        Ok(())
    }

    // Schema operations
    pub fn create_collection(
        &self,
        name: &str,
        container_type: ContainerType,
    ) -> Result<(), GraphError> {
        let collection_type = match container_type {
            ContainerType::VertexContainer => 2,
            ContainerType::EdgeContainer => 3,
        };
        let body = json!({ "name": name, "type": collection_type });
        let request = self.post("/_api/collection");
        let _: Value = self.execute_json(request, &body)?;
        Ok(())
    }

    pub fn list_collections(&self) -> Result<Vec<ContainerInfo>, GraphError> {
        let request = self.get("/_api/collection");
        let response: Value = self.execute(request)?;
        
        // Try to get the result array from the response
        let collections_array = if let Some(result) = response.get("result") {
            result.as_array().ok_or_else(|| {
                GraphError::InternalError("Invalid response for list_collections - result is not array".to_string())
            })?
        } else {
            // Fallback: try to use response directly as array (older API format)
            response.as_array().ok_or_else(|| {
                GraphError::InternalError("Invalid response for list_collections - no result field and response is not array".to_string())
            })?
        };
        
        let collections = collections_array
            .iter()
            .filter(|v| !v["isSystem"].as_bool().unwrap_or(false)) // Filter out system collections
            .map(|v| {
                let name = v["name"].as_str().unwrap_or_default().to_string();
                let coll_type = v["type"].as_u64().unwrap_or(2);
                let container_type = if coll_type == 3 {
                    ContainerType::EdgeContainer
                } else {
                    ContainerType::VertexContainer
                };
                ContainerInfo {
                    name,
                    container_type,
                    element_count: None,
                }
            })
            .collect();
        Ok(collections)
    }

    pub fn create_index(
        &self,
        collection: String,
        fields: Vec<String>,
        unique: bool,
        index_type: IndexType,
        name: Option<String>,
    ) -> Result<(), GraphError> {
        let type_str = match index_type {
            IndexType::Exact => "persistent",
            IndexType::Range => "persistent", // ArangoDB's persistent index supports range queries
            IndexType::Text => "inverted", // Full-text requires enterprise edition or arangosearch
            IndexType::Geospatial => "geo",
        };

        let mut body = json!({
            "type": type_str,
            "fields": fields,
            "unique": unique,
        });

        // Add name if provided
        if let Some(index_name) = name {
            body["name"] = json!(index_name);
        }

        let request = self.post(&format!("/_api/index?collection={}", collection));
        let _: Value = self.execute_json(request, &body)?;
        Ok(())
    }

    pub fn drop_index(&self, name: &str) -> Result<(), GraphError> {
        // First, find the index by name to get its ID
        let collections = self.list_collections()?;
        
        for collection in collections {
            let endpoint = format!("/_api/index?collection={}", collection.name);
            let request = self.get(&endpoint);
            
            if let Ok(response) = self.execute::<Value>(request) {
                if let Some(indexes) = response["indexes"].as_array() {
                    for idx in indexes {
                        if let Some(idx_name) = idx["name"].as_str() {
                            if idx_name == name {
                                if let Some(idx_id) = idx["id"].as_str() {
                                    let delete_request = self.delete(&format!("/_api/index/{}", idx_id));
                                    let _: Value = self.execute(delete_request)?;
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Err(GraphError::InternalError(format!("Index '{}' not found", name)))
    }

    pub fn list_indexes(&self) -> Result<Vec<IndexDefinition>, GraphError> {
        // Get all collections first
        let collections = self.list_collections()?;
        let mut all_indexes = Vec::new();
        
        for collection in collections {
            let endpoint = format!("/_api/index?collection={}", collection.name);
            let request = self.get(&endpoint);
            
            match self.execute::<Value>(request) {
                Ok(response) => {
                    if let Some(indexes) = response["indexes"].as_array() {
                        for index in indexes {
                            // Skip primary and edge indexes
                            if let Some(index_type) = index["type"].as_str() {
                                if index_type == "primary" || index_type == "edge" {
                                    continue;
                                }
                            }
                            
                            let name = index["name"].as_str().unwrap_or("").to_string();
                            let id = index["id"].as_str().unwrap_or("").to_string();
                            
                            let fields: Vec<String> = index["fields"]
                                .as_array()
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|f| f.as_str())
                                        .map(String::from)
                                        .collect()
                                })
                                .unwrap_or_default();
                            
                            if fields.is_empty() {
                                continue;
                            }
                            
                            let unique = index["unique"].as_bool().unwrap_or(false);
                            let index_type_str = index["type"].as_str().unwrap_or("persistent");
                            let index_type = match index_type_str {
                                "geo" => golem_graph::golem::graph::schema::IndexType::Geospatial,
                                "inverted" => golem_graph::golem::graph::schema::IndexType::Text,
                                _ => golem_graph::golem::graph::schema::IndexType::Exact,
                            };
                            
                            // Use a combination of collection and fields as logical name for matching
                            let logical_name = if fields.len() == 1 {
                                format!("idx_{}_{}", collection.name, fields[0])
                            } else {
                                format!("idx_{}_{}", collection.name, fields.join("_"))
                            };
                            
                            // Prefer the ArangoDB generated name, but fall back to our logical name
                            let final_name = if !name.is_empty() { name } else if !id.is_empty() { id } else { logical_name };
                            
                            all_indexes.push(IndexDefinition {
                                name: final_name,
                                label: collection.name.clone(),
                                container: Some(collection.name.clone()),
                                properties: fields,
                                unique,
                                index_type,
                            });
                        }
                    }
                }
                Err(_) => {
                    // Skip collections that we can't access
                    continue;
                }
            }
        }
        
        Ok(all_indexes)
    }

    pub fn get_index(&self, name: &str) -> Result<Option<IndexDefinition>, GraphError> {
        let all_indexes = self.list_indexes()?;
        
        // Try to find by exact name match first
        if let Some(index) = all_indexes.iter().find(|idx| idx.name == name) {
            return Ok(Some(index.clone()));
        }
        
        // If the requested name follows our pattern (idx_collection_field), try to match by properties
        if name.starts_with("idx_") {
            let parts: Vec<&str> = name.split('_').collect();
            if parts.len() >= 3 {
                let collection_part = parts[1];
                let field_part = parts[2..].join("_");
                
                if let Some(index) = all_indexes.iter().find(|idx| {
                    idx.label == collection_part && 
                    idx.properties.len() == 1 && 
                    idx.properties[0] == field_part
                }) {
                    return Ok(Some(index.clone()));
                }
            }
        }
        
        Ok(None)
    }

    pub fn define_edge_type(&self, definition: EdgeTypeDefinition) -> Result<(), GraphError> {
        // In ArangoDB, we just ensure the edge collection exists
        // The from/to collection constraints are not enforced at the database level
        // but are handled at the application level
        self.create_collection(&definition.collection, ContainerType::EdgeContainer)?;
        
        // Note: ArangoDB doesn't enforce from/to collection constraints like some other graph databases
        // The constraints in EdgeTypeDefinition are mainly for application-level validation
        Ok(())
    }

    pub fn list_edge_types(&self) -> Result<Vec<EdgeTypeDefinition>, GraphError> {
        // In ArangoDB, we return edge collections as edge types
        // Since ArangoDB doesn't enforce from/to constraints at the DB level,
        // we return edge collections with empty from/to collections
        let collections = self.list_collections()?;
        let edge_types = collections
            .into_iter()
            .filter(|c| matches!(c.container_type, ContainerType::EdgeContainer))
            .map(|c| EdgeTypeDefinition {
                collection: c.name,
                from_collections: vec![], // ArangoDB doesn't store these constraints
                to_collections: vec![],   // ArangoDB doesn't store these constraints
            })
            .collect();
        Ok(edge_types)
    }

    pub fn get_transaction_status(&self, transaction_id: &str) -> Result<String, GraphError> {
        let endpoint = format!("/_api/transaction/{}", transaction_id);
        let request = self.get(&endpoint);

        let response: TransactionStatusResponse = self.execute(request)?;
        Ok(response.status)
    }

    pub fn get_database_statistics(&self) -> Result<DatabaseStatistics, GraphError> {
        let collections: ListCollectionsResponse = self
            .execute(self.get("/_api/collection?excludeSystem=true"))?;

        let mut total_vertex_count = 0;
        let mut total_edge_count = 0;

        for collection_info in collections.result {
            let properties_endpoint =
                format!("/_api/collection/{}/properties", collection_info.name);
            let properties: CollectionPropertiesResponse =
                self.execute(self.get(&properties_endpoint))?;

            if properties.collection_type == ArangoCollectionType::Edge {
                total_edge_count += properties.count;
            } else {
                total_vertex_count += properties.count;
            }
        }

        Ok(DatabaseStatistics {
            vertex_count: total_vertex_count,
            edge_count: total_edge_count,
        })
    }

    pub fn execute_query(&self, query: Value) -> Result<Value, GraphError> {
        let request = self.post("/_api/cursor");
        self.execute_json(request, &query)
    }

    pub fn ensure_collection_exists(&self, name: &str, container_type: ContainerType) -> Result<(), GraphError> {
        // Try to create collection, ignore error if it already exists
        match self.create_collection(name, container_type) {
            Ok(_) => Ok(()),
            Err(GraphError::InternalError(msg)) if msg.contains("duplicate name") => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[derive(serde::Deserialize, Debug)]
struct TransactionStatusResponse {
    #[serde(rename = "id")]
    _id: String,
    status: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct DatabaseStatistics {
    pub vertex_count: u64,
    pub edge_count: u64,
}

#[derive(serde::Deserialize, Debug)]
struct ListCollectionsResponse {
    result: Vec<CollectionInfoShort>,
}

#[derive(serde::Deserialize, Debug)]
struct CollectionInfoShort {
    name: String,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct CollectionPropertiesResponse {
    count: u64,
    #[serde(rename = "type")]
    collection_type: ArangoCollectionType,
}

#[derive(Debug, PartialEq)]
enum ArangoCollectionType {
    Document,
    Edge,
    Unknown(u8),
}

impl From<u8> for ArangoCollectionType {
    fn from(value: u8) -> Self {
        match value {
            2 => ArangoCollectionType::Document,
            3 => ArangoCollectionType::Edge,
            _ => ArangoCollectionType::Unknown(value),
        }
    }
}

impl<'de> serde::Deserialize<'de> for ArangoCollectionType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        Ok(ArangoCollectionType::from(value))
    }
}
