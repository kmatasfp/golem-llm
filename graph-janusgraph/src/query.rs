use crate::conversions;
use crate::{GraphJanusGraphComponent, Transaction};
use golem_graph::golem::graph::types::PropertyValue;
use golem_graph::golem::graph::{
    errors::GraphError,
    query::{Guest as QueryGuest, QueryExecutionResult, QueryParameters, QueryResult},
};
use serde_json::{json, Map, Value};

fn to_bindings(parameters: QueryParameters) -> Result<Map<String, Value>, GraphError> {
    let mut bindings = Map::new();
    for (key, value) in parameters {
        bindings.insert(key, conversions::to_json_value(value)?);
    }
    Ok(bindings)
}

fn parse_gremlin_response(response: Value) -> Result<QueryResult, GraphError> {
    let result_data = response
        .get("result")
        .and_then(|r| r.get("data"))
        .ok_or_else(|| {
            GraphError::InternalError("Invalid response structure from Gremlin".to_string())
        })?;

    // Handle GraphSON format: {"@type": "g:List", "@value": [...]}
    let arr = if let Some(graphson_obj) = result_data.as_object() {
        if let Some(value_array) = graphson_obj.get("@value").and_then(|v| v.as_array()) {
            value_array
        } else {
            return Ok(QueryResult::Values(vec![]));
        }
    } else if let Some(direct_array) = result_data.as_array() {
        direct_array
    } else {
        return Ok(QueryResult::Values(vec![]));
    };

    if arr.is_empty() {
        return Ok(QueryResult::Values(vec![]));
    }

    if let Some(first_item) = arr.first() {
        if first_item.is_object() {
            if let Some(obj) = first_item.as_object() {
                // Check if this is a GraphSON Map
                if obj.get("@type") == Some(&Value::String("g:Map".to_string())) {
                    // Handle GraphSON Maps
                    let mut maps = Vec::new();
                    for item in arr {
                        if let Some(obj) = item.as_object() {
                            if let Some(map_array) = obj.get("@value").and_then(|v| v.as_array()) {
                                let mut row: Vec<(String, PropertyValue)> = Vec::new();
                                // Process GraphSON Map: array contains alternating keys and values
                                let mut i = 0;
                                while i + 1 < map_array.len() {
                                    if let (Some(key_val), Some(value_val)) = (map_array.get(i), map_array.get(i + 1)) {
                                        if let Some(key_str) = key_val.as_str() {
                                            // Handle GraphSON List format for valueMap results
                                            if let Some(graphson_obj) = value_val.as_object() {
                                                if graphson_obj.get("@type") == Some(&Value::String("g:List".to_string())) {
                                                    if let Some(list_values) = graphson_obj.get("@value").and_then(|v| v.as_array()) {
                                                        if let Some(first_value) = list_values.first() {
                                                            row.push((key_str.to_string(), conversions::from_gremlin_value(first_value)?));
                                                        }
                                                    }
                                                } else {
                                                    // Regular GraphSON object
                                                    row.push((key_str.to_string(), conversions::from_gremlin_value(value_val)?));
                                                }
                                            } else {
                                                row.push((key_str.to_string(), conversions::from_gremlin_value(value_val)?));
                                            }
                                        }
                                    }
                                    i += 2;
                                }
                                maps.push(row);
                            }
                        }
                    }
                    return Ok(QueryResult::Maps(maps));
                } else if obj.contains_key("@type") && obj.contains_key("@value") {
                    // This is a GraphSON wrapped primitive value, treat as values
                    let values = arr
                        .iter()
                        .map(conversions::from_gremlin_value)
                        .collect::<Result<Vec<_>, _>>()?;
                    return Ok(QueryResult::Values(values));
                } else {
                    // Regular JSON object maps
                    let mut maps = Vec::new();
                    for item in arr {
                        if let Some(gremlin_map) = item.as_object() {
                            let mut row: Vec<(String, PropertyValue)> = Vec::new();
                            for (key, gremlin_value) in gremlin_map {
                                // Handle GraphSON List format for valueMap results
                                if let Some(graphson_obj) = gremlin_value.as_object() {
                                    if graphson_obj.get("@type") == Some(&Value::String("g:List".to_string())) {
                                        if let Some(list_values) = graphson_obj.get("@value").and_then(|v| v.as_array()) {
                                            if let Some(first_value) = list_values.first() {
                                                row.push((key.clone(), conversions::from_gremlin_value(first_value)?));
                                            }
                                        }
                                    } else {
                                        // Regular GraphSON object
                                        row.push((key.clone(), conversions::from_gremlin_value(gremlin_value)?));
                                    }
                                } else if let Some(inner_array) = gremlin_value.as_array() {
                                    if let Some(actual_value) = inner_array.first() {
                                        row.push((key.clone(), conversions::from_gremlin_value(actual_value)?));
                                    }
                                } else {
                                    row.push((key.clone(), conversions::from_gremlin_value(gremlin_value)?));
                                }
                            }
                            maps.push(row);
                        }
                    }
                    return Ok(QueryResult::Maps(maps));
                }
            }
        }
    }

    let values = arr
        .iter()
        .map(conversions::from_gremlin_value)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(QueryResult::Values(values))
}

