#[allow(static_mut_refs)]
mod bindings;

use golem_rust::atomically;
use crate::bindings::exports::test::graph_exports::test_graph_api::*;
use crate::bindings::golem::graph::graph;
use crate::bindings::test::helper_client::test_helper_client::TestHelperApi;

struct Component;

// Configuration constants for different graph database providers
#[cfg(feature = "neo4j")]
const PROVIDER: &'static str = "neo4j";
#[cfg(feature = "arangodb")]
const PROVIDER: &'static str = "arangodb";
#[cfg(feature = "janusgraph")]
const PROVIDER: &'static str = "janusgraph";

// Test configuration
const TEST_HOST: &'static str = "localhost";
const TEST_DATABASE: &'static str = "test_graph";

impl Guest for Component {
    /// test1 demonstrates basic vertex creation and retrieval operations
    fn test1() -> String {
        println!("Starting test1: Basic vertex operations with {}", PROVIDER);
        
        let config = graph::ConnectionConfig {
            hosts: vec![TEST_HOST.to_string()],
            port: None,
            database_name: Some(TEST_DATABASE.to_string()),
            username: Some("test".to_string()),
            password: Some("test".to_string()),
            timeout_seconds: Some(30),
            max_connections: Some(5),
            provider_config: vec![],
        };

        println!("Connecting to graph database...");
        let graph_connection = match graph::connect(&config) {
            Ok(conn) => conn,
            Err(error) => return format!("Connection failed: {:?}", error),
        };

        println!("Beginning transaction...");
        let transaction = match graph_connection.begin_transaction() {
            Ok(tx) => tx,
            Err(error) => return format!("Transaction creation failed: {:?}", error),
        };

        // Create a test vertex
        let properties = vec![
            ("name".to_string(), graph::PropertyValue::StringValue("Alice".to_string())),
            ("age".to_string(), graph::PropertyValue::Int32(30)),
            ("active".to_string(), graph::PropertyValue::Boolean(true)),
        ];

        println!("Creating vertex...");
        let vertex = match transaction.create_vertex("Person", properties) {
            Ok(v) => v,
            Err(error) => return format!("Vertex creation failed: {:?}", error),
        };

        println!("Created vertex with ID: {:?}", vertex.id);

        // Retrieve the vertex by ID
        let retrieved_vertex = match transaction.get_vertex(vertex.id.clone()) {
            Ok(Some(v)) => v,
            Ok(None) => return "Vertex not found after creation".to_string(),
            Err(error) => return format!("Vertex retrieval failed: {:?}", error),
        };

        // Commit transaction
        match transaction.commit() {
            Ok(_) => println!("Transaction committed successfully"),
            Err(error) => return format!("Commit failed: {:?}", error),
        };

        // Close connection
        let _ = graph_connection.close();

        format!(
            "SUCCESS: Created and retrieved vertex of type '{}' with ID {:?} and {} properties",
            retrieved_vertex.vertex_type,
            retrieved_vertex.id,
            retrieved_vertex.properties.len()
        )
    }

