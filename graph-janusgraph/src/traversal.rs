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

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::client::JanusGraphApi;
//     use golem_graph::golem::graph::transactions::GuestTransaction;
//     use golem_graph::golem::graph::types::{FilterCondition, ComparisonOperator, PropertyValue};
//     use std::sync::Arc;
//     use std::{collections::HashMap, env};

//     fn create_test_api() -> Arc<JanusGraphApi> {
//     let host = env::var("JANUSGRAPH_HOST").unwrap_or_else(|_| "localhost".to_string());
//     let port = env::var("JANUSGRAPH_PORT")
//         .unwrap_or_else(|_| "8182".to_string())
//         .parse()
//         .unwrap();
//     Arc::new(JanusGraphApi::new(&host, port, None, None).unwrap())
// }

// fn create_test_transaction_with_api(api: Arc<JanusGraphApi>) -> Transaction {
//     Transaction { api }
// }

//     fn setup_modern_graph(tx: &Transaction) -> HashMap<String, ElementId> {
//         // Print session id
//         println!("[SETUP][SESSION] Using session id: {}", tx.api.session_id());
//         cleanup_modern_graph(tx);
//         let mut ids = HashMap::new();
//         let props = |_name, label: &str, val: &str| {
//             (
//                 label.to_string(),
//                 PropertyValue::StringValue(val.to_string()),
//             )
//         };
//         // Create vertices using create_vertex to avoid duplicates
//         let marko = tx
//             .create_vertex(
//                 "person".to_string(),
//                 vec![
//                     props("name", "name", "marko"),
//                     ("age".to_string(), PropertyValue::Int64(29)),
//                 ],
//             )
//             .unwrap();
//         println!("[SETUP] Created vertex marko: {:?}", marko);
//         ids.insert("marko".to_string(), marko.id.clone());
//         let vadas = tx
//             .create_vertex(
//                 "person".to_string(),
//                 vec![
//                     props("name", "name", "vadas"),
//                     ("age".to_string(), PropertyValue::Int64(27)),
//                 ],
//             )
//             .unwrap();
//         println!("[SETUP] Created vertex vadas: {:?}", vadas);
//         ids.insert("vadas".to_string(), vadas.id.clone());
//         let josh = tx
//             .create_vertex(
//                 "person".to_string(),
//                 vec![
//                     props("name", "name", "josh"),
//                     ("age".to_string(), PropertyValue::Int64(32)),
//                 ],
//             )
//             .unwrap();
//         println!("[SETUP] Created vertex josh: {:?}", josh);
//         ids.insert("josh".to_string(), josh.id.clone());
//         let peter = tx
//             .create_vertex(
//                 "person".to_string(),
//                 vec![
//                     props("name", "name", "peter"),
//                     ("age".to_string(), PropertyValue::Int64(35)),
//                 ],
//             )
//             .unwrap();
//         println!("[SETUP] Created vertex peter: {:?}", peter);
//         ids.insert("peter".to_string(), peter.id.clone());
//         let lop = tx
//             .create_vertex(
//                 "software".to_string(),
//                 vec![
//                     props("name", "name", "lop"),
//                     ("lang".to_string(), PropertyValue::StringValue("java".to_string())),
//                 ],
//             )
//             .unwrap();
//         println!("[SETUP] Created vertex lop: {:?}", lop);
//         ids.insert("lop".to_string(), lop.id.clone());
//         let ripple = tx
//             .create_vertex(
//                 "software".to_string(),
//                 vec![
//                     props("name", "name", "ripple"),
//                     ("lang".to_string(), PropertyValue::StringValue("java".to_string())),
//                 ],
//             )
//             .unwrap();
//         println!("[SETUP] Created vertex ripple: {:?}", ripple);
//         ids.insert("ripple".to_string(), ripple.id.clone());

//         // Print all vertices after creation
//         let verts = tx.find_vertices(None, None, None, None, None).unwrap_or_default();
//         println!("[DEBUG][SETUP] All vertices after creation:");
//         for v in &verts {
//             println!("  id: {:?}, type: {:?}, properties: {:?}", v.id, v.vertex_type, v.properties);
//         }