impl Transaction {
    pub fn execute_query(
        &self,
        query: String,
        parameters: Option<QueryParameters>,
        _options: Option<golem_graph::golem::graph::query::QueryOptions>,
    ) -> Result<QueryExecutionResult, GraphError> {
        let params = parameters.unwrap_or_default();
        let bindings_map = to_bindings(params)?;

        let response = self.api.execute(&query, Some(json!(bindings_map)))?;
        let query_result_value = parse_gremlin_response(response)?;

        Ok(QueryExecutionResult {
            query_result_value,
            execution_time_ms: None,
            rows_affected: None,
            explanation: None,
            profile_data: None,
        })
    }
}

impl QueryGuest for GraphJanusGraphComponent {
    fn execute_query(
        transaction: golem_graph::golem::graph::transactions::TransactionBorrow<'_>,
        query: String,
        parameters: Option<QueryParameters>,
        options: Option<golem_graph::golem::graph::query::QueryOptions>,
    ) -> Result<QueryExecutionResult, GraphError> {
        let tx: &Transaction = transaction.get();
        tx.execute_query(query, parameters, options)
    }
}

// #[cfg(test)]
// mod tests {
//     use crate::client::JanusGraphApi;
//     use crate::Transaction;
//     use golem_graph::golem::graph::{
//         errors::GraphError,
//         query::{QueryParameters, QueryResult},
//         transactions::GuestTransaction,
//         types::PropertyValue,
//     };
//     use std::{env, sync::Arc};

//     fn create_test_transaction() -> Transaction {
//         let host = env::var("JANUSGRAPH_HOST").unwrap_or_else(|_| "localhost".to_string());
//         let port = env::var("JANUSGRAPH_PORT")
//             .unwrap_or_else(|_| "8182".to_string())
//             .parse()
//             .unwrap();
//         let api = JanusGraphApi::new(&host, port, None, None).unwrap();
//         Transaction { api: Arc::new(api) }
//     }

//     fn setup_test_data(tx: &Transaction) {
//         // Clean up any existing test data first
//         let _ = tx.execute_query("g.V().hasLabel('person').drop()".to_string(), None, None);
//         let _ = tx.execute_query("g.V().hasLabel('software').drop()".to_string(), None, None);
        
//         // Create test vertices in the same transaction
//         tx.create_vertex(
//             "person".to_string(),
//             vec![
//                 (
//                     "name".to_string(),
//                     PropertyValue::StringValue("marko".to_string()),
//                 ),
//                 ("age".to_string(), PropertyValue::Int64(29)),
//             ],
//         )
//         .unwrap();
//         tx.create_vertex(
//             "person".to_string(),
//             vec![
//                 (
//                     "name".to_string(),
//                     PropertyValue::StringValue("vadas".to_string()),
//                 ),
//                 ("age".to_string(), PropertyValue::Int64(27)),
//             ],
//         )
//         .unwrap();
//         tx.create_vertex(
//             "software".to_string(),
//             vec![
//                 (
//                     "name".to_string(),
//                     PropertyValue::StringValue("lop".to_string()),
//                 ),
//                 (
//                     "lang".to_string(),
//                     PropertyValue::StringValue("java".to_string()),
//                 ),
//             ],
//         )
//         .unwrap();
//     }

//     fn global_cleanup() {
//         let tx = create_test_transaction();
//         let _ = tx.execute_query("g.V().drop()".to_string(), None, None);
//         let _ = tx.execute_query("g.E().drop()".to_string(), None, None);
//         tx.commit().unwrap();
        
//         // Wait for cleanup to propagate
//         std::thread::sleep(std::time::Duration::from_millis(500));
//     }

