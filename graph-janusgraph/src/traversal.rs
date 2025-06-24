use crate::{
    helpers::{element_id_to_key, parse_path_from_gremlin, parse_vertex_from_gremlin},
    GraphJanusGraphComponent, Transaction,
};
use golem_graph::golem::graph::{
    errors::GraphError,
    traversal::{
        Direction, Guest as TraversalGuest, NeighborhoodOptions, Path, PathOptions, Subgraph,
    },
    types::{ElementId, Vertex},
};
use serde_json::{json, Value};

/// Convert our ElementId into a JSON binding for Gremlin
fn id_to_json(id: ElementId) -> Value {
    match id {
        ElementId::StringValue(s) => json!(s),
        ElementId::Int64(i)        => json!(i),
        ElementId::Uuid(u)         => json!(u.to_string()),
    }
}

/// Build the "edge‐and‐spill‐into‐vertex" step for Gremlin:
///  - Outgoing:  `outE().otherV()`
///  - Incoming:  `inE().otherV()`
///  - Both:      `bothE().otherV()`
/// And, if you passed a list of edge labels, it will bind them:
///   outE(edge_labels_0).otherV()
// fn build_edge_step(
//     dir: &Direction,
//     edge_types: &Option<Vec<String>>,
//     bindings: &mut serde_json::Map<String, Value>,
// ) -> String {
//     let base = match dir {
//         Direction::Outgoing => "outE",
//         Direction::Incoming => "inE",
//         Direction::Both     => "bothE",
//     };
//     if let Some(labels) = edge_types {
//         if !labels.is_empty() {
//             let key = format!("edge_labels_{}", bindings.len());
//             bindings.insert(key.clone(), json!(labels));
//             return format!("{}({}).otherV()", base, key);
//         }
//     }
//     format!("{}().otherV()", base)
// }


fn build_traversal_step(
    dir: &Direction,
    edge_types: &Option<Vec<String>>,
    bindings: &mut serde_json::Map<String, Value>,
) -> String {
    let base = match dir {
        Direction::Outgoing => "outE",
        Direction::Incoming => "inE",
        Direction::Both     => "bothE",
    };
    if let Some(labels) = edge_types {
        if !labels.is_empty() {
            let key = format!("edge_labels_{}", bindings.len());
            bindings.insert(key.clone(), json!(labels));
            return format!("{}({}).otherV()", base, key);
        }
    }
    format!("{}().otherV()", base)
}


impl Transaction {
    pub fn find_shortest_path(
        &self,
        from_vertex: ElementId,
        to_vertex: ElementId,
        _options: Option<PathOptions>,
    ) -> Result<Option<Path>, GraphError> {
        let mut bindings = serde_json::Map::new();
        bindings.insert("from_id".to_string(), id_to_json(from_vertex));
        bindings.insert("to_id".to_string(), id_to_json(to_vertex));
    
        // Use outE().inV() to include both vertices and edges in the path traversal
        let gremlin = "g.V(from_id).repeat(outE().inV().simplePath()).until(hasId(to_id)).path().limit(1)";
    
        println!("[DEBUG][find_shortest_path] Executing query: {}", gremlin);
        println!("[DEBUG][find_shortest_path] Bindings: {:?}", bindings);
        
        let resp = self.api.execute(gremlin, Some(Value::Object(bindings)))?;
        println!("[DEBUG][find_shortest_path] Raw response: {}", serde_json::to_string_pretty(&resp).unwrap_or_else(|_| format!("{:?}", resp)));
        
        // Handle GraphSON g:List format
        let data_array = if let Some(data) = resp["result"]["data"].as_object() {
            if data.get("@type") == Some(&Value::String("g:List".to_string())) {
                data.get("@value").and_then(|v| v.as_array())
            } else {
                None
            }
        } else {
            resp["result"]["data"].as_array()
        };
        
        if let Some(arr) = data_array {
            println!("[DEBUG][find_shortest_path] Data array length: {}", arr.len());
            if let Some(val) = arr.first() {
                println!("[DEBUG][find_shortest_path] First value: {}", serde_json::to_string_pretty(val).unwrap_or_else(|_| format!("{:?}", val)));
                return Ok(Some(parse_path_from_gremlin(val)?));
            } else {
                println!("[DEBUG][find_shortest_path] Data array is empty");
            }
        } else {
            println!("[DEBUG][find_shortest_path] No data array in response");
        }
    
        Ok(None)
    }
    
    
    