//         // Edges
//         let e1 = tx.create_edge(
//             "knows".to_string(),
//             ids["marko"].clone(),
//             ids["vadas"].clone(),
//             vec![("weight".to_string(), PropertyValue::Float64(0.5))],
//         ).unwrap();
//         println!("[SETUP] Created edge marko-knows-vadas: {:?}", e1);
//         let e2 = tx.create_edge(
//             "knows".to_string(),
//             ids["marko"].clone(),
//             ids["josh"].clone(),
//             vec![("weight".to_string(), PropertyValue::Float64(1.0))],
//         ).unwrap();
//         println!("[SETUP] Created edge marko-knows-josh: {:?}", e2);
//         let e3 = tx.create_edge(
//             "created".to_string(),
//             ids["marko"].clone(),
//             ids["lop"].clone(),
//             vec![("weight".to_string(), PropertyValue::Float64(0.4))],
//         ).unwrap();
//         println!("[SETUP] Created edge marko-created-lop: {:?}", e3);
//         let e4 = tx.create_edge(
//             "created".to_string(),
//             ids["josh"].clone(),
//             ids["ripple"].clone(),
//             vec![("weight".to_string(), PropertyValue::Float64(1.0))],
//         ).unwrap();
//         println!("[SETUP] Created edge josh-created-ripple: {:?}", e4);
//         let e5 = tx.create_edge(
//             "created".to_string(),
//             ids["josh"].clone(),
//             ids["lop"].clone(),
//             vec![("weight".to_string(), PropertyValue::Float64(0.4))],
//         ).unwrap();
//         println!("[SETUP] Created edge josh-created-lop: {:?}", e5);
//         let e6 = tx.create_edge(
//             "created".to_string(),
//             ids["peter"].clone(),
//             ids["lop"].clone(),
//             vec![("weight".to_string(), PropertyValue::Float64(0.2))],
//         ).unwrap();
//         println!("[SETUP] Created edge peter-created-lop: {:?}", e6);
//         ids
//     }

//     fn cleanup_modern_graph(tx: &Transaction) {
//         let mut attempts = 0;
//         let max_attempts = 5;
//         loop {
//             attempts += 1;
//             let res1 = tx.execute_query("g.V().hasLabel('person').drop()".to_string(), None, None);
//             let res2 = tx.execute_query("g.V().hasLabel('software').drop()".to_string(), None, None);
//             let commit_res = tx.commit();
//             let lock_err = |e: &golem_graph::golem::graph::errors::GraphError| {
//                 matches!(e, golem_graph::golem::graph::errors::GraphError::InvalidQuery(msg) if msg.contains("Lock expired"))
//             };
//             if res1.as_ref().err().map_or(false, lock_err)
//                 || res2.as_ref().err().map_or(false, lock_err)
//                 || commit_res.as_ref().err().map_or(false, lock_err)
//             {
//                 if attempts < max_attempts {
//                     println!("[WARN] LockTimeout in cleanup_modern_graph, retrying ({}/{})...", attempts, max_attempts);
//                     std::thread::sleep(std::time::Duration::from_millis(500));
//                     continue;
//                 } else {
//                     println!("[ERROR] LockTimeout in cleanup_modern_graph after {} attempts, giving up!", max_attempts);
//                 }
//             }
//             break;
//         }
//     }

//     fn fetch_modern_graph_ids(tx: &Transaction) -> HashMap<String, ElementId> {
//         let mut ids = HashMap::new();
//         let names = ["marko", "vadas", "josh", "peter", "lop", "ripple"];
//         let mut retries = 0;
//         let max_retries = 10;
//         while retries < max_retries {
//             ids.clear();
//             for name in names.iter() {
//                 let filter = FilterCondition {
//                     property: "name".to_string(),
//                     operator: ComparisonOperator::Equal,
//                     value: PropertyValue::StringValue(name.to_string()),
//                 };
//                 let verts = tx.find_vertices(
//                     None,
//                     Some(vec![filter]),
//                     None, None, None
//                 ).unwrap_or_default();
//                 println!("[DEBUG][FETCH_IDS] For name '{}', found vertices: {:?}", name, verts);
//                 if let Some(v) = verts.first() {
//                     ids.insert(name.to_string(), v.id.clone());
//                 }
//             }
//             if ids.len() == names.len() {
//                 break;
//             }
//             std::thread::sleep(std::time::Duration::from_millis(300));
//             retries += 1;
//         }
//         println!("[DEBUG][FETCH_IDS] Final ids map: {:?}", ids);
//         ids
//     }

