use crate::{helpers, GraphArangoDbComponent, SchemaManager};
use golem_graph::LOGGING_STATE;
use golem_graph::{
    durability::ExtendedGuest,
    golem::graph::{
        errors::GraphError,
        schema::{
            ContainerInfo, ContainerType, EdgeLabelSchema, EdgeTypeDefinition,
            Guest as SchemaGuest, GuestSchemaManager, IndexDefinition,
            SchemaManager as SchemaManagerResource, VertexLabelSchema,
        },
    },
};
use std::sync::Arc;

impl SchemaGuest for GraphArangoDbComponent {
    type SchemaManager = SchemaManager;

    fn get_schema_manager() -> Result<golem_graph::golem::graph::schema::SchemaManager, GraphError>
    {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        let config = helpers::config_from_env()?;

        let graph = GraphArangoDbComponent::connect_internal(&config)?;

        let manager = SchemaManager {
            graph: Arc::new(graph),
        };

        Ok(SchemaManagerResource::new(manager))
    }
}

impl GuestSchemaManager for SchemaManager {
    fn define_vertex_label(&self, schema: VertexLabelSchema) -> Result<(), GraphError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        self.create_container(schema.label, ContainerType::VertexContainer)
    }

    fn define_edge_label(&self, schema: EdgeLabelSchema) -> Result<(), GraphError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        self.create_container(schema.label, ContainerType::EdgeContainer)
    }

    fn get_vertex_label_schema(
        &self,
        _label: String,
    ) -> Result<Option<VertexLabelSchema>, GraphError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        Err(GraphError::UnsupportedOperation(
            "get_vertex_label_schema is not yet supported".to_string(),
        ))
    }

    fn get_edge_label_schema(&self, _label: String) -> Result<Option<EdgeLabelSchema>, GraphError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        Err(GraphError::UnsupportedOperation(
            "get_edge_label_schema is not yet supported".to_string(),
        ))
    }

    fn list_vertex_labels(&self) -> Result<Vec<String>, GraphError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        let all_containers = self.list_containers()?;
        Ok(all_containers
            .into_iter()
            .filter(|c| matches!(c.container_type, ContainerType::VertexContainer))
            .map(|c| c.name)
            .collect())
    }

    fn list_edge_labels(&self) -> Result<Vec<String>, GraphError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        let all_containers = self.list_containers()?;
        Ok(all_containers
            .into_iter()
            .filter(|c| matches!(c.container_type, ContainerType::EdgeContainer))
            .map(|c| c.name)
            .collect())
    }

    fn create_index(&self, index: IndexDefinition) -> Result<(), GraphError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        self.graph.api.create_index(
            index.label,
            index.properties,
            index.unique,
            index.index_type,
            Some(index.name),
        )
    }

    fn drop_index(&self, name: String) -> Result<(), GraphError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        self.graph.api.drop_index(&name)
    }

    fn list_indexes(&self) -> Result<Vec<IndexDefinition>, GraphError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        self.graph.api.list_indexes()
    }

    fn get_index(&self, name: String) -> Result<Option<IndexDefinition>, GraphError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        self.graph.api.get_index(&name)
    }

    fn define_edge_type(&self, definition: EdgeTypeDefinition) -> Result<(), GraphError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        self.graph.api.define_edge_type(definition)
    }

    fn list_edge_types(&self) -> Result<Vec<EdgeTypeDefinition>, GraphError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        self.graph.api.list_edge_types()
    }

    fn create_container(
        &self,
        name: String,
        container_type: ContainerType,
    ) -> Result<(), GraphError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        self.graph.api.create_collection(&name, container_type)
    }

    fn list_containers(&self) -> Result<Vec<ContainerInfo>, GraphError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());
        self.graph.api.list_collections()
    }
}