    /// test2 demonstrates edge creation and relationship operations
    fn test2() -> String {
        println!("Starting test2: Edge operations with {}", PROVIDER);
        
        let config = graph::ConnectionConfig {
            hosts: vec![TEST_HOST.to_string()],
            port: None,
            database_name: Some(TEST_DATABASE.to_string()),
            username: Some("test".to_string()),
            password: Some("test".to_string()),
            timeout_seconds: Some(30),
            max_connections: Some(5),
            provider_config: vec![],
        };

        let graph_connection = match graph::connect(&config) {
            Ok(conn) => conn,
            Err(error) => return format!("Connection failed: {:?}", error),
        };

        let transaction = match graph_connection.begin_transaction() {
            Ok(tx) => tx,
            Err(error) => return format!("Transaction creation failed: {:?}", error),
        };

        // Create two vertices
        let person1_props = vec![
            ("name".to_string(), graph::PropertyValue::StringValue("Bob".to_string())),
            ("age".to_string(), graph::PropertyValue::Int32(25)),
        ];

        let person2_props = vec![
            ("name".to_string(), graph::PropertyValue::StringValue("Carol".to_string())),
            ("age".to_string(), graph::PropertyValue::Int32(28)),
        ];

        let vertex1 = match transaction.create_vertex("Person", person1_props) {
            Ok(v) => v,
            Err(error) => return format!("First vertex creation failed: {:?}", error),
        };

        let vertex2 = match transaction.create_vertex("Person", person2_props) {
            Ok(v) => v,
            Err(error) => return format!("Second vertex creation failed: {:?}", error),
        };

        // Create an edge between them
        let edge_props = vec![
            ("relationship".to_string(), graph::PropertyValue::StringValue("FRIEND".to_string())),
            ("since".to_string(), graph::PropertyValue::StringValue("2020-01-01".to_string())),
            ("weight".to_string(), graph::PropertyValue::Float32(0.8)),
        ];

        let edge = match transaction.create_edge(
            "KNOWS",
            vertex1.id.clone(),
            vertex2.id.clone(),
            edge_props,
        ) {
            Ok(e) => e,
            Err(error) => return format!("Edge creation failed: {:?}", error),
        };

        // Retrieve adjacent vertices
        let adjacent_vertices = match transaction.get_adjacent_vertices(
            vertex1.id.clone(),
            graph::Direction::Outgoing,
            Some(vec!["KNOWS".to_string()]),
            Some(10),
        ) {
            Ok(vertices) => vertices,
            Err(error) => return format!("Adjacent vertices retrieval failed: {:?}", error),
        };

        match transaction.commit() {
            Ok(_) => (),
            Err(error) => return format!("Commit failed: {:?}", error),
        };

        let _ = graph_connection.close();

        format!(
            "SUCCESS: Created edge of type '{}' between vertices. Found {} adjacent vertices",
            edge.edge_type,
            adjacent_vertices.len()
        )
    }

    /// test3 demonstrates transaction rollback and error handling
    fn test3() -> String {
        println!("Starting test3: Transaction operations with {}", PROVIDER);
        
        let config = graph::ConnectionConfig {
            hosts: vec![TEST_HOST.to_string()],
            port: None,
            database_name: Some(TEST_DATABASE.to_string()),
            username: Some("test".to_string()),
            password: Some("test".to_string()),
            timeout_seconds: Some(30),
            max_connections: Some(5),
            provider_config: vec![],
        };

        let graph_connection = match graph::connect(&config) {
            Ok(conn) => conn,
            Err(error) => return format!("Connection failed: {:?}", error),
        };

        let transaction = match graph_connection.begin_transaction() {
            Ok(tx) => tx,
            Err(error) => return format!("Transaction creation failed: {:?}", error),
        };

        // Create a vertex
        let properties = vec![
            ("name".to_string(), graph::PropertyValue::StringValue("TestUser".to_string())),
            ("temp".to_string(), graph::PropertyValue::Boolean(true)),
        ];

        let vertex = match transaction.create_vertex("TempUser", properties) {
            Ok(v) => v,
            Err(error) => return format!("Vertex creation failed: {:?}", error),
        };

        // Check if transaction is active
        let is_active_before = transaction.is_active();
        
        // Intentionally rollback the transaction
        match transaction.rollback() {
            Ok(_) => println!("Transaction rolled back successfully"),
            Err(error) => return format!("Rollback failed: {:?}", error),
        };

        let is_active_after = transaction.is_active();

        let _ = graph_connection.close();

        format!(
            "SUCCESS: Transaction test completed. Active before rollback: {}, after rollback: {}. Vertex ID was: {:?}",
            is_active_before,
            is_active_after,
            vertex.id
        )
    }