//    #[test]
// fn test_find_shortest_path() {
//     let api = create_test_api();
//     let tx_setup = create_test_transaction_with_api(api.clone());
//     setup_modern_graph(&tx_setup);
//     tx_setup.commit().unwrap();
//     // Use the same transaction for traversal and queries
//     let tx = &tx_setup;
//     let mut verts = vec![];
//     let mut edges = vec![];
//     let mut retries = 0;
//     while (verts.is_empty() || edges.is_empty()) && retries < 10 {
//         verts = tx.find_vertices(None, None, None, None, None).unwrap_or_default();
//         edges = tx.find_edges(None, None, None, None, None).unwrap_or_default();
//         if verts.is_empty() || edges.is_empty() {
//             std::thread::sleep(std::time::Duration::from_millis(300));
//         }
//         retries += 1;
//     }
//     // Debug print all vertices and their properties
//     println!("[DEBUG][TEST] All vertices after setup:");
//     for v in &verts {
//         println!("  id: {:?}, type: {:?}, properties: {:?}", v.id, v.vertex_type, v.properties);
//     }
//     let ids = fetch_modern_graph_ids(tx);
//     assert!(ids.contains_key("marko"), "Vertex 'marko' not found in ids: {:?}", ids);
//     assert!(ids.contains_key("ripple"), "Vertex 'ripple' not found in ids: {:?}", ids);
//     let mut path_opt = tx.find_shortest_path(ids["marko"].clone(), ids["ripple"].clone(), None);
//     let mut retries = 0;
//     while !matches!(path_opt.as_ref().ok(), Some(Some(_))) && retries < 10 {
//         std::thread::sleep(std::time::Duration::from_millis(300));
//         path_opt = tx.find_shortest_path(ids["marko"].clone(), ids["ripple"].clone(), None);
//         retries += 1;
//     }
//     let path = path_opt.expect("No path result").expect("No path found from marko to ripple");
//     assert_eq!(path.vertices.len(), 3);
//     assert_eq!(path.edges.len(), 2);
//     cleanup_modern_graph(tx);
// }

//     #[test]
//     fn test_path_exists() {
//         let api = create_test_api();
//         let tx_setup = create_test_transaction_with_api(api.clone());
//         setup_modern_graph(&tx_setup);
        
//         // Use the same transaction for setup and queries (like test_find_shortest_path)
//         let tx = &tx_setup;
        
//         // Wait for data to be available with robust retry logic
//         let mut verts = vec![];
//         let mut edges = vec![];
//         let mut retries = 0;
//         while (verts.is_empty() || edges.is_empty()) && retries < 10 {
//             verts = tx.find_vertices(None, None, None, None, None).unwrap_or_default();
//             edges = tx.find_edges(None, None, None, None, None).unwrap_or_default();
//             if verts.is_empty() || edges.is_empty() {
//                 std::thread::sleep(std::time::Duration::from_millis(300));
//             }
//             retries += 1;
//         }
        
//         // Debug print for troubleshooting
//         println!("[DEBUG][test_path_exists] Vertices found: {}, Edges found: {}", verts.len(), edges.len());
        
//         let ids = fetch_modern_graph_ids(tx);
//         assert!(ids.contains_key("marko"), "Vertex 'marko' not found in ids: {:?}", ids);
//         assert!(ids.contains_key("ripple"), "Vertex 'ripple' not found in ids: {:?}", ids);
//         assert!(ids.contains_key("vadas"), "Vertex 'vadas' not found in ids: {:?}", ids);
//         assert!(ids.contains_key("peter"), "Vertex 'peter' not found in ids: {:?}", ids);
        
//         // Test path exists with retry logic
//         let mut path_exists_result = None;
//         let mut retries = 0;
//         while path_exists_result.is_none() && retries < 10 {
//             match tx.path_exists(ids["marko"].clone(), ids["ripple"].clone(), None) {
//                 Ok(exists) if exists => {
//                     path_exists_result = Some(true);
//                 }
//                 Ok(_) => {
//                     std::thread::sleep(std::time::Duration::from_millis(300));
//                 }
//                 Err(e) => {
//                     println!("[DEBUG][test_path_exists] Error checking path existence: {:?}", e);
//                     std::thread::sleep(std::time::Duration::from_millis(300));
//                 }
//             }
//             retries += 1;
//         }
        
//         assert!(path_exists_result.unwrap_or(false), "Path from marko to ripple should exist");
        
//         // Test path that exists through shared connections (vadas to peter via marko and lop)
//         assert!(tx.path_exists(ids["vadas"].clone(), ids["peter"].clone(), None).unwrap(), 
//                 "Path from vadas to peter should exist via marko and lop");
        
//         cleanup_modern_graph(tx);
//     }

