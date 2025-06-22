use crate::conversions::{self};
use crate::helpers::{parse_edge_from_row, parse_vertex_from_graph_data};
use crate::Transaction;
use golem_graph::golem::graph::{
    errors::GraphError,
    transactions::{EdgeSpec, GuestTransaction, VertexSpec},
    types::{Direction, Edge, ElementId, FilterCondition, PropertyMap, SortSpec, Vertex},
};
use golem_graph::query_utils::{build_sort_clause, build_where_clause, QuerySyntax};
use serde_json::{json, Map};

impl Transaction {
    pub(crate) fn commit(&self) -> Result<(), GraphError> {
        self.api.commit_transaction(&self.transaction_url)
    }
}

fn cypher_syntax() -> QuerySyntax {
    QuerySyntax {
        equal: "=",
        not_equal: "<>",
        less_than: "<",
        less_than_or_equal: "<=",
        greater_than: ">",
        greater_than_or_equal: ">=",
        contains: "CONTAINS",
        starts_with: "STARTS WITH",
        ends_with: "ENDS WITH",
        regex_match: "=~",
        param_prefix: "$",
    }
}

impl GuestTransaction for Transaction {
    fn commit(&self) -> Result<(), GraphError> {
        self.api.commit_transaction(&self.transaction_url)
    }

    fn rollback(&self) -> Result<(), GraphError> {
        self.api.rollback_transaction(&self.transaction_url)
    }

    fn create_vertex(
        &self,
        vertex_type: String,
        properties: PropertyMap,
    ) -> Result<Vertex, GraphError> {
        self.create_vertex_with_labels(vertex_type, vec![], properties)
    }

    fn create_vertex_with_labels(
        &self,
        vertex_type: String,
        additional_labels: Vec<String>,
        properties: PropertyMap,
    ) -> Result<Vertex, GraphError> {
        let mut labels = vec![vertex_type];
        labels.extend(additional_labels);
        let cypher_labels = labels.join(":");

        let properties_map = conversions::to_cypher_properties(properties)?;

        let statement = json!({
            "statement": format!("CREATE (n:`{}`) SET n = $props RETURN n", cypher_labels),
            "parameters": { "props": properties_map },
            "resultDataContents": ["row","graph"]
        });

        let statements = json!({ "statements": [statement] });
        let response = self
            .api
            .execute_in_transaction(&self.transaction_url, statements)?;

        let result = response["results"]
            .as_array()
            .and_then(|r| r.first())
            .ok_or_else(|| {
                GraphError::InternalError(
                    "Invalid response from Neo4j for create_vertex".to_string(),
                )
            })?;

        let data = result["data"]
            .as_array()
            .and_then(|d| d.first())
            .ok_or_else(|| {
                GraphError::InternalError("Missing data in create_vertex response".to_string())
            })?;

        let graph_node = data["graph"]["nodes"]
            .as_array()
            .and_then(|n| n.first())
            .ok_or_else(|| {
                GraphError::InternalError(
                    "Missing graph node in create_vertex response".to_string(),
                )
            })?;

        parse_vertex_from_graph_data(graph_node, None)
    }

    fn get_vertex(&self, id: ElementId) -> Result<Option<Vertex>, GraphError> {
        // Robust: If id is a string of the form 'prop:<property>:<value>', fetch by property
        if let ElementId::StringValue(s) = &id {
            if let Some((prop, value)) = s.strip_prefix("prop:").and_then(|rest| rest.split_once(":")) {
                let statement = json!({
                    "statement": format!("MATCH (n) WHERE n.`{}` = $value RETURN n", prop),
                    "parameters": { "value": value },
                    "resultDataContents": ["row","graph"]
                });
                let statements = json!({ "statements": [statement] });
                let response = self.api.execute_in_transaction(&self.transaction_url, statements)?;
                let result = response["results"].as_array().and_then(|r| r.first());
                if result.is_none() {
                    return Ok(None);
                }
                if let Some(errors) = result.unwrap()["errors"].as_array() {
                    if !errors.is_empty() {
                        return Ok(None);
                    }
                }
                let data = result.unwrap()["data"].as_array().and_then(|d| d.first());
                if data.is_none() {
                    return Ok(None);
                }
                let json_node = data
                    .as_ref()
                    .and_then(|d| d.get("graph"))
                    .and_then(|g| g.get("nodes"))
                    .and_then(|nodes| nodes.as_array())
                    .and_then(|arr| arr.first())
                    .or_else(|| {
                        data.as_ref()
                            .and_then(|d| d.get("row"))
                            .and_then(|r| r.as_array())
                            .and_then(|arr| arr.first())
                    });
                if let Some(json_node) = json_node {
                    let vertex = parse_vertex_from_graph_data(json_node, None)?;
                    return Ok(Some(vertex));
                } else {
                    return Ok(None);
                }
            }
        }
        // Legacy: fallback to elementId(n)
        let id_str = match id.clone() {
            ElementId::StringValue(s) => s,
            ElementId::Int64(i) => i.to_string(),
            ElementId::Uuid(u) => u,
        };
        let cypher_id_value = json!(id_str);
        let statement = json!({
            "statement": "MATCH (n) WHERE elementId(n) = $id RETURN n",
            "parameters": { "id": cypher_id_value },
            "resultDataContents": ["row","graph"]
        });
        let statements = json!({ "statements": [statement] });
        let response = self.api.execute_in_transaction(&self.transaction_url, statements)?;
        let result = response["results"].as_array().and_then(|r| r.first());
        if result.is_none() {
            return Ok(None);
        }
        if let Some(errors) = result.unwrap()["errors"].as_array() {
            if !errors.is_empty() {
                return Ok(None);
            }
        }
        let data = result.unwrap()["data"].as_array().and_then(|d| d.first());
        if data.is_none() {
            return Ok(None);
        }
        let json_node = data
            .as_ref()
            .and_then(|d| d.get("graph"))
            .and_then(|g| g.get("nodes"))
            .and_then(|nodes| nodes.as_array())
            .and_then(|arr| arr.first())
            .or_else(|| {
                data.as_ref()
                    .and_then(|d| d.get("row"))
                    .and_then(|r| r.as_array())
                    .and_then(|arr| arr.first())
            });
        if let Some(json_node) = json_node {
            let vertex = parse_vertex_from_graph_data(json_node, None)?;
            Ok(Some(vertex))
        } else {
            Ok(None)
        }
    }

