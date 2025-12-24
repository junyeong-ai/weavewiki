use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub path: String,
    pub name: String,
    pub metadata: NodeMetadata,
    pub evidence: EvidenceLocation,
    pub tier: InformationTier,
    pub confidence: f32,
    pub last_verified: DateTime<Utc>,
    pub status: NodeStatus,
}

impl Node {
    /// Creates a new Node with required fields and sensible defaults
    pub fn new(node_type: NodeType, path: String, name: String) -> Self {
        let node_type_str = match node_type {
            NodeType::Module => "module",
            NodeType::File => "file",
            NodeType::Function => "function",
            NodeType::Method => "method",
            NodeType::Class => "class",
            NodeType::Interface => "interface",
            NodeType::Type => "type",
            NodeType::Enum => "enum",
            NodeType::Api => "api",
            NodeType::Entity => "entity",
            NodeType::Component => "component",
            NodeType::Route => "route",
            NodeType::Config => "config",
        };

        Self {
            id: format!("{}:{}", node_type_str, name),
            node_type,
            path,
            name,
            metadata: NodeMetadata::default(),
            evidence: EvidenceLocation::empty(),
            tier: InformationTier::Fact,
            confidence: 1.0,
            last_verified: Utc::now(),
            status: NodeStatus::Verified,
        }
    }

    /// Sets the metadata for this node
    pub fn with_metadata(mut self, metadata: NodeMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Sets the evidence location for this node
    pub fn with_evidence(mut self, evidence: EvidenceLocation) -> Self {
        self.evidence = evidence;
        self
    }

    /// Sets the information tier for this node
    pub fn with_tier(mut self, tier: InformationTier) -> Self {
        self.tier = tier;
        self
    }

    /// Sets the confidence score for this node
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    /// Sets the status for this node
    pub fn with_status(mut self, status: NodeStatus) -> Self {
        self.status = status;
        self
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    Module,
    File,
    Function,
    Method,
    Class,
    Interface,
    Type,
    Enum,
    Api,
    Entity,
    Component,
    Route,
    Config,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeMetadata {
    pub description: Option<String>,
    pub visibility: Option<Visibility>,
    pub signature: Option<FunctionSignature>,
    pub extends: Option<String>,
    pub implements: Option<Vec<String>>,
    pub api_metadata: Option<ApiMetadata>,
    pub component_metadata: Option<ComponentMetadata>,
    pub entity_metadata: Option<EntityMetadata>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Private,
    Protected,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionSignature {
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    #[serde(rename = "async")]
    pub is_async: bool,
    pub generator: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: Option<String>,
    pub optional: bool,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMetadata {
    pub method: HttpMethod,
    pub path: String,
    pub request_schema: Option<SchemaReference>,
    pub response_schema: Option<SchemaReference>,
    pub auth: Option<AuthRequirement>,
    pub rate_limit: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Head,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRequirement {
    pub required: bool,
    #[serde(rename = "type")]
    pub auth_type: Option<String>,
    pub roles: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentMetadata {
    pub props: Option<Vec<PropDefinition>>,
    pub state: Option<StateDefinition>,
    pub hooks: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropDefinition {
    pub name: String,
    #[serde(rename = "type")]
    pub prop_type: String,
    pub required: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDefinition {
    pub fields: Vec<FieldDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityMetadata {
    pub table_name: Option<String>,
    pub fields: Option<Vec<FieldDefinition>>,
    pub relations: Option<Vec<RelationDefinition>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDefinition {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub nullable: bool,
    pub primary_key: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationDefinition {
    pub name: String,
    pub relation_type: String,
    pub target: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InformationTier {
    Fact,
    Inference,
    Interpretation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    Verified,
    Stale,
    Conflict,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceLocation {
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub start_column: Option<u32>,
    pub end_column: Option<u32>,
}

impl EvidenceLocation {
    pub fn empty() -> Self {
        Self {
            file: String::new(),
            start_line: 0,
            end_line: 0,
            start_column: None,
            end_column: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SchemaReference {
    Inline { schema: serde_json::Value },
    Ref { ref_path: String },
}