//     #[test]
//     fn test_find_all_paths() {
//         let api = create_test_api();
//         let tx_setup = create_test_transaction_with_api(api.clone());
//         setup_modern_graph(&tx_setup);
        
//         // Use the same transaction for setup and queries (like test_find_shortest_path)
//         let tx = &tx_setup;
        
//         // Wait for data to be available with robust retry logic
//         let mut verts = vec![];
//         let mut edges = vec![];
//         let mut retries = 0;
//         while (verts.is_empty() || edges.is_empty()) && retries < 10 {
//             verts = tx.find_vertices(None, None, None, None, None).unwrap_or_default();
//             edges = tx.find_edges(None, None, None, None, None).unwrap_or_default();
//             if verts.is_empty() || edges.is_empty() {
//                 std::thread::sleep(std::time::Duration::from_millis(300));
//             }
//             retries += 1;
//         }
        
//         // Debug print for troubleshooting
//         println!("[DEBUG][test_find_all_paths] Vertices found: {}, Edges found: {}", verts.len(), edges.len());
        
//         let ids = fetch_modern_graph_ids(tx);
//         assert!(ids.contains_key("marko"), "Vertex 'marko' not found in ids: {:?}", ids);
//         assert!(ids.contains_key("lop"), "Vertex 'lop' not found in ids: {:?}", ids);
        
//         // Test find_all_paths with retry logic
//         let mut paths = None;
//         let mut retries = 0;
//         while retries < 10 {
//             match tx.find_all_paths(ids["marko"].clone(), ids["lop"].clone(), None, Some(5)) {
//                 Ok(p) if p.len() >= 2 => {
//                     paths = Some(p);
//                     break;
//                 }
//                 Ok(p) => {
//                     println!("[DEBUG][test_find_all_paths] Found {} paths, expecting at least 2", p.len());
//                     std::thread::sleep(std::time::Duration::from_millis(300));
//                 }
//                 Err(e) => {
//                     println!("[DEBUG][test_find_all_paths] Error finding paths: {:?}", e);
//                     std::thread::sleep(std::time::Duration::from_millis(300));
//                 }
//             }
//             retries += 1;
//         }
        
//         let paths = paths.expect("Should find at least 2 paths from marko to lop");
//         assert_eq!(paths.len(), 2, "Expected 2 paths from marko to lop, found {}", paths.len());
        
//         cleanup_modern_graph(tx);
//     }

//     #[test]
//     fn test_get_neighborhood() {
//         let api = create_test_api();
//         let tx_setup = create_test_transaction_with_api(api.clone());
//         setup_modern_graph(&tx_setup);
        
//         // Use the same transaction for setup and queries (like test_find_shortest_path)
//         let tx = &tx_setup;
        
//         // Wait for data to be available with robust retry logic
//         let mut verts = vec![];
//         let mut edges = vec![];
//         let mut retries = 0;
//         while (verts.is_empty() || edges.is_empty()) && retries < 10 {
//             verts = tx.find_vertices(None, None, None, None, None).unwrap_or_default();
//             edges = tx.find_edges(None, None, None, None, None).unwrap_or_default();
//             if verts.is_empty() || edges.is_empty() {
//                 std::thread::sleep(std::time::Duration::from_millis(300));
//             }
//             retries += 1;
//         }
        
//         // Debug print for troubleshooting
//         println!("[DEBUG][test_get_neighborhood] Vertices found: {}, Edges found: {}", verts.len(), edges.len());
        
//         let ids = fetch_modern_graph_ids(tx);
//         assert!(ids.contains_key("marko"), "Vertex 'marko' not found in ids: {:?}", ids);
        
//         // Test get_neighborhood with retry logic
//         let mut sub = None;
//         let mut retries = 0;
//         while retries < 10 {
//             match tx.get_neighborhood(
//                 ids["marko"].clone(),
//                 NeighborhoodOptions {
//                     direction: Direction::Outgoing,
//                     depth: 1,
//                     edge_types: None,
//                     max_vertices: None,
//                 },
//             ) {
//                 Ok(s) if s.vertices.len() >= 4 && s.edges.len() >= 3 => {
//                     sub = Some(s);
//                     break;
//                 }
//                 Ok(s) => {
//                     println!("[DEBUG][test_get_neighborhood] Found {} vertices and {} edges, expecting at least 4 vertices and 3 edges", s.vertices.len(), s.edges.len());
//                     std::thread::sleep(std::time::Duration::from_millis(300));
//                 }
//                 Err(e) => {
//                     println!("[DEBUG][test_get_neighborhood] Error getting neighborhood: {:?}", e);
//                     std::thread::sleep(std::time::Duration::from_millis(300));
//                 }
//             }
//             retries += 1;
//         }
        