    fn update_vertex(&self, id: ElementId, properties: PropertyMap) -> Result<Vertex, GraphError> {
        let cypher_id = match id.clone() {
            ElementId::StringValue(s) => s,
            ElementId::Int64(i) => i.to_string(),
            ElementId::Uuid(u) => u,
        };
        let properties_map = conversions::to_cypher_properties(properties)?;
        let statement = json!({
            "statement": "MATCH (n) WHERE elementId(n) = $id SET n = $props RETURN n",
            "parameters": { "id": cypher_id, "props": properties_map }
        });
        let statements = json!({ "statements": [statement] });
        let response = self
            .api
            .execute_in_transaction(&self.transaction_url, statements)?;
        let result = response["results"]
            .as_array()
            .and_then(|r| r.first())
            .ok_or_else(|| {
                GraphError::InternalError(
                    "Invalid response from Neo4j for update_vertex".to_string(),
                )
            })?;
        let data = result["data"]
            .as_array()
            .and_then(|d| d.first())
            .ok_or_else(|| GraphError::ElementNotFound(id.clone()))?;
        let graph_node = data["graph"]["nodes"]
            .as_array()
            .and_then(|n| n.first())
            .ok_or_else(|| GraphError::ElementNotFound(id.clone()))?;
        parse_vertex_from_graph_data(graph_node, Some(id))
    }

    fn update_vertex_properties(
        &self,
        id: ElementId,
        updates: PropertyMap,
    ) -> Result<Vertex, GraphError> {
        let cypher_id = match id.clone() {
            ElementId::StringValue(s) => s,
            ElementId::Int64(i) => i.to_string(),
            ElementId::Uuid(u) => u,
        };

        let properties_map = conversions::to_cypher_properties(updates)?;

        let statement = json!({
            "statement": "MATCH (n) WHERE elementId(n) = $id SET n += $props RETURN n",
            "parameters": {
                "id": cypher_id,
                "props": properties_map,
            }
        });

        let statements = json!({ "statements": [statement] });
        let response = self
            .api
            .execute_in_transaction(&self.transaction_url, statements)?;

        let result = response["results"]
            .as_array()
            .and_then(|r| r.first())
            .ok_or_else(|| {
                GraphError::InternalError(
                    "Invalid response from Neo4j for update_vertex_properties".to_string(),
                )
            })?;

        if let Some(errors) = result["errors"].as_array() {
            if !errors.is_empty() {
                return Err(GraphError::InternalError(format!(
                    "Neo4j error on update_vertex_properties: {}",
                    errors[0]
                )));
            }
        }

        let data = result["data"]
            .as_array()
            .and_then(|d| d.first())
            .ok_or_else(|| GraphError::ElementNotFound(id.clone()))?;

        let graph_node = data["graph"]["nodes"]
            .as_array()
            .and_then(|n| n.first())
            .ok_or_else(|| GraphError::ElementNotFound(id.clone()))?;

        parse_vertex_from_graph_data(graph_node, Some(id))
    }

    fn delete_vertex(&self, id: ElementId, delete_edges: bool) -> Result<(), GraphError> {
        let cypher_id = match id {
            ElementId::StringValue(s) => s,
            ElementId::Int64(i) => i.to_string(),
            ElementId::Uuid(u) => u,
        };

        let detach_str = if delete_edges { "DETACH" } else { "" };
        let statement = json!({
            "statement": format!("MATCH (n) WHERE elementId(n) = $id {} DELETE n", detach_str),
            "parameters": { "id": cypher_id }
        });

        let statements = json!({ "statements": [statement] });
        self.api
            .execute_in_transaction(&self.transaction_url, statements)?;
        Ok(())
    }