//     fn cleanup_test_data_separate() {
//         let tx = create_test_transaction();
//         let _ = tx.execute_query("g.V().hasLabel('person').drop()".to_string(), None, None);
//         let _ = tx.execute_query("g.V().hasLabel('software').drop()".to_string(), None, None);
//         let _ = tx.execute_query("g.V().hasLabel('TestVertex').drop()".to_string(), None, None);
//         let _ = tx.execute_query("g.E().hasLabel('TestEdge').drop()".to_string(), None, None);
//         tx.commit().unwrap();
//     }

//     #[test]
//     fn test_simple_value_query() {
//         let tx_setup = create_test_transaction();
//         setup_test_data(&tx_setup);
//         tx_setup.commit().unwrap();
        
//         // Create a new transaction for querying
//         let tx = create_test_transaction();
        
//         let result = tx
//             .execute_query(
//                 "g.V().has('name', 'marko').values('age')".to_string(),
//                 None,
//                 None,
//             )
//             .unwrap();

//         println!("[DEBUG] Query result: {:?}", result.query_result_value);

//         match result.query_result_value {
//             QueryResult::Values(values) => {
//                 println!("[DEBUG] Values found: {:?}", values);
//                 assert!(values.iter().any(|v| v == &PropertyValue::Int64(29)), "Should find at least one marko with age 29");
//             }
//             _ => panic!("Expected Values result"),
//         }

//         cleanup_test_data_separate();
//     }

//     #[test]
//     fn test_map_query_with_params() {
//         let tx_setup = create_test_transaction();
//         setup_test_data(&tx_setup);
//         tx_setup.commit().unwrap();
        
//         // Create a new transaction for querying
//         let tx = create_test_transaction();

//         let params: QueryParameters = vec![(
//             "person_name".to_string(),
//             PropertyValue::StringValue("marko".to_string()),
//         )];
//         let result = tx
//             .execute_query(
//                 "g.V().has('name', person_name).valueMap('name', 'age')".to_string(),
//                 Some(params),
//                 None,
//             )
//             .unwrap();

//         println!("[DEBUG] valueMap query result: {:?}", result.query_result_value);

//         match result.query_result_value {
//             QueryResult::Maps(maps) => {
//                 assert_eq!(maps.len(), 1);
//                 let row = &maps[0];
//                 assert_eq!(row.len(), 2);
//                 let name = row.iter().find(|(k, _)| k == "name").unwrap();
//                 let age = row.iter().find(|(k, _)| k == "age").unwrap();
//                 assert_eq!(name.1, PropertyValue::StringValue("marko".to_string()));
//                 assert_eq!(age.1, PropertyValue::Int64(29));
//             }
//             _ => panic!("Expected Maps result, got: {:?}", result.query_result_value),
//         }

//         cleanup_test_data_separate();
//     }

//     #[test]
//     fn test_complex_query() {
//         // Clean all existing data first
//         global_cleanup();
        
//         let tx_setup = create_test_transaction();
//         setup_test_data(&tx_setup);
//         tx_setup.commit().unwrap();
        
//         // Create a new transaction for querying
//         let tx = create_test_transaction();

//         let result = tx
//             .execute_query("g.V().count()".to_string(), None, None)
//             .unwrap();

//         println!("[DEBUG] Complex query count result: {:?}", result.query_result_value);

//         match result.query_result_value {
//             QueryResult::Values(values) => {
//                 assert_eq!(values.len(), 1);
//                 assert_eq!(values[0], PropertyValue::Int64(3));
//             }
//             _ => panic!("Expected Values result"),
//         }

//         cleanup_test_data_separate();
//     }

//     #[test]
//     fn test_empty_result_query() {
//         let tx_setup = create_test_transaction();
//         setup_test_data(&tx_setup);
//         tx_setup.commit().unwrap();
        
//         // Create a new transaction for querying
//         let tx = create_test_transaction();

//         let result = tx
//             .execute_query("g.V().has('name', 'non_existent')".to_string(), None, None)
//             .unwrap();

//         match result.query_result_value {
//             QueryResult::Values(values) => {
//                 assert!(values.is_empty());
//             }
//             _ => panic!("Expected empty Values result"),
//         }

//         cleanup_test_data_separate();
//     }

//     #[test]
//     fn test_invalid_query() {
       
//         let tx = create_test_transaction();

//         let result = tx.execute_query("g.V().invalidStep()".to_string(), None, None);

//         assert!(matches!(result, Err(GraphError::InvalidQuery(_))));
//     }
// }