//         let sub = sub.expect("Should find neighborhood with at least 4 vertices and 3 edges");
//         assert_eq!(sub.vertices.len(), 4, "Expected 4 vertices in neighborhood, found {}", sub.vertices.len());
//         assert_eq!(sub.edges.len(), 3, "Expected 3 edges in neighborhood, found {}", sub.edges.len());
        
//         cleanup_modern_graph(tx);
//     }

//     #[test]
//     fn test_get_vertices_at_distance() {
//         let api = create_test_api();
//         let tx = create_test_transaction_with_api(api.clone());
//         setup_modern_graph(&tx);
        
//         // Get vertex IDs (retry if needed)
//         let mut ids = None;
//         for attempt in 0..10 {
//             match fetch_modern_graph_ids(&tx) {
//                 id_map if id_map.contains_key("marko") => {
//                     ids = Some(id_map);
//                     break;
//                 }
//                 _ => {
//                     println!("Attempt {}: Waiting for vertices to be available...", attempt + 1);
//                     std::thread::sleep(std::time::Duration::from_millis(300));
//                 }
//             }
//         }
//         let ids = ids.expect("Failed to get vertex IDs after retries");
        
//         // Get vertices at distance with retry logic (no separate edge visibility check)
//         let mut verts = None;
//         for attempt in 0..10 {
//             match tx.get_vertices_at_distance(ids["marko"].clone(), 2, Direction::Outgoing, None) {
//                 Ok(vertices) if vertices.len() >= 2 => {
//                     println!("Attempt {}: Found {} vertices at distance 2", attempt + 1, vertices.len());
//                     verts = Some(vertices);
//                     break;
//                 }
//                 Ok(vertices) => {
//                     println!("Attempt {}: Found {} vertices at distance 2 (expected at least 2)", attempt + 1, vertices.len());
//                     std::thread::sleep(std::time::Duration::from_millis(500));
//                 }
//                 Err(e) => {
//                     println!("Attempt {}: Error getting vertices at distance: {:?}", attempt + 1, e);
//                     std::thread::sleep(std::time::Duration::from_millis(500));
//                 }
//             }
//         }
        
//         let verts = verts.expect("Failed to get vertices at distance after retries");
//         assert_eq!(verts.len(), 2, "Expected 2 vertices at distance 2 from marko");
//         cleanup_modern_graph(&tx);
//     }

//     #[test]
//     fn test_unsupported_path_options() {
//         let api = create_test_api();
//         let tx_setup = create_test_transaction_with_api(api.clone());
//         setup_modern_graph(&tx_setup);
        
//         // Use the same transaction for setup and queries (like other tests)
//         let tx = &tx_setup;
        
//         // Wait for data to be available with robust retry logic
//         let mut verts = vec![];
//         let mut edges = vec![];
//         let mut retries = 0;
//         while (verts.is_empty() || edges.is_empty()) && retries < 10 {
//             verts = tx.find_vertices(None, None, None, None, None).unwrap_or_default();
//             edges = tx.find_edges(None, None, None, None, None).unwrap_or_default();
//             if verts.is_empty() || edges.is_empty() {
//                 std::thread::sleep(std::time::Duration::from_millis(300));
//             }
//             retries += 1;
//         }
        
//         // Debug print for troubleshooting
//         println!("[DEBUG][test_unsupported_path_options] Vertices found: {}, Edges found: {}", verts.len(), edges.len());
        
//         let ids = fetch_modern_graph_ids(tx);
//         assert!(ids.contains_key("marko"), "Vertex 'marko' not found in ids: {:?}", ids);
//         assert!(ids.contains_key("lop"), "Vertex 'lop' not found in ids: {:?}", ids);
        
//         let options = PathOptions {
//             vertex_types: Some(vec!["person".to_string()]),
//             edge_types: None,
//             max_depth: None,
//             vertex_filters: None,
//             edge_filters: None,
//         };
//         let result = tx.find_all_paths(
//             ids["marko"].clone(),
//             ids["lop"].clone(),
//             Some(options),
//             None,
//         );
//         assert!(matches!(result, Err(GraphError::UnsupportedOperation(_))));
//         cleanup_modern_graph(tx);
//     }
// }