    fn find_vertices(
        &self,
        vertex_type: Option<String>,
        filters: Option<Vec<FilterCondition>>,
        sort: Option<Vec<SortSpec>>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<Vertex>, GraphError> {
        let mut params = Map::new();
        let syntax = cypher_syntax();

        let match_clause = match &vertex_type {
            Some(vt) => format!("MATCH (n:`{}`)", vt),
            None => "MATCH (n)".to_string(),
        };

        let where_clause = build_where_clause(&filters, "n", &mut params, &syntax, |v| {
            conversions::to_json_value(v)
        })?;
        let sort_clause = build_sort_clause(&sort, "n");

        let limit_clause = limit.map_or("".to_string(), |l| format!("LIMIT {}", l));
        let offset_clause = offset.map_or("".to_string(), |o| format!("SKIP {}", o));

        let full_query = format!(
            "{} {} RETURN n {} {} {}",
            match_clause, where_clause, sort_clause, offset_clause, limit_clause
        );

        let statement = json!({
            "statement": full_query,
            "parameters": params
        });

        let statements = json!({ "statements": [statement] });
        let response = self
            .api
            .execute_in_transaction(&self.transaction_url, statements)?;

        let result = response["results"]
            .as_array()
            .and_then(|r| r.first())
            .ok_or_else(|| {
                GraphError::InternalError(
                    "Invalid response from Neo4j for find_vertices".to_string(),
                )
            })?;

        if let Some(errors) = result["errors"].as_array() {
            if !errors.is_empty() {
                return Err(GraphError::InternalError(format!(
                    "Neo4j error on find_vertices: {}",
                    errors[0]
                )));
            }
        }

        let empty_vec = vec![];
        let data = result["data"].as_array().unwrap_or(&empty_vec);
        let mut vertices = Vec::new();

        for item in data {
            if let Some(graph_node) = item["graph"]["nodes"].as_array().and_then(|n| n.first()) {
                let vertex = parse_vertex_from_graph_data(graph_node, None)?;
                vertices.push(vertex);
            }
        }

        Ok(vertices)
    }

    fn create_edge(
        &self,
        edge_type: String,
        from_vertex: ElementId,
        to_vertex: ElementId,
        properties: PropertyMap,
    ) -> Result<Edge, GraphError> {
        // 1) Turn ElementId::StringValue("67") or Int64(67) into an i64 for id(...)
        let from_id_int = match from_vertex.clone() {
            ElementId::Int64(i)       => i,
            ElementId::StringValue(s) => s.parse::<i64>()
                                           .map_err(|_| GraphError::InvalidQuery("Expected numeric id".into()))?,
            ElementId::Uuid(_)        => {
                return Err(GraphError::InvalidQuery(
                    "Cannot use UUID for numeric id match".into(),
                ))
            }
        };
        let to_id_int = match to_vertex.clone() {
            ElementId::Int64(i)       => i,
            ElementId::StringValue(s) => s.parse::<i64>()
                                           .map_err(|_| GraphError::InvalidQuery("Expected numeric id".into()))?,
            ElementId::Uuid(_)        => {
                return Err(GraphError::InvalidQuery(
                    "Cannot use UUID for numeric id match".into(),
                ))
            }
        };
    
        // 2) Convert properties
        let props = conversions::to_cypher_properties(properties.clone())?;
    
        // 3) MATCH by id(), CREATE, then RETURN toString(id(...)) so we get "67", not "4:...:67"
        let stmt = json!({
            "statement": format!(
                "MATCH (a) WHERE id(a) = $from_id \
                 MATCH (b) WHERE id(b) = $to_id \
                 CREATE (a)-[r:`{}`]->(b) SET r = $props \
                 RETURN toString(id(r)), type(r), properties(r), \
                        toString(id(startNode(r))), toString(id(endNode(r)))",
                edge_type
            ),
            "parameters": {
                "from_id": from_id_int,
                "to_id":   to_id_int,
                "props":   props
            }
        });
        let response = self
            .api
            .execute_in_transaction(&self.transaction_url, json!({ "statements": [stmt] }))?;
    
        // 4) Pull out the first row and hand off to your existing parser
        let results = response["results"].as_array()
            .and_then(|a| a.first())
            .ok_or_else(|| GraphError::InternalError("Invalid response from Neo4j for create_edge".into()))?;
        let data = results["data"].as_array()
            .and_then(|d| d.first())
            .ok_or_else(|| GraphError::InternalError("Invalid response from Neo4j for create_edge".into()))?;
        let row = data["row"].as_array()
            .ok_or_else(|| GraphError::InternalError("Missing row data for create_edge".into()))?;
    
        parse_edge_from_row(row)
    }
    

    fn get_edge(&self, id: ElementId) -> Result<Option<Edge>, GraphError> {
        // 1) Parse numeric id
        let id_num = match id.clone() {
            ElementId::Int64(i) => i,
            ElementId::StringValue(s) => s
                .parse::<i64>()
                .map_err(|_| GraphError::InvalidQuery("Invalid edge ID".into()))?,
            ElementId::Uuid(_) => {
                return Err(GraphError::InvalidQuery(
                    "Cannot use UUID for numeric id match".into(),
                ))
            }
        };
    
        // 2) MATCH on id(r) but RETURN toString(id(...)) so parse_edge_from_row yields StringValue
        let statement = json!({
            "statement": "\
                MATCH ()-[r]-() \
                WHERE id(r) = $id \
                RETURN \
                  toString(id(r)), \
                  type(r), \
                  properties(r), \
                  toString(id(startNode(r))), \
                  toString(id(endNode(r)))",
            "parameters": { "id": id_num }
        });
        let resp = self
            .api
            .execute_in_transaction(&self.transaction_url, json!({ "statements": [statement] }))?;
    
        // 3) Safely unwrap into slices
        let results = match resp["results"].as_array() {
            Some(arr) => arr.as_slice(),
            None => return Ok(None),
        };
        if results.is_empty() {
            return Ok(None);
        }
    
        let data = match results[0]["data"].as_array() {
            Some(arr) => arr.as_slice(),
            None => return Ok(None),
        };
        if data.is_empty() {
            return Ok(None);
        }
    
        // 4) Extract the row array
        let row = data[0]["row"]
            .as_array()
            .ok_or_else(|| GraphError::InternalError("Missing row in get_edge".into()))?;
    
        // 5) Delegate to your parser (which will see strings like "0", "71", "72")
        let edge = parse_edge_from_row(row)?;
        Ok(Some(edge))
    }
    
    

    fn update_edge(&self, id: ElementId, properties: PropertyMap) -> Result<Edge, GraphError> {
        let cypher_id = match id.clone() {
            ElementId::StringValue(s) => s,
            ElementId::Int64(i) => i.to_string(),
            ElementId::Uuid(u) => u,
        };

        let properties_map = conversions::to_cypher_properties(properties)?;

        let statement = json!({
            "statement": "MATCH ()-[r]-() WHERE elementId(r) = $id SET r = $props RETURN elementId(r), type(r), properties(r), elementId(startNode(r)), elementId(endNode(r))",
            "parameters": {
                "id": cypher_id,
                "props": properties_map,
            }
        });

        let statements = json!({ "statements": [statement] });
        let response = self
            .api
            .execute_in_transaction(&self.transaction_url, statements)?;

        let result = response["results"]
            .as_array()
            .and_then(|r| r.first())
            .ok_or_else(|| {
                GraphError::InternalError("Invalid response from Neo4j for update_edge".to_string())
            })?;

        if let Some(errors) = result["errors"].as_array() {
            if !errors.is_empty() {
                return Err(GraphError::InternalError(format!(
                    "Neo4j error on update_edge: {}",
                    errors[0]
                )));
            }
        }

        let data = result["data"]
            .as_array()
            .and_then(|d| d.first())
            .ok_or_else(|| GraphError::ElementNotFound(id.clone()))?;

        let row = data["row"].as_array().ok_or_else(|| {
            GraphError::InternalError("Missing row data for update_edge".to_string())
        })?;

        parse_edge_from_row(row)
    }

    fn update_edge_properties(
        &self,
        id: ElementId,
        updates: PropertyMap,
    ) -> Result<Edge, GraphError> {
        let cypher_id = match id.clone() {
            ElementId::StringValue(s) => s,
            ElementId::Int64(i) => i.to_string(),
            ElementId::Uuid(u) => u,
        };

        let properties_map = conversions::to_cypher_properties(updates)?;

        let statement = json!({
            "statement": "MATCH ()-[r]-() WHERE elementId(r) = $id SET r += $props RETURN elementId(r), type(r), properties(r), elementId(startNode(r)), elementId(endNode(r))",
            "parameters": {
                "id": cypher_id,
                "props": properties_map,
            }
        });

        let statements = json!({ "statements": [statement] });
        let response = self
            .api
            .execute_in_transaction(&self.transaction_url, statements)?;

        let result = response["results"]
            .as_array()
            .and_then(|r| r.first())
            .ok_or_else(|| {
                GraphError::InternalError(
                    "Invalid response from Neo4j for update_edge_properties".to_string(),
                )
            })?;

        if let Some(errors) = result["errors"].as_array() {
            if !errors.is_empty() {
                return Err(GraphError::InternalError(format!(
                    "Neo4j error on update_edge_properties: {}",
                    errors[0]
                )));
            }
        }

        let data = result["data"]
            .as_array()
            .and_then(|d| d.first())
            .ok_or_else(|| GraphError::ElementNotFound(id.clone()))?;

        let row = data["row"].as_array().ok_or_else(|| {
            GraphError::InternalError("Missing row data for update_edge_properties".to_string())
        })?;

        parse_edge_from_row(row)
    }

    fn delete_edge(&self, id: ElementId) -> Result<(), GraphError> {
        // 1) Turn ElementId::StringValue("1") or Int64(1) into an i64 for MATCH id(r)
        let id_num = match id {
            ElementId::Int64(i)       => i,
            ElementId::StringValue(s) => s.parse::<i64>()
                                           .map_err(|_| GraphError::InvalidQuery("Invalid edge ID".into()))?,
            ElementId::Uuid(_)        => {
                return Err(GraphError::InvalidQuery(
                    "Cannot use UUID for numeric id match".into(),
                ))
            }
        };
    
        // 2) MATCH on id(r) = $id_num and DELETE the relationship
        let stmt = json!({
            "statement": "MATCH ()-[r]-() WHERE id(r) = $id DELETE r",
            "parameters": { "id": id_num }
        });
        let batch = json!({ "statements": [stmt] });
        self.api.execute_in_transaction(&self.transaction_url, batch)?;
        Ok(())
    }
    

    fn find_edges(
        &self,
        edge_types: Option<Vec<String>>,
        filters: Option<Vec<FilterCondition>>,
        sort: Option<Vec<SortSpec>>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<Edge>, GraphError> {
        let mut params = Map::new();
        let syntax = cypher_syntax();

        let edge_type_str = edge_types.map_or("".to_string(), |types| {
            if types.is_empty() {
                "".to_string()
            } else {
                format!(":{}", types.join("|"))
            }
        });

        let match_clause = format!("MATCH ()-[r{}]-()", &edge_type_str);

        let where_clause = build_where_clause(&filters, "r", &mut params, &syntax, |v| {
            conversions::to_json_value(v)
        })?;
        let sort_clause = build_sort_clause(&sort, "r");

        let limit_clause = limit.map_or("".to_string(), |l| format!("LIMIT {}", l));
        let offset_clause = offset.map_or("".to_string(), |o| format!("SKIP {}", o));

        let full_query = format!(
            "{} {} RETURN elementId(r), type(r), properties(r), elementId(startNode(r)), elementId(endNode(r)) {} {} {}",
            match_clause, where_clause, sort_clause, offset_clause, limit_clause
        );

        let statement = json!({
            "statement": full_query,
            "parameters": params
        });

        let statements = json!({ "statements": [statement] });
        let response = self
            .api
            .execute_in_transaction(&self.transaction_url, statements)?;

        let result = response["results"]
            .as_array()
            .and_then(|r| r.first())
            .ok_or_else(|| {
                GraphError::InternalError("Invalid response from Neo4j for find_edges".to_string())
            })?;

        if let Some(errors) = result["errors"].as_array() {
            if !errors.is_empty() {
                return Err(GraphError::InternalError(format!(
                    "Neo4j error on find_edges: {}",
                    errors[0]
                )));
            }
        }

        let empty_vec = vec![];
        let data = result["data"].as_array().unwrap_or(&empty_vec);
        let mut edges = Vec::new();

        for item in data {
            if let Some(row) = item["row"].as_array() {
                let edge = parse_edge_from_row(row)?;
                edges.push(edge);
            }
        }

        Ok(edges)
    }

    fn get_adjacent_vertices(
        &self,
        vertex_id: ElementId,
        direction: Direction,
        edge_types: Option<Vec<String>>,
        limit: Option<u32>,
    ) -> Result<Vec<Vertex>, GraphError> {
        let cypher_id = match vertex_id {
            ElementId::StringValue(s) => s,
            ElementId::Int64(i) => i.to_string(),
            ElementId::Uuid(u) => u,
        };

        let (left_arrow, right_arrow) = match direction {
            Direction::Outgoing => ("", "->"),
            Direction::Incoming => ("<-", ""),
            Direction::Both => ("-", "-"),
        };

        let edge_type_str = edge_types.map_or("".to_string(), |types| {
            if types.is_empty() {
                "".to_string()
            } else {
                format!(":{}", types.join("|"))
            }
        });

        let limit_clause = limit.map_or("".to_string(), |l| format!("LIMIT {}", l));

        let full_query = format!(
            "MATCH (a){}[r{}]{}(b) WHERE elementId(a) = $id RETURN b {}",
            left_arrow, edge_type_str, right_arrow, limit_clause
        );

        let statement = json!({
            "statement": full_query,
            "parameters": { "id": cypher_id }
        });

        let statements = json!({ "statements": [statement] });
        let response = self
            .api
            .execute_in_transaction(&self.transaction_url, statements)?;

        let result = response["results"]
            .as_array()
            .and_then(|r| r.first())
            .ok_or_else(|| {
                GraphError::InternalError(
                    "Invalid response from Neo4j for get_adjacent_vertices".to_string(),
                )
            })?;

        if let Some(errors) = result["errors"].as_array() {
            if !errors.is_empty() {
                return Err(GraphError::InternalError(format!(
                    "Neo4j error on get_adjacent_vertices: {}",
                    errors[0]
                )));
            }
        }

        let empty_vec = vec![];
        let data = result["data"].as_array().unwrap_or(&empty_vec);
        let mut vertices = Vec::new();

        for item in data {
            if let Some(graph_node) = item["graph"]["nodes"].as_array().and_then(|n| n.first()) {
                let vertex = parse_vertex_from_graph_data(graph_node, None)?;
                vertices.push(vertex);
            }
        }

        Ok(vertices)
    }

    fn get_connected_edges(
        &self,
        vertex_id: ElementId,
        direction: Direction,
        edge_types: Option<Vec<String>>,
        limit: Option<u32>,
    ) -> Result<Vec<Edge>, GraphError> {
        let cypher_id = match vertex_id {
            ElementId::StringValue(s) => s,
            ElementId::Int64(i) => i.to_string(),
            ElementId::Uuid(u) => u,
        };

        let (left_arrow, right_arrow) = match direction {
            Direction::Outgoing => ("", "->"),
            Direction::Incoming => ("<-", ""),
            Direction::Both => ("-", "-"),
        };

        let edge_type_str = edge_types.map_or("".to_string(), |types| {
            if types.is_empty() {
                "".to_string()
            } else {
                format!(":{}", types.join("|"))
            }
        });

        let limit_clause = limit.map_or("".to_string(), |l| format!("LIMIT {}", l));

        let full_query = format!(
            "MATCH (a){}[r{}]{}(b) WHERE elementId(a) = $id RETURN elementId(r), type(r), properties(r), elementId(startNode(r)), elementId(endNode(r)) {}",
            left_arrow, edge_type_str, right_arrow, limit_clause
        );

        let statement = json!({
            "statement": full_query,
            "parameters": { "id": cypher_id }
        });

        let statements = json!({ "statements": [statement] });
        let response = self
            .api
            .execute_in_transaction(&self.transaction_url, statements)?;

        let result = response["results"]
            .as_array()
            .and_then(|r| r.first())
            .ok_or_else(|| {
                GraphError::InternalError(
                    "Invalid response from Neo4j for get_connected_edges".to_string(),
                )
            })?;

        if let Some(errors) = result["errors"].as_array() {
            if !errors.is_empty() {
                return Err(GraphError::InternalError(format!(
                    "Neo4j error on get_connected_edges: {}",
                    errors[0]
                )));
            }
        }

        let empty_vec = vec![];
        let data = result["data"].as_array().unwrap_or(&empty_vec);
        let mut edges = Vec::new();

        for item in data {
            if let Some(row) = item["row"].as_array() {
                let edge = parse_edge_from_row(row)?;
                edges.push(edge);
            }
        }

        Ok(edges)
    }

    fn create_vertices(&self, vertices: Vec<VertexSpec>) -> Result<Vec<Vertex>, GraphError> {
        if vertices.is_empty() {
            return Ok(vec![]);
        }

        let mut statements = Vec::new();
        for spec in vertices {
            let mut labels = vec![spec.vertex_type];
            if let Some(additional) = spec.additional_labels {
                labels.extend(additional);
            }
            let cypher_labels = labels.join(":");
            let properties_map = conversions::to_cypher_properties(spec.properties)?;

            let statement = json!({
                "statement": format!("CREATE (n:`{}`) SET n = $props RETURN n", cypher_labels),
                "parameters": { "props": properties_map }
            });
            statements.push(statement);
        }

        let statements_payload = json!({ "statements": statements });
        let response = self
            .api
            .execute_in_transaction(&self.transaction_url, statements_payload)?;

        let results = response["results"].as_array().ok_or_else(|| {
            GraphError::InternalError("Invalid response from Neo4j for create_vertices".to_string())
        })?;

        let mut created_vertices = Vec::new();
        for result in results {
            if let Some(errors) = result["errors"].as_array() {
                if !errors.is_empty() {
                    return Err(GraphError::InternalError(format!(
                        "Neo4j error on create_vertices: {}",
                        errors[0]
                    )));
                }
            }

            let empty_vec = vec![];
            let data = result["data"].as_array().unwrap_or(&empty_vec);
            for item in data {
                if let Some(graph_node) = item["graph"]["nodes"].as_array().and_then(|n| n.first())
                {
                    let vertex = parse_vertex_from_graph_data(graph_node, None)?;
                    created_vertices.push(vertex);
                }
            }
        }

        Ok(created_vertices)
    }

    fn create_edges(&self, edges: Vec<EdgeSpec>) -> Result<Vec<Edge>, GraphError> {
        if edges.is_empty() {
            return Ok(vec![]);
        }

        let mut statements = Vec::new();
        for spec in edges {
            let properties_map = conversions::to_cypher_properties(spec.properties)?;
            let from_id = match spec.from_vertex {
                ElementId::StringValue(s) => s,
                ElementId::Int64(i) => i.to_string(),
                ElementId::Uuid(u) => u,
            };
            let to_id = match spec.to_vertex {
                ElementId::StringValue(s) => s,
                ElementId::Int64(i) => i.to_string(),
                ElementId::Uuid(u) => u,
            };

            let statement = json!({
                "statement": format!("MATCH (a), (b) WHERE elementId(a) = $from_id AND elementId(b) = $to_id CREATE (a)-[r:`{}`]->(b) SET r = $props RETURN elementId(r), type(r), properties(r), elementId(a), elementId(b)", spec.edge_type),
                "parameters": {
                    "from_id": from_id,
                    "to_id": to_id,
                    "props": properties_map
                }
            });
            statements.push(statement);
        }

        let statements_payload = json!({ "statements": statements });
        let response = self
            .api
            .execute_in_transaction(&self.transaction_url, statements_payload)?;

        let results = response["results"].as_array().ok_or_else(|| {
            GraphError::InternalError("Invalid response from Neo4j for create_edges".to_string())
        })?;

        let mut created_edges = Vec::new();
        for result in results {
            if let Some(errors) = result["errors"].as_array() {
                if !errors.is_empty() {
                    return Err(GraphError::InternalError(format!(
                        "Neo4j error on create_edges: {}",
                        errors[0]
                    )));
                }
            }

            let empty_vec = vec![];
            let data = result["data"].as_array().unwrap_or(&empty_vec);
            for item in data {
                if let Some(row) = item["row"].as_array() {
                    let edge = parse_edge_from_row(row)?;
                    created_edges.push(edge);
                }
            }
        }

        Ok(created_edges)
    }

    fn upsert_vertex(
        &self,
        id: Option<ElementId>,
        vertex_type: String,
        properties: PropertyMap,
    ) -> Result<Vertex, GraphError> {
        if id.is_some() {
            return Err(GraphError::UnsupportedOperation(
                "upsert_vertex with a specific element ID is not yet supported. \
                Please provide matching properties and a null ID."
                    .to_string(),
            ));
        }
        if properties.is_empty() {
            return Err(GraphError::InvalidQuery(
                "upsert_vertex requires at least one property to match on for the MERGE operation."
                    .to_string(),
            ));
        }

        let set_props = conversions::to_cypher_properties(properties)?;

        let mut match_props = Map::new();
        let merge_prop_clauses: Vec<String> = set_props
            .keys()
            .map(|k| {
                let param_name = format!("match_{}", k);
                match_props.insert(param_name.clone(), set_props[k].clone());
                format!("{}: ${}", k, param_name)
            })
            .collect();
        let merge_clause = format!("{{ {} }}", merge_prop_clauses.join(", "));

        let mut params = match_props;
        params.insert("set_props".to_string(), json!(set_props));

        let statement = json!({
            "statement": format!(
                "MERGE (n:`{}` {}) SET n = $set_props RETURN n",
                vertex_type, merge_clause
            ),
            "parameters": params,
        });

        let statements = json!({ "statements": [statement] });
        let response = self
            .api
            .execute_in_transaction(&self.transaction_url, statements)?;

        let result = response["results"]
            .as_array()
            .and_then(|r| r.first())
            .ok_or_else(|| {
                GraphError::InternalError(
                    "Invalid response from Neo4j for upsert_vertex".to_string(),
                )
            })?;

        if let Some(errors) = result["errors"].as_array() {
            if !errors.is_empty() {
                return Err(GraphError::InvalidQuery(errors[0].to_string()));
            }
        }

        let data = result["data"]
            .as_array()
            .and_then(|d| d.first())
            .ok_or_else(|| {
                GraphError::InternalError("Missing data in upsert_vertex response".to_string())
            })?;

        let graph_node = data["graph"]["nodes"]
            .as_array()
            .and_then(|n| n.first())
            .ok_or_else(|| {
                GraphError::InternalError(
                    "Missing graph node in upsert_vertex response".to_string(),
                )
            })?;

        parse_vertex_from_graph_data(graph_node, None)
    }

    fn upsert_edge(
        &self,
        id: Option<ElementId>,
        edge_type: String,
        from_vertex: ElementId,
        to_vertex: ElementId,
        properties: PropertyMap,
    ) -> Result<Edge, GraphError> {
        if id.is_some() {
            return Err(GraphError::UnsupportedOperation(
                "upsert_edge with a specific element ID is not yet supported. \
                Please provide matching properties and a null ID."
                    .to_string(),
            ));
        }

        let from_id = match from_vertex {
            ElementId::StringValue(s) => s,
            ElementId::Int64(i) => i.to_string(),
            ElementId::Uuid(u) => u,
        };
        let to_id = match to_vertex {
            ElementId::StringValue(s) => s,
            ElementId::Int64(i) => i.to_string(),
            ElementId::Uuid(u) => u,
        };

        let set_props = conversions::to_cypher_properties(properties)?;
        let mut match_props = Map::new();
        let merge_prop_clauses: Vec<String> = set_props
            .keys()
            .map(|k| {
                let param_name = format!("match_{}", k);
                match_props.insert(param_name.clone(), set_props[k].clone());
                format!("{}: ${}", k, param_name)
            })
            .collect();

        let merge_clause = if merge_prop_clauses.is_empty() {
            "".to_string()
        } else {
            format!("{{ {} }}", merge_prop_clauses.join(", "))
        };

        let mut params = match_props;
        params.insert("from_id".to_string(), json!(from_id));
        params.insert("to_id".to_string(), json!(to_id));
        params.insert("set_props".to_string(), json!(set_props));

        let statement = json!({
            "statement": format!(
                "MATCH (a), (b) WHERE elementId(a) = $from_id AND elementId(b) = $to_id \
                MERGE (a)-[r:`{}` {}]->(b) \
                SET r = $set_props \
                RETURN elementId(r), type(r), properties(r), elementId(startNode(r)), elementId(endNode(r))",
                edge_type, merge_clause
            ),
            "parameters": params,
        });

        let statements = json!({ "statements": [statement] });
        let response = self
            .api
            .execute_in_transaction(&self.transaction_url, statements)?;

        let result = response["results"]
            .as_array()
            .and_then(|r| r.first())
            .ok_or_else(|| {
                GraphError::InternalError("Invalid response from Neo4j for upsert_edge".to_string())
            })?;

        if let Some(errors) = result["errors"].as_array() {
            if !errors.is_empty() {
                return Err(GraphError::InvalidQuery(errors[0].to_string()));
            }
        }

        let data = result["data"]
            .as_array()
            .and_then(|d| d.first())
            .ok_or_else(|| {
                GraphError::InternalError("Missing data in upsert_edge response".to_string())
            })?;

        let row = data["row"].as_array().ok_or_else(|| {
            GraphError::InternalError("Missing row data for upsert_edge".to_string())
        })?;

        parse_edge_from_row(row)
    }

    fn is_active(&self) -> bool {
        true
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::client::Neo4jApi;
//     use golem_graph::golem::graph::types::PropertyValue;
//     use std::env;

//     fn get_neo4j_host() -> String {
//         env::var("NEO4J_HOST").unwrap_or_else(|_| "localhost".to_string())
//     }

//     fn get_neo4j_port() -> u16 {
//         env::var("NEO4J_PORT")
//             .unwrap_or_else(|_| "7474".to_string())
//             .parse()
//             .unwrap()
//     }

//     fn get_neo4j_user() -> String {
//         env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".to_string())
//     }

//     fn get_neo4j_password() -> String {
//         env::var("NEO4J_PASSWORD").unwrap_or_else(|_| "password".to_string())
//     }

//     fn create_test_transaction() -> Result<Transaction, GraphError> {
//         let host = get_neo4j_host();
//         let port = get_neo4j_port();
//         let user = get_neo4j_user();
//         let password = get_neo4j_password();
    
//         let api = Neo4jApi::new(&host, port, "neo4j", &user, &password);
//         let transaction_url = api.begin_transaction()?;
//         Ok(Transaction {
//             api: std::sync::Arc::new(api),
//             transaction_url,
//         })
//     }
    

//     #[test]
//     fn test_create_and_get_vertex() {
//         let tx = match create_test_transaction() {
//             Ok(t) => t,
//             Err(e) => {
//                 panic!("Failed to create test transaction: {:?}", e);
//             }
//         };

//         let vertex_type = "TestVertex".to_string();
//         let name_value = "test_vertex_1".to_string();
//         let properties = vec![
//             ("name".to_string(), PropertyValue::StringValue(name_value.clone())),
//         ];

//         let created_vertex = tx
//             .create_vertex(vertex_type.clone(), properties.clone())
//             .expect("Failed to create vertex");
//         assert_eq!(created_vertex.vertex_type, vertex_type);

//         // Use property-based lookup for robustness
//         let property_id = ElementId::StringValue(format!("prop:name:{}", name_value));
//         let retrieved_vertex = tx
//             .get_vertex(property_id)
//             .expect("get_vertex failed")
//             .expect("Vertex not found");
//         assert_eq!(retrieved_vertex.vertex_type, vertex_type);
//         let retrieved_name = retrieved_vertex
//             .properties
//             .iter()
//             .find(|(k, _)| k == "name")
//             .expect("Missing 'name' property")
//             .1
//             .clone();
//         assert_eq!(retrieved_name, properties[0].1);

//         tx.delete_vertex(created_vertex.id, true)
//             .expect("delete_vertex failed");
//         tx.commit().expect("commit failed");
//     }

//     #[test]
//     fn test_create_and_delete_edge() {
//         use std::time::{SystemTime, UNIX_EPOCH};
//         let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
//         let from_name = format!("from_vertex_{}", now);
//         let to_name = format!("to_vertex_{}", now);

//         let tx = create_test_transaction()
//             .expect("Failed to create test transaction for create/delete edge");
//         let _from_vertex = tx.create_vertex("TestVertex".to_string(), vec![
//             ("name".to_string(), PropertyValue::StringValue(from_name.clone())),
//         ]).unwrap();
//         let _to_vertex = tx.create_vertex("TestVertex".to_string(), vec![
//             ("name".to_string(), PropertyValue::StringValue(to_name.clone())),
//         ]).unwrap();
//         tx.commit().unwrap();

//         let tx = create_test_transaction().expect("Failed to create test transaction for edge creation");
//         let edge_type = "TEST_EDGE".to_string();
//         let properties = vec![("weight".to_string(), PropertyValue::Float32(0.5))];

//         // Use property-based lookup for from/to vertices, ensure exactly one match
//         let from_id = ElementId::StringValue(format!("prop:name:{}", from_name));
//         let to_id = ElementId::StringValue(format!("prop:name:{}", to_name));
//         let from_vertex = tx.get_vertex(from_id).unwrap().expect("from_vertex not found");
//         let to_vertex = tx.get_vertex(to_id).unwrap().expect("to_vertex not found");
//         println!("from_vertex.id: {:?}", from_vertex.id);
//         println!("to_vertex.id: {:?}", to_vertex.id);

//         let created_edge = tx
//             .create_edge(
//                 edge_type.clone(),
//                 from_vertex.id.clone(),
//                 to_vertex.id.clone(),
//                 properties.clone(),
//             )
//             .unwrap();
//         assert_eq!(created_edge.edge_type, edge_type);
//         assert_eq!(created_edge.from_vertex, from_vertex.id);
//         assert_eq!(created_edge.to_vertex, to_vertex.id);

//         let retrieved_edge = tx.get_edge(created_edge.id.clone()).unwrap().unwrap();
//         assert_eq!(retrieved_edge.id, created_edge.id);

//         let edge_id = created_edge.id.clone();
//         tx.delete_edge(edge_id.clone()).unwrap();
//         let deleted_edge = tx.get_edge(edge_id).unwrap();
//         assert!(deleted_edge.is_none());

//         tx.delete_vertex(from_vertex.id, true).unwrap();
//         tx.delete_vertex(to_vertex.id, true).unwrap();
//         tx.commit().unwrap();
//     }

//     #[test]
//     fn test_transaction_commit() {
//         let vertex_type = "CommitTest".to_string();
//         let key_value = "value_1".to_string();
//         let properties = vec![
//             ("key".to_string(), PropertyValue::StringValue(key_value.clone())),
//         ];

//         let tx1 = create_test_transaction()
//             .expect("Failed to transmit test transaction ");
//         let created_vertex = tx1.create_vertex(vertex_type.clone(), properties.clone()).unwrap();
//         tx1.commit().unwrap();

//         let tx2 = create_test_transaction()
//             .expect("Failed to transmit test transaction 2");
//         // Use property-based lookup for robustness
//         let property_id = ElementId::StringValue(format!("prop:key:{}", key_value));
//         let retrieved_vertex = tx2.get_vertex(property_id).unwrap();
//         assert!(retrieved_vertex.is_some());

//         tx2.delete_vertex(created_vertex.id, true).unwrap();
//         tx2.commit().unwrap();
//     }

//     #[test]
//     fn test_transaction_rollback() {
//         // if env::var("NEO4J_HOST").is_err() {
//         //     println!("Skipping test_transaction_rollback: NEO4J_HOST not set");
//         //     return;
//         // }

//         let vertex_type = "RollbackTest".to_string();
//         let properties = vec![(
//             "key".to_string(),
//             PropertyValue::StringValue("value".to_string()),
//         )];

//         let tx1 = create_test_transaction()
//         .expect("Failed to transaction rollback test transaction ");
//         let created_vertex = tx1.create_vertex(vertex_type.clone(), properties).unwrap();
//         tx1.rollback().unwrap();

//         let tx2 = create_test_transaction()
//         .expect("Failed to transaction rollback test transaction ");
//         let retrieved_vertex = tx2.get_vertex(created_vertex.id.clone()).unwrap();
//         assert!(retrieved_vertex.is_none());

//         tx2.commit().unwrap();
//     }
// }