    /// test4 demonstrates batch operations for creating multiple vertices and edges
    fn test4() -> String {
        println!("Starting test4: Batch operations with {}", PROVIDER);
        
        let config = graph::ConnectionConfig {
            hosts: vec![TEST_HOST.to_string()],
            port: None,
            database_name: Some(TEST_DATABASE.to_string()),
            username: Some("test".to_string()),
            password: Some("test".to_string()),
            timeout_seconds: Some(30),
            max_connections: Some(5),
            provider_config: vec![],
        };

        let graph_connection = match graph::connect(&config) {
            Ok(conn) => conn,
            Err(error) => return format!("Connection failed: {:?}", error),
        };

        let transaction = match graph_connection.begin_transaction() {
            Ok(tx) => tx,
            Err(error) => return format!("Transaction creation failed: {:?}", error),
        };

        // Create multiple vertices in a batch
        let vertex_specs = vec![
            graph::VertexSpec {
                vertex_type: "Company".to_string(),
                additional_labels: None,
                properties: vec![
                    ("name".to_string(), graph::PropertyValue::StringValue("TechCorp".to_string())),
                    ("founded".to_string(), graph::PropertyValue::Int32(2010)),
                ],
            },
            graph::VertexSpec {
                vertex_type: "Company".to_string(),
                additional_labels: None,
                properties: vec![
                    ("name".to_string(), graph::PropertyValue::StringValue("DataInc".to_string())),
                    ("founded".to_string(), graph::PropertyValue::Int32(2015)),
                ],
            },
            graph::VertexSpec {
                vertex_type: "Employee".to_string(),
                additional_labels: Some(vec!["Person".to_string()]),
                properties: vec![
                    ("name".to_string(), graph::PropertyValue::StringValue("John".to_string())),
                    ("role".to_string(), graph::PropertyValue::StringValue("Developer".to_string())),
                ],
            },
        ];

        let vertices = match transaction.create_vertices(vertex_specs) {
            Ok(v) => v,
            Err(error) => return format!("Batch vertex creation failed: {:?}", error),
        };

        // Create edges between the vertices
        if vertices.len() >= 3 {
            let edge_specs = vec![
                graph::EdgeSpec {
                    edge_type: "WORKS_FOR".to_string(),
                    from_vertex: vertices[2].id.clone(), // Employee
                    to_vertex: vertices[0].id.clone(), // TechCorp
                    properties: vec![
                        ("start_date".to_string(), graph::PropertyValue::StringValue("2022-01-01".to_string())),
                        ("position".to_string(), graph::PropertyValue::StringValue("Senior Developer".to_string())),
                    ],
                },
            ];

            let edges = match transaction.create_edges(edge_specs) {
                Ok(e) => e,
                Err(error) => return format!("Batch edge creation failed: {:?}", error),
            };

            match transaction.commit() {
                Ok(_) => (),
                Err(error) => return format!("Commit failed: {:?}", error),
            };

            let _ = graph_connection.close();

            format!(
                "SUCCESS: Created {} vertices and {} edges in batch operations",
                vertices.len(),
                edges.len()
            )
        } else {
            format!("ERROR: Expected at least 3 vertices, got {}", vertices.len())
        }
    }

