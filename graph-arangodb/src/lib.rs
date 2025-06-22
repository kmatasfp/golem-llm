mod client;
mod connection;
mod conversions;
mod helpers;
mod query;
mod schema;
mod transaction;
mod traversal;

use client::ArangoDbApi;
use golem_graph::durability::{DurableGraph, ExtendedGuest};
use golem_graph::golem::graph::{
    connection::ConnectionConfig, errors::GraphError, transactions::Guest as TransactionGuest,
};
use std::sync::Arc;

pub struct GraphArangoDbComponent;

pub struct Graph {
    api: Arc<ArangoDbApi>,
}

pub struct Transaction {
    api: Arc<ArangoDbApi>,
    transaction_id: String,
}

pub struct SchemaManager {
    graph: Arc<Graph>,
}

impl ExtendedGuest for GraphArangoDbComponent {
    type Graph = Graph;
    fn connect_internal(config: &ConnectionConfig) -> Result<Graph, GraphError> {
        let host: &String = config
            .hosts
            .first()
            .ok_or_else(|| GraphError::ConnectionFailed("Missing host".to_string()))?;

        let port = config.port.unwrap_or(8529);

        let username = config
            .username
            .as_deref()
            .ok_or_else(|| GraphError::ConnectionFailed("Missing username".to_string()))?;
        let password = config
            .password
            .as_deref()
            .ok_or_else(|| GraphError::ConnectionFailed("Missing password".to_string()))?;

        let database_name = config.database_name.as_deref().unwrap_or("_system");

        let api = ArangoDbApi::new(host, port, username, password, database_name);
        Ok(Graph::new(api))
    }
}

impl TransactionGuest for GraphArangoDbComponent {
    type Transaction = Transaction;
}

impl Graph {
    fn new(api: ArangoDbApi) -> Self {
        Self { api: Arc::new(api) }
    }
}

impl Transaction {
    fn new(api: Arc<ArangoDbApi>, transaction_id: String) -> Self {
        Self {
            api,
            transaction_id,
        }
    }
}

type DurableGraphArangoDbComponent = DurableGraph<GraphArangoDbComponent>;

golem_graph::export_graph!(DurableGraphArangoDbComponent with_types_in golem_graph);

#[cfg(test)]
mod lib_tests {
    use super::*;
    use golem_graph::durability::ExtendedGuest;
    use golem_graph::golem::graph::{
        connection::ConnectionConfig, transactions::GuestTransaction, types::PropertyValue,
    };

    use std::env;

    fn get_test_config() -> ConnectionConfig {
        let host = env::var("ARANGODB_HOST").unwrap_or_else(|_| "localhost".into());
        let port = env::var("ARANGODB_PORT")
            .unwrap_or_else(|_| "8529".into())
            .parse()
            .expect("Invalid ARANGODB_PORT");
        let username = env::var("ARANGODB_USER").unwrap_or_else(|_| "root".into());
        let password = env::var("ARANGODB_PASS").unwrap_or_else(|_| "".into());
        let database_name = env::var("ARANGODB_DB").unwrap_or_else(|_| "_system".into());

        ConnectionConfig {
            hosts: vec![host],
            port: Some(port),
            username: Some(username),
            password: Some(password),
            database_name: Some(database_name),
            timeout_seconds: None,
            max_connections: None,
            provider_config: vec![],
        }
    }
    
    fn setup_test_env() {
        // Set environment variables for test, force overriding any existing values
        if let Ok(val) = env::var("ARANGO_HOST") {
            env::set_var("ARANGODB_HOST", val);
        }
        if let Ok(val) = env::var("ARANGO_PORT") {
            env::set_var("ARANGODB_PORT", val);
        }
        if let Ok(val) = env::var("ARANGO_USERNAME") {
            env::set_var("ARANGODB_USER", val);
        }
        if let Ok(val) = env::var("ARANGO_PASSWORD") {
            env::set_var("ARANGODB_PASS", val);
        }
        if let Ok(val) = env::var("ARANGO_DATABASE") {
            env::set_var("ARANGODB_DB", val);
        }
        
        // Set defaults if neither old nor new variables are set
        if env::var("ARANGODB_HOST").is_err() {
            env::set_var("ARANGODB_HOST", "localhost");
        }
        if env::var("ARANGODB_PORT").is_err() {
            env::set_var("ARANGODB_PORT", "8529");
        }
        if env::var("ARANGODB_USER").is_err() {
            env::set_var("ARANGODB_USER", "root");
        }
        if env::var("ARANGODB_PASS").is_err() {
            env::set_var("ARANGODB_PASS", "password");
        }
        if env::var("ARANGODB_DB").is_err() {
            env::set_var("ARANGODB_DB", "test");
        }
    }
    
    fn create_test_transaction() -> crate::Transaction {
        setup_test_env();
        let host = env::var("ARANGODB_HOST").unwrap_or_else(|_| "localhost".to_string());
        let port: u16 = env::var("ARANGODB_PORT")
            .unwrap_or_else(|_| "8529".to_string())
            .parse()
            .expect("Invalid ARANGODB_PORT");
        let user = env::var("ARANGODB_USER").unwrap_or_else(|_| "root".to_string());
        let pass = env::var("ARANGODB_PASS").unwrap_or_else(|_| "".to_string());        let db = env::var("ARANGODB_DB").unwrap_or_else(|_| "_system".to_string());

        let api = ArangoDbApi::new(&host, port, &user, &pass, &db);
        
        // Ensure test collection exists
        let _ = api.ensure_collection_exists("DurTest", golem_graph::golem::graph::schema::ContainerType::VertexContainer);
        
        // Begin transaction with collections declared
        let collections = vec!["DurTest".to_string()];
        let tx_id = api.begin_transaction_with_collections(false, collections).unwrap();
        crate::Transaction::new(std::sync::Arc::new(api), tx_id)
    }

    #[test]
    fn test_successful_connection() {
        setup_test_env();
        let cfg = get_test_config();
        let graph = GraphArangoDbComponent::connect_internal(&cfg);
        assert!(graph.is_ok(), "connect_internal should succeed");
    }

    #[test]
    fn test_failed_connection_bad_credentials() {
        setup_test_env();
        
        // Skip this test if running without authentication (empty password)
        if env::var("ARANGO_PASSWORD").unwrap_or_default().is_empty() && 
           env::var("ARANGODB_PASSWORD").unwrap_or_default().is_empty() {
            println!("Skipping test_failed_connection_bad_credentials: Running without authentication");
            return;
        }
        
        let mut cfg = get_test_config();
        cfg.username = Some("bad_user".into());
        cfg.password = Some("bad_pass".into());

        let api = ArangoDbApi::new(
            &cfg.hosts[0],
            cfg.port.unwrap(),
            cfg.username.as_deref().unwrap(),
            cfg.password.as_deref().unwrap(),
            cfg.database_name.as_deref().unwrap(),
        );
        assert!(api.begin_transaction(false).is_err());
    }

    #[test]
    fn test_durability_of_committed_data() {
        setup_test_env();

        let tx1 = create_test_transaction();
        let unique_id = "dur_test_123".to_string();
        let created = tx1
            .create_vertex(
                "DurTest".into(),
                vec![(
                    "test_id".into(),
                    PropertyValue::StringValue(unique_id.clone()),
                )],
            )
            .unwrap();
        tx1.commit().unwrap();

        let tx2 = create_test_transaction();
        let fetched = tx2.get_vertex(created.id.clone()).unwrap();
        assert!(fetched.is_some());

        tx2.delete_vertex(created.id, true).unwrap();
        tx2.commit().unwrap();
    }
}