    pub fn find_all_paths(
        &self,
        from_vertex: ElementId,
        to_vertex: ElementId,
        options: Option<PathOptions>,
        limit: Option<u32>,
    ) -> Result<Vec<Path>, GraphError> {
        // ←— Unsuppported‑options guard
        if let Some(opts) = &options {
            if opts.vertex_types.is_some()
                || opts.vertex_filters.is_some()
                || opts.edge_filters.is_some()
            {
                return Err(GraphError::UnsupportedOperation(
                    "vertex_types, vertex_filters, and edge_filters are not yet supported in find_all_paths"
                        .to_string(),
                ));
            }
        }

        let mut bindings = serde_json::Map::new();
        let edge_types = options.and_then(|o| o.edge_types);
        let step = build_traversal_step(&Direction::Both, &edge_types, &mut bindings);
        bindings.insert("from_id".to_string(), id_to_json(from_vertex));
        bindings.insert("to_id".to_string(), id_to_json(to_vertex));

        let mut gremlin = format!(
            "g.V(from_id).repeat({}.simplePath()).until(hasId(to_id)).path()",
            step
        );
        if let Some(lim) = limit {
            gremlin.push_str(&format!(".limit({})", lim));
        }

        println!("[DEBUG][find_all_paths] Executing query: {}", gremlin);
        println!("[DEBUG][find_all_paths] Bindings: {:?}", bindings);
        
        let response = self.api.execute(&gremlin, Some(Value::Object(bindings)))?;
        println!("[DEBUG][find_all_paths] Raw response: {}", serde_json::to_string_pretty(&response).unwrap_or_else(|_| format!("{:?}", response)));
        
        // Handle GraphSON g:List format (same as find_shortest_path)
        let data_array = if let Some(data) = response["result"]["data"].as_object() {
            if data.get("@type") == Some(&Value::String("g:List".to_string())) {
                data.get("@value").and_then(|v| v.as_array())
            } else {
                None
            }
        } else {
            response["result"]["data"].as_array()
        };
        
        if let Some(arr) = data_array {
            println!("[DEBUG][find_all_paths] Data array length: {}", arr.len());
            arr.iter().map(parse_path_from_gremlin).collect()
        } else {
            println!("[DEBUG][find_all_paths] No data array in response");
            Ok(Vec::new())
        }
    }

    pub fn get_neighborhood(
        &self,
        center: ElementId,
        options: NeighborhoodOptions,
    ) -> Result<Subgraph, GraphError> {
        let mut bindings = serde_json::Map::new();
        bindings.insert("center_id".to_string(), id_to_json(center.clone()));

        let edge_step = match options.direction {
            Direction::Outgoing => "outE",
            Direction::Incoming => "inE",
            Direction::Both => "bothE",
        };
        let mut gremlin = format!(
            "g.V(center_id).repeat({}().otherV().simplePath()).times({}).path()",
            edge_step, options.depth
        );
        if let Some(lim) = options.max_vertices {
            gremlin.push_str(&format!(".limit({})", lim));
        }

        let response = self.api.execute(&gremlin, Some(Value::Object(bindings)))?;
        println!("[DEBUG][get_neighborhood] Raw response: {}", serde_json::to_string_pretty(&response).unwrap_or_default());

        // Handle GraphSON g:List format (same as find_shortest_path and find_all_paths)
        let data_array = if let Some(data) = response["result"]["data"].as_object() {
            if data.get("@type") == Some(&Value::String("g:List".to_string())) {
                data.get("@value").and_then(|v| v.as_array())
            } else {
                None
            }
        } else {
            response["result"]["data"].as_array()
        };
        
        if let Some(arr) = data_array {
            println!("[DEBUG][get_neighborhood] Data array length: {}", arr.len());
            let mut verts = std::collections::HashMap::new();
            let mut edges = std::collections::HashMap::new();
            for val in arr {
                println!("[DEBUG][get_neighborhood] Processing path: {}", serde_json::to_string_pretty(val).unwrap_or_else(|_| format!("{:?}", val)));
                let path = parse_path_from_gremlin(val)?;
                for v in path.vertices {
                    verts.insert(element_id_to_key(&v.id), v);
                }
                for e in path.edges {
                    edges.insert(element_id_to_key(&e.id), e);
                }
            }
            
            Ok(Subgraph {
                vertices: verts.into_values().collect(),
                edges: edges.into_values().collect(),
            })
        } else {
            println!("[DEBUG][get_neighborhood] No data array in response");
            Ok(Subgraph {
                vertices: Vec::new(),
                edges: Vec::new(),
            })
        }
    }