    /// test5 demonstrates graph traversal and pathfinding operations
    fn test5() -> String {
        println!("Starting test5: Traversal operations with {}", PROVIDER);
        
        let config = graph::ConnectionConfig {
            hosts: vec![TEST_HOST.to_string()],
            port: None,
            database_name: Some(TEST_DATABASE.to_string()),
            username: Some("test".to_string()),
            password: Some("test".to_string()),
            timeout_seconds: Some(30),
            max_connections: Some(5),
            provider_config: vec![],
        };

        let graph_connection = match graph::connect(&config) {
            Ok(conn) => conn,
            Err(error) => return format!("Connection failed: {:?}", error),
        };

        let transaction = match graph_connection.begin_transaction() {
            Ok(tx) => tx,
            Err(error) => return format!("Transaction creation failed: {:?}", error),
        };

        // Create a small network: A -> B -> C
        let vertex_a = match transaction.create_vertex("Node", vec![
            ("name".to_string(), graph::PropertyValue::StringValue("A".to_string())),
        ]) {
            Ok(v) => v,
            Err(error) => return format!("Vertex A creation failed: {:?}", error),
        };

        let vertex_b = match transaction.create_vertex("Node", vec![
            ("name".to_string(), graph::PropertyValue::StringValue("B".to_string())),
        ]) {
            Ok(v) => v,
            Err(error) => return format!("Vertex B creation failed: {:?}", error),
        };

        let vertex_c = match transaction.create_vertex("Node", vec![
            ("name".to_string(), graph::PropertyValue::StringValue("C".to_string())),
        ]) {
            Ok(v) => v,
            Err(error) => return format!("Vertex C creation failed: {:?}", error),
        };

        // Create edges
        let _ = transaction.create_edge("CONNECTS", vertex_a.id.clone(), vertex_b.id.clone(), vec![]);
        let _ = transaction.create_edge("CONNECTS", vertex_b.id.clone(), vertex_c.id.clone(), vec![]);

        // Test neighborhood exploration
        let neighborhood = match graph::get_neighborhood(
            &transaction,
            vertex_b.id.clone(),
            graph::NeighborhoodOptions {
                depth: 1,
                direction: graph::Direction::Both,
                edge_types: Some(vec!["CONNECTS".to_string()]),
                max_vertices: Some(10),
            },
        ) {
            Ok(subgraph) => subgraph,
            Err(error) => return format!("Neighborhood exploration failed: {:?}", error),
        };

        // Test pathfinding
        let path_exists = match graph::path_exists(
            &transaction,
            vertex_a.id.clone(),
            vertex_c.id.clone(),
            Some(graph::PathOptions {
                max_depth: Some(3),
                edge_types: Some(vec!["CONNECTS".to_string()]),
                vertex_types: None,
                vertex_filters: None,
                edge_filters: None,
            }),
        ) {
            Ok(exists) => exists,
            Err(error) => return format!("Path existence check failed: {:?}", error),
        };

        match transaction.commit() {
            Ok(_) => (),
            Err(error) => return format!("Commit failed: {:?}", error),
        };

        let _ = graph_connection.close();

        format!(
            "SUCCESS: Traversal test completed. Neighborhood has {} vertices and {} edges. Path from A to C exists: {}",
            neighborhood.vertices.len(),
            neighborhood.edges.len(),
            path_exists
        )
    }

    /// test6 demonstrates query operations using database-specific query languages
    fn test6() -> String {
        println!("Starting test6: Query operations with {}", PROVIDER);
        
        let config = graph::ConnectionConfig {
            hosts: vec![TEST_HOST.to_string()],
            port: None,
            database_name: Some(TEST_DATABASE.to_string()),
            username: Some("test".to_string()),
            password: Some("test".to_string()),
            timeout_seconds: Some(30),
            max_connections: Some(5),
            provider_config: vec![],
        };

        let graph_connection = match graph::connect(&config) {
            Ok(conn) => conn,
            Err(error) => return format!("Connection failed: {:?}", error),
        };

        let transaction = match graph_connection.begin_transaction() {
            Ok(tx) => tx,
            Err(error) => return format!("Transaction creation failed: {:?}", error),
        };

        // Create some test data first
        let _ = transaction.create_vertex("Product", vec![
            ("name".to_string(), graph::PropertyValue::StringValue("Widget".to_string())),
            ("price".to_string(), graph::PropertyValue::Float32(19.99)),
        ]);

        let _ = transaction.create_vertex("Product", vec![
            ("name".to_string(), graph::PropertyValue::StringValue("Gadget".to_string())),
            ("price".to_string(), graph::PropertyValue::Float32(29.99)),
        ]);

        // Execute a provider-specific query
        let query_string = match PROVIDER {
            "neo4j" => "MATCH (p:Product) WHERE p.price > $min_price RETURN p",
            "arangodb" => "FOR p IN Product FILTER p.price > @min_price RETURN p",
            "janusgraph" => "g.V().hasLabel('Product').has('price', gt(min_price))",
            _ => "SELECT * FROM Product WHERE price > ?",
        };

        let parameters = vec![
            ("min_price".to_string(), graph::PropertyValue::Float32(15.0)),
        ];

        let query_result = match graph::execute_query(
            &transaction,
            query_string.to_string(),
            Some(parameters),
            Some(graph::QueryOptions {
                timeout_seconds: Some(30),
                max_results: Some(100),
                explain: false,
                profile: false,
            }),
        ) {
            Ok(result) => result,
            Err(error) => return format!("Query execution failed: {:?}", error),
        };

        match transaction.commit() {
            Ok(_) => (),
            Err(error) => return format!("Commit failed: {:?}", error),
        };

        let _ = graph_connection.close();

        let result_count = match &query_result.query_result_value {
            graph::QueryResult::Vertices(vertices) => vertices.len(),
            graph::QueryResult::Maps(maps) => maps.len(),
            graph::QueryResult::Values(values) => values.len(),
            _ => 0,
        };

        format!(
            "SUCCESS: Query executed successfully with {} provider. Found {} results. Execution time: {:?}ms",
            PROVIDER,
            result_count,
            query_result.execution_time_ms
        )
    }

