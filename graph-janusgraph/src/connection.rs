use crate::{Graph, Transaction};
use golem_graph::{
    durability::ProviderGraph,
    golem::graph::{
        connection::{GraphStatistics, GuestGraph},
        errors::GraphError,
        transactions::Transaction as TransactionResource,
    },
};

impl ProviderGraph for Graph {
    type Transaction = Transaction;
}

impl GuestGraph for Graph {
    fn begin_transaction(&self) -> Result<TransactionResource, GraphError> {
        self.api.execute("g.tx().open()", None)?;
               let transaction = Transaction::new(self.api.clone());
               Ok(TransactionResource::new(transaction))
    }

    fn begin_read_transaction(&self) -> Result<TransactionResource, GraphError> {
        self.begin_transaction()
    }

    fn ping(&self) -> Result<(), GraphError> {
        self.api.execute("1+1", None)?;
        Ok(())
    }

    fn close(&self) -> Result<(), GraphError> {
        // The underlying HTTP client doesn't need explicit closing for this implementation.
        Ok(())
    }

    fn get_statistics(&self) -> Result<GraphStatistics, GraphError> {
        let vertex_count_res = self.api.execute("g.V().count()", None)?;
        let edge_count_res = self.api.execute("g.E().count()", None)?;

        // Helper to extract count from JanusGraph response
        fn extract_count(val: &serde_json::Value) -> Option<u64> {
            val.get("result")
                .and_then(|r| r.get("data"))
                .and_then(|d| {
                    // JanusGraph returns: { "@type": "g:List", "@value": [ { ... } ] }
                    if let Some(list) = d.get("@value").and_then(|v| v.as_array()) {
                        list.first()
                    } else if let Some(arr) = d.as_array() {
                        arr.first()
                    } else {
                        None
                    }
                })
                .and_then(|v| {
                    // The count is usually a number or an object with @type/@value
                    if let Some(n) = v.as_u64() {
                        Some(n)
                    } else if let Some(obj) = v.as_object() {
                        if let Some(val) = obj.get("@value") {
                            val.as_u64()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
        }

        let vertex_count = extract_count(&vertex_count_res);
        let edge_count = extract_count(&edge_count_res);

        Ok(GraphStatistics {
            vertex_count,
            edge_count,
            label_count: None, // JanusGraph requires a more complex query for this
            property_count: None, // JanusGraph requires a more complex query for this
        })
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::client::JanusGraphApi;
//     use golem_graph::golem::graph::transactions::GuestTransaction;
//     use std::{env, sync::Arc};

//     fn get_test_graph() -> Graph {
//         let host = env::var("JANUSGRAPH_HOST").unwrap_or_else(|_| "localhost".into());
//         let port: u16 = env::var("JANUSGRAPH_PORT")
//             .unwrap_or_else(|_| "8182".into())
//             .parse()
//             .unwrap();
//         let api = JanusGraphApi::new(&host, port, None, None).unwrap();
//         Graph { api: Arc::new(api) }
//     }

//     fn create_test_transaction() -> Transaction {
//         let host = env::var("JANUSGRAPH_HOST").unwrap_or_else(|_| "localhost".into());
//         let port: u16 = env::var("JANUSGRAPH_PORT")
//             .unwrap_or_else(|_| "8182".into())
//             .parse()
//             .unwrap();
//         let api = JanusGraphApi::new(&host, port, None, None).unwrap();
//         // this returns your crate::Transaction
//         Transaction { api: Arc::new(api) }
//     }

//     fn create_test_transaction_with_api(api: Arc<JanusGraphApi>) -> Transaction {
//         Transaction { api }
//     }

//     #[test]
//     fn test_ping() {
//         // if env::var("JANUSGRAPH_HOST").is_err() {
//         //     println!("Skipping test_ping: JANUSGRAPH_HOST not set");
//         //     return;
//         // }
//         let graph = get_test_graph();
//         assert!(graph.ping().is_ok());
//     }

//     #[test]
//     fn test_get_statistics() {
//         let session_id = uuid::Uuid::new_v4().to_string();
//         let api = Arc::new(JanusGraphApi::new_with_session(
//             &env::var("JANUSGRAPH_HOST").unwrap_or_else(|_| "localhost".into()),
//             env::var("JANUSGRAPH_PORT").unwrap_or_else(|_| "8182".into()).parse().unwrap(),
//             None, None,
//             session_id.clone()
//         ).unwrap());
        
//         // Helper to extract count from JanusGraph response (same as get_statistics method)
//         fn extract_count(val: &serde_json::Value) -> Option<u64> {
//             val.get("result")
//                 .and_then(|r| r.get("data"))
//                 .and_then(|d| {
//                     // JanusGraph returns: { "@type": "g:List", "@value": [ { ... } ] }
//                     if let Some(list) = d.get("@value").and_then(|v| v.as_array()) {
//                         list.first()
//                     } else if let Some(arr) = d.as_array() {
//                         arr.first()
//                     } else {
//                         None
//                     }
//                 })
//                 .and_then(|v| {
//                     // The count is usually a number or an object with @type/@value
//                     if let Some(n) = v.as_u64() {
//                         Some(n)
//                     } else if let Some(obj) = v.as_object() {
//                         if let Some(val) = obj.get("@value") {
//                             val.as_u64()
//                         } else {
//                             None
//                         }
//                     } else {
//                         None
//                     }
//                 })
//         }
        
//         // Clean up StatNode vertices before test
//         let tx_cleanup = create_test_transaction_with_api(api.clone());
//         let _ = tx_cleanup.execute_query("g.V().hasLabel('StatNode').drop()".to_string(), None, None);
//         tx_cleanup.commit().unwrap();
        
//         // Use the same transaction for all operations (like traversal tests)
//         let tx = create_test_transaction_with_api(api.clone());
        
//         let v1 = tx.create_vertex("StatNode".to_string(), vec![]).unwrap();
//         let v2 = tx.create_vertex("StatNode".to_string(), vec![]).unwrap();
//         eprintln!("[DEBUG] v1: {:?}", v1);
//         eprintln!("[DEBUG] v2: {:?}", v2);
        
//         let edge_result = tx.create_edge(
//             "STAT_EDGE".to_string(),
//             v1.id.clone(),
//             v2.id.clone(),
//             vec![],
//         );
//         eprintln!("[DEBUG] Edge creation result: {:?}", edge_result);
        
//         // Query for visibility before commit using API directly (like get_statistics method)
//         let mut statnode_count_val = 0;
//         let mut statedge_count_val = 0;
//         let mut retries = 0;
        
//         while retries < 10 {
//             let statnode_count_res = tx.api.execute("g.V().hasLabel('StatNode').count()", None).unwrap();
//             let statedge_count_res = tx.api.execute("g.E().hasLabel('STAT_EDGE').count()", None).unwrap();
            
//             statnode_count_val = extract_count(&statnode_count_res).unwrap_or(0);
//             statedge_count_val = extract_count(&statedge_count_res).unwrap_or(0);
            
//             eprintln!("[DEBUG][Retry {}] StatNode count: {}, STAT_EDGE count: {}", retries, statnode_count_val, statedge_count_val);
            
//             if statnode_count_val >= 2 && statedge_count_val >= 1 {
//                 break;
//             }
            
//             std::thread::sleep(std::time::Duration::from_millis(300));
//             retries += 1;
//         }
        
//         assert!(statnode_count_val >= 2, "Expected at least 2 StatNode vertices, got {}", statnode_count_val);
//         assert!(statedge_count_val >= 1, "Expected at least 1 STAT_EDGE edge, got {}", statedge_count_val);
        
//         // Clean up after test
//         let _ = tx.execute_query("g.V().hasLabel('StatNode').drop()".to_string(), None, None);
//     }

//     #[test]
//     fn test_create_statnode_and_edge() {
//         let session_id = uuid::Uuid::new_v4().to_string();
//         let api = Arc::new(JanusGraphApi::new_with_session(
//             &env::var("JANUSGRAPH_HOST").unwrap_or_else(|_| "localhost".into()),
//             env::var("JANUSGRAPH_PORT").unwrap_or_else(|_| "8182".into()).parse().unwrap(),
//             None, None,
//             session_id.clone()
//         ).unwrap());
        
//         // Setup: clean up before test
//         let tx_cleanup = create_test_transaction_with_api(api.clone());
//         let _ = tx_cleanup.execute_query("g.V().hasLabel('StatNode').drop()".to_string(), None, None);
//         tx_cleanup.commit().unwrap();
        
//         // Use the same transaction for all operations (consistent with other tests)
//         let tx = create_test_transaction_with_api(api.clone());
//         let v1 = tx.create_vertex("StatNode".to_string(), vec![]).unwrap();
//         let v2 = tx.create_vertex("StatNode".to_string(), vec![]).unwrap();
//         eprintln!("[DEBUG] v1: {:?}", v1);
//         eprintln!("[DEBUG] v2: {:?}", v2);
        
//         let edge_result = tx.create_edge(
//             "STAT_EDGE".to_string(),
//             v1.id.clone(),
//             v2.id.clone(),
//             vec![],
//         );
//         eprintln!("[DEBUG] Edge creation result: {:?}", edge_result);
        
//         // Clean up after test
//         let _ = tx.execute_query("g.V().hasLabel('StatNode').drop()".to_string(), None, None);
//     }

//     #[test]
//     fn test_statnode_and_edge_persistence() {
//         let session_id = uuid::Uuid::new_v4().to_string();
//         let api = Arc::new(JanusGraphApi::new_with_session(
//             &env::var("JANUSGRAPH_HOST").unwrap_or_else(|_| "localhost".into()),
//             env::var("JANUSGRAPH_PORT").unwrap_or_else(|_| "8182".into()).parse().unwrap(),
//             None, None,
//             session_id.clone()
//         ).unwrap());
        
//         // Use unique labels to avoid test interference
//         let uuid_str = uuid::Uuid::new_v4().to_string().replace('-', "");
//         let vertex_label = format!("StatNodePersist_{}", &uuid_str[..8]);
//         let edge_label = format!("STAT_EDGE_Persist_{}", &uuid_str[..8]);
        
//         // Helper to extract count from JanusGraph response (same as other tests)
//         fn extract_count(val: &serde_json::Value) -> Option<u64> {
//             val.get("result")
//                 .and_then(|r| r.get("data"))
//                 .and_then(|d| {
//                     // JanusGraph returns: { "@type": "g:List", "@value": [ { ... } ] }
//                     if let Some(list) = d.get("@value").and_then(|v| v.as_array()) {
//                         list.first()
//                     } else if let Some(arr) = d.as_array() {
//                         arr.first()
//                     } else {
//                         None
//                     }
//                 })
//                 .and_then(|v| {
//                     // The count is usually a number or an object with @type/@value
//                     if let Some(n) = v.as_u64() {
//                         Some(n)
//                     } else if let Some(obj) = v.as_object() {
//                         if let Some(val) = obj.get("@value") {
//                             val.as_u64()
//                         } else {
//                             None
//                         }
//                     } else {
//                         None
//                     }
//                 })
//         }
        
//         // Clean up before test
//         let tx_cleanup = create_test_transaction_with_api(api.clone());
//         let _ = tx_cleanup.execute_query(format!("g.V().hasLabel('{}').drop()", vertex_label), None, None);
//         tx_cleanup.commit().unwrap();
        
//         // Use the same transaction for all operations (consistent with other tests)
//         let tx = create_test_transaction_with_api(api.clone());
//         let v1 = tx.create_vertex(vertex_label.clone(), vec![]).unwrap();
//         let v2 = tx.create_vertex(vertex_label.clone(), vec![]).unwrap();
//         let _ = tx.create_edge(edge_label.clone(), v1.id.clone(), v2.id.clone(), vec![]);
        
//         // Query for visibility using API directly with retry logic (like other tests)
//         let mut statnode_count_val = 0;
//         let mut statedge_count_val = 0;
//         let mut retries = 0;
        
//         while retries < 10 {
//             let statnode_count_res = tx.api.execute(&format!("g.V().hasLabel('{}').count()", vertex_label), None).unwrap();
//             let statedge_count_res = tx.api.execute(&format!("g.E().hasLabel('{}').count()", edge_label), None).unwrap();
            
//             statnode_count_val = extract_count(&statnode_count_res).unwrap_or(0);
//             statedge_count_val = extract_count(&statedge_count_res).unwrap_or(0);
            
//             eprintln!("[DEBUG][Retry {}] {} count: {}, {} count: {}", retries, vertex_label, statnode_count_val, edge_label, statedge_count_val);
            
//             if statnode_count_val >= 2 && statedge_count_val >= 1 {
//                 break;
//             }
            
//             std::thread::sleep(std::time::Duration::from_millis(300));
//             retries += 1;
//         }
        
//         assert!(statnode_count_val >= 2, "Expected at least 2 {} vertices, got {}", vertex_label, statnode_count_val);
//         assert!(statedge_count_val >= 1, "Expected at least 1 {} edge, got {}", edge_label, statedge_count_val);
        
//         // Clean up after test
//         let _ = tx.execute_query(format!("g.V().hasLabel('{}').drop()", vertex_label), None, None);
//     }

//     #[test]
//     fn test_get_statistics_robust() {
//         let session_id = uuid::Uuid::new_v4().to_string();
//         let api = Arc::new(JanusGraphApi::new_with_session(
//             &env::var("JANUSGRAPH_HOST").unwrap_or_else(|_| "localhost".into()),
//             env::var("JANUSGRAPH_PORT").unwrap_or_else(|_| "8182".into()).parse().unwrap(),
//             None, None,
//             session_id.clone()
//         ).unwrap());
        
//         // Use unique labels to avoid test interference
//         let uuid_str = uuid::Uuid::new_v4().to_string().replace('-', "");
//         let vertex_label = format!("StatNodeRobust_{}", &uuid_str[..8]);
//         let edge_label = format!("STAT_EDGE_Robust_{}", &uuid_str[..8]);
        
//         // Helper to extract count from JanusGraph response (same as other tests)
//         fn extract_count(val: &serde_json::Value) -> Option<u64> {
//             val.get("result")
//                 .and_then(|r| r.get("data"))
//                 .and_then(|d| {
//                     // JanusGraph returns: { "@type": "g:List", "@value": [ { ... } ] }
//                     if let Some(list) = d.get("@value").and_then(|v| v.as_array()) {
//                         list.first()
//                     } else if let Some(arr) = d.as_array() {
//                         arr.first()
//                     } else {
//                         None
//                     }
//                 })
//                 .and_then(|v| {
//                     // The count is usually a number or an object with @type/@value
//                     if let Some(n) = v.as_u64() {
//                         Some(n)
//                     } else if let Some(obj) = v.as_object() {
//                         if let Some(val) = obj.get("@value") {
//                             val.as_u64()
//                         } else {
//                             None
//                         }
//                     } else {
//                         None
//                     }
//                 })
//         }
        
//         // Clean up vertices with unique label before test
//         let tx_cleanup = create_test_transaction_with_api(api.clone());
//         let _ = tx_cleanup.execute_query(format!("g.V().hasLabel('{}').drop()", vertex_label), None, None);
//         tx_cleanup.commit().unwrap();
        
//         // Use the same transaction for all operations (consistent with other tests)
//         let tx = create_test_transaction_with_api(api.clone());
        
//         // Get baseline counts
//         let statnode_count_before_res = tx.api.execute(&format!("g.V().hasLabel('{}').count()", vertex_label), None).unwrap();
//         let statedge_count_before_res = tx.api.execute(&format!("g.E().hasLabel('{}').count()", edge_label), None).unwrap();
//         let statnode_count_before_val = extract_count(&statnode_count_before_res).unwrap_or(0);
//         let statedge_count_before_val = extract_count(&statedge_count_before_res).unwrap_or(0);
        
//         let v1 = tx.create_vertex(vertex_label.clone(), vec![]).unwrap();
//         let v2 = tx.create_vertex(vertex_label.clone(), vec![]).unwrap();
//         let _ = tx.create_edge(edge_label.clone(), v1.id.clone(), v2.id.clone(), vec![]);
        
//         // Query for visibility with retry logic
//         let mut statnode_count_val = 0;
//         let mut statedge_count_val = 0;
//         let expected_vertex_count = statnode_count_before_val + 2;
//         let expected_edge_count = statedge_count_before_val + 1;
        
//         for attempt in 1..=10 {
//             let statnode_count_res = tx.api.execute(&format!("g.V().hasLabel('{}').count()", vertex_label), None).unwrap();
//             let statedge_count_res = tx.api.execute(&format!("g.E().hasLabel('{}').count()", edge_label), None).unwrap();
            
//             statnode_count_val = extract_count(&statnode_count_res).unwrap_or(0);
//             statedge_count_val = extract_count(&statedge_count_res).unwrap_or(0);
            
//             eprintln!("[DEBUG][Attempt {}] {} count: {} (expected {})", attempt, vertex_label, statnode_count_val, expected_vertex_count);
//             eprintln!("[DEBUG][Attempt {}] {} count: {} (expected {})", attempt, edge_label, statedge_count_val, expected_edge_count);
            
//             if statnode_count_val >= expected_vertex_count && statedge_count_val >= expected_edge_count {
//                 break;
//             }
            
//             std::thread::sleep(std::time::Duration::from_millis(300));
//         }
        
//         assert!(statnode_count_val >= expected_vertex_count, "Expected at least {} {} vertices, got {}", expected_vertex_count, vertex_label, statnode_count_val);
//         assert!(statedge_count_val >= expected_edge_count, "Expected at least {} {} edges, got {}", expected_edge_count, edge_label, statedge_count_val);
        
//         // Clean up after test
//         let _ = tx.execute_query(format!("g.V().hasLabel('{}').drop()", vertex_label), None, None);
//     }
// }