    pub fn path_exists(
        &self,
        from_vertex: ElementId,
        to_vertex: ElementId,
        options: Option<PathOptions>,
    ) -> Result<bool, GraphError> {
        self.find_all_paths(from_vertex, to_vertex, options, Some(1))
            .map(|p| !p.is_empty())
    }

    pub fn get_vertices_at_distance(
        &self,
        source: ElementId,
        distance: u32,
        direction: Direction,
        edge_types: Option<Vec<String>>,
    ) -> Result<Vec<Vertex>, GraphError> {
        let mut bindings = serde_json::Map::new();
        bindings.insert("source_id".to_string(), id_to_json(source));

        let step = match direction {
            Direction::Outgoing => "out",
            Direction::Incoming => "in",
            Direction::Both => "both",
        }
        .to_string();
        let label_key = if let Some(labels) = &edge_types {
            if !labels.is_empty() {
                bindings.insert("edge_labels".to_string(), json!(labels));
                "edge_labels".to_string()
            } else {
                "".to_string()
            }
        } else {
            "".to_string()
        };

        let gremlin = format!(
            "g.V(source_id).repeat({}({})).times({}).dedup().elementMap()",
            step, label_key, distance
        );
        
        println!("[DEBUG][get_vertices_at_distance] Executing query: {}", gremlin);
        println!("[DEBUG][get_vertices_at_distance] Bindings: {:?}", bindings);
        
        let response = self.api.execute(&gremlin, Some(Value::Object(bindings)))?;
        println!("[DEBUG][get_vertices_at_distance] Raw response: {}", serde_json::to_string_pretty(&response).unwrap_or_else(|_| format!("{:?}", response)));

        // Handle GraphSON g:List format (same as other methods)
        let data_array = if let Some(data) = response["result"]["data"].as_object() {
            if data.get("@type") == Some(&Value::String("g:List".to_string())) {
                data.get("@value").and_then(|v| v.as_array())
            } else {
                None
            }
        } else {
            response["result"]["data"].as_array()
        };

        if let Some(arr) = data_array {
            println!("[DEBUG][get_vertices_at_distance] Data array length: {}", arr.len());
            arr.iter().map(parse_vertex_from_gremlin).collect()
        } else {
            println!("[DEBUG][get_vertices_at_distance] No data array in response");
            Ok(Vec::new())
        }
    }
}

impl TraversalGuest for GraphJanusGraphComponent {
    fn find_shortest_path(
        transaction: golem_graph::golem::graph::transactions::TransactionBorrow<'_>,
        from_vertex: ElementId,
        to_vertex: ElementId,
        options: Option<PathOptions>,
    ) -> Result<Option<Path>, GraphError> {
        let tx: &Transaction = transaction.get();
        tx.find_shortest_path(from_vertex, to_vertex, options)
    }

    fn find_all_paths(
        transaction: golem_graph::golem::graph::transactions::TransactionBorrow<'_>,
        from_vertex: ElementId,
        to_vertex: ElementId,
        options: Option<PathOptions>,
        limit: Option<u32>,
    ) -> Result<Vec<Path>, GraphError> {
        let tx: &Transaction = transaction.get();
        tx.find_all_paths(from_vertex, to_vertex, options, limit)
    }

    fn get_neighborhood(
        transaction: golem_graph::golem::graph::transactions::TransactionBorrow<'_>,
        center: ElementId,
        options: NeighborhoodOptions,
    ) -> Result<Subgraph, GraphError> {
        let tx: &Transaction = transaction.get();
        tx.get_neighborhood(center, options)
    }

    fn path_exists(
        transaction: golem_graph::golem::graph::transactions::TransactionBorrow<'_>,
        from_vertex: ElementId,
        to_vertex: ElementId,
        options: Option<PathOptions>,
    ) -> Result<bool, GraphError> {
        let tx: &Transaction = transaction.get();
        tx.path_exists(from_vertex, to_vertex, options)
    }

    fn get_vertices_at_distance(
        transaction: golem_graph::golem::graph::transactions::TransactionBorrow<'_>,
        source: ElementId,
        distance: u32,
        direction: Direction,
        edge_types: Option<Vec<String>>,
    ) -> Result<Vec<Vertex>, GraphError> {
        let tx: &Transaction = transaction.get();
        tx.get_vertices_at_distance(source, distance, direction, edge_types)
    }
}