    /// test7 demonstrates schema management operations
    fn test7() -> String {
        println!("Starting test7: Schema operations with {}", PROVIDER);
        
        let schema_manager = match graph::get_schema_manager() {
            Ok(manager) => manager,
            Err(error) => return format!("Schema manager creation failed: {:?}", error),
        };

        // Define a vertex label schema
        let user_schema = graph::VertexLabelSchema {
            label: "User".to_string(),
            properties: vec![
                graph::PropertyDefinition {
                    name: "username".to_string(),
                    property_type: graph::PropertyType::StringType,
                    required: true,
                    unique: true,
                    default_value: None,
                },
                graph::PropertyDefinition {
                    name: "email".to_string(),
                    property_type: graph::PropertyType::StringType,
                    required: true,
                    unique: true,
                    default_value: None,
                },
                graph::PropertyDefinition {
                    name: "age".to_string(),
                    property_type: graph::PropertyType::Int32,
                    required: false,
                    unique: false,
                    default_value: Some(graph::PropertyValue::Int32(0)),
                },
            ],
            container: None,
        };

        match schema_manager.define_vertex_label(user_schema) {
            Ok(_) => println!("User vertex label schema defined successfully"),
            Err(error) => return format!("Vertex label definition failed: {:?}", error),
        };

        // Define an edge label schema
        let follows_schema = graph::EdgeLabelSchema {
            label: "FOLLOWS".to_string(),
            properties: vec![
                graph::PropertyDefinition {
                    name: "since".to_string(),
                    property_type: graph::PropertyType::StringType,
                    required: false,
                    unique: false,
                    default_value: None,
                },
                graph::PropertyDefinition {
                    name: "weight".to_string(),
                    property_type: graph::PropertyType::Float32,
                    required: false,
                    unique: false,
                    default_value: Some(graph::PropertyValue::Float32(1.0)),
                },
            ],
            from_labels: Some(vec!["User".to_string()]),
            to_labels: Some(vec!["User".to_string()]),
            container: None,
        };

        match schema_manager.define_edge_label(follows_schema) {
            Ok(_) => println!("FOLLOWS edge label schema defined successfully"),
            Err(error) => return format!("Edge label definition failed: {:?}", error),
        };

        // Create an index
        let index_def = graph::IndexDefinition {
            name: "user_username_idx".to_string(),
            label: "User".to_string(),
            properties: vec!["username".to_string()],
            index_type: graph::IndexType::Exact,
            unique: true,
            container: None,
        };

        match schema_manager.create_index(index_def) {
            Ok(_) => println!("Index created successfully"),
            Err(error) => return format!("Index creation failed: {:?}", error),
        };

        // List vertex labels
        let vertex_labels = match schema_manager.list_vertex_labels() {
            Ok(labels) => labels,
            Err(error) => return format!("Listing vertex labels failed: {:?}", error),
        };

        // List edge labels
        let edge_labels = match schema_manager.list_edge_labels() {
            Ok(labels) => labels,
            Err(error) => return format!("Listing edge labels failed: {:?}", error),
        };

        // List indexes
        let indexes = match schema_manager.list_indexes() {
            Ok(idx_list) => idx_list,
            Err(error) => return format!("Listing indexes failed: {:?}", error),
        };

        format!(
            "SUCCESS: Schema operations completed with {} provider. Found {} vertex labels, {} edge labels, and {} indexes",
            PROVIDER,
            vertex_labels.len(),
            edge_labels.len(),
            indexes.len()
        )
    }
}

bindings::export!(Component with_types_in bindings);

