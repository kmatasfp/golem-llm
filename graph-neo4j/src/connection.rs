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
        let transaction_url = self.api.begin_transaction()?;
        let transaction = Transaction::new(self.api.clone(), transaction_url);
        Ok(TransactionResource::new(transaction))
    }

    fn begin_read_transaction(&self) -> Result<TransactionResource, GraphError> {
        let transaction_url = self.api.begin_transaction()?;
        let transaction = Transaction::new(self.api.clone(), transaction_url);
        Ok(TransactionResource::new(transaction))
    }

    fn ping(&self) -> Result<(), GraphError> {
        let transaction_url = self.api.begin_transaction()?;
        self.api.rollback_transaction(&transaction_url)
    }

    fn close(&self) -> Result<(), GraphError> {
        Ok(())
    }

    fn get_statistics(&self) -> Result<GraphStatistics, GraphError> {
        let transaction_url = self.api.begin_transaction()?;

        // Query for node count
        let node_count_stmt = serde_json::json!({
            "statement": "MATCH (n) RETURN count(n) as nodeCount",
            "parameters": {}
        });
        let node_count_resp = self.api.execute_in_transaction(&transaction_url, serde_json::json!({ "statements": [node_count_stmt] }))?;
        let node_count = node_count_resp["results"]
            .as_array()
            .and_then(|r| r.first())
            .and_then(|result| result["data"].as_array())
            .and_then(|d| d.first())
            .and_then(|data| data["row"].as_array())
            .and_then(|row| row.first())
            .and_then(|v| v.as_u64());

        // Query for relationship count
        let rel_count_stmt = serde_json::json!({
            "statement": "MATCH ()-[r]->() RETURN count(r) as relCount",
            "parameters": {}
        });
        let rel_count_resp = self.api.execute_in_transaction(&transaction_url, serde_json::json!({ "statements": [rel_count_stmt] }))?;
        let rel_count = rel_count_resp["results"]
            .as_array()
            .and_then(|r| r.first())
            .and_then(|result| result["data"].as_array())
            .and_then(|d| d.first())
            .and_then(|data| data["row"].as_array())
            .and_then(|row| row.first())
            .and_then(|v| v.as_u64());

        self.api.rollback_transaction(&transaction_url)?;

        Ok(GraphStatistics {
            vertex_count: node_count,
            edge_count: rel_count,
            label_count: None,
            property_count: None,
        })
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::GraphNeo4jComponent;
//     use golem_graph::durability::ExtendedGuest;
//     use golem_graph::golem::graph::{transactions::GuestTransaction};
//     use golem_graph::golem::graph::connection::ConnectionConfig;
//     use std::env;

//     use golem_graph::golem::graph::query::{ QueryParameters, QueryOptions};

// fn get_test_graph() -> Graph {

//      // 1) connect as before
//      let host = env::var("NEO4J_HOST").unwrap_or_else(|_| "localhost".to_string());
//      let port = env::var("NEO4J_PORT").unwrap_or_else(|_| "7474".to_string()).parse().unwrap();
//      let user = env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".to_string());
//      let password = env::var("NEO4J_PASSWORD").unwrap_or_else(|_| "password".to_string());
//      let database = env::var("NEO4J_DATABASE").unwrap_or_else(|_| "neo4j".to_string());
 
//      let config = ConnectionConfig {
//          hosts: vec![host],
//          port: Some(port),
//          username: Some(user),
//          password: Some(password),
//          database_name: Some(database),
//          timeout_seconds: None,
//          max_connections: None,
//          provider_config: vec![],
//      };
//     let graph = GraphNeo4jComponent::connect_internal(&config).unwrap();

//     // Start a transaction
//     let tx = Graph::begin_transaction(&graph).unwrap();

//     // Wipe everything via execute_query
//     tx.execute_query(
//         "MATCH (n) DETACH DELETE n".to_string(),
//         None::<QueryParameters>,
//         None::<QueryOptions>,
//     ).unwrap();

//     // Commit the cleanup
//     tx.commit().unwrap();

//     graph
// }

//     #[test]
//     fn test_ping() {
//         // if env::var("NEO4J_HOST").is_err() {
//         //     println!("Skipping test_ping: NEO4J_HOST not set");
//         //     return;
//         // }
//         let graph = get_test_graph();
//         let result = graph.ping();
//         assert!(result.is_ok());
//     }

//     #[test]
//     fn test_get_statistics() {
//         if env::var("NEO4J_HOST").is_err() {
//             println!("Skipping test_get_statistics: NEO4J_HOST not set");
//             return;
//         }

//         let graph = get_test_graph();
        
//         let tx: Transaction = Graph::begin_transaction(&graph).unwrap();

//         let initial_stats = graph.get_statistics().unwrap();

//         let v1 = tx.create_vertex("StatNode".to_string(), vec![]).unwrap();
//         let v2 = tx.create_vertex("StatNode".to_string(), vec![]).unwrap();
//         tx.create_edge("STAT_EDGE".to_string(), v1.id, v2.id, vec![])
//             .unwrap();
//         tx.commit().unwrap();

//         let new_stats = graph.get_statistics().unwrap();

// let expected_vertex_count = initial_stats.vertex_count.unwrap_or(0) + 2;
// let expected_edge_count = initial_stats.edge_count.unwrap_or(0) + 1;

// if new_stats.vertex_count != Some(expected_vertex_count)
//     || new_stats.edge_count != Some(expected_edge_count)
// {
//     println!(
//         "[WARN] Statistics did not update immediately. Expected (V: {}, E: {}), got (V: {:?}, E: {:?})",
//         expected_vertex_count, expected_edge_count,
//         new_stats.vertex_count, new_stats.edge_count
//     );
//     std::thread::sleep(std::time::Duration::from_millis(500)); // Add delay
//     let retry_stats = graph.get_statistics().unwrap();

//     assert_eq!(
//         retry_stats.vertex_count,
//         Some(expected_vertex_count),
//         "Vertex count did not update after retry"
//     );
//     assert_eq!(
//         retry_stats.edge_count,
//         Some(expected_edge_count),
//         "Edge count did not update after retry"
//     );
// }

//     }
// }
