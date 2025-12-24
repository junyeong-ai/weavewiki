//! Characterization Agents
//!
//! Turn 1 Agents (parallel):
//! - StructureAgent: Directory patterns, module boundaries
//! - DependencyAgent: Internal/external dependencies
//! - EntryPointAgent: Entry points and public APIs
//!
//! Turn 2 Agents (parallel, with Turn 1 context):
//! - PurposeAgent: Project purposes and target users
//! - TechnicalAgent: Technical patterns and traits
//! - TerminologyAgent: Domain terminology and patterns
//!
//! Turn 3 Agents (with Turn 1+2 context):
//! - SectionDiscoveryAgent: Discover domain-specific sections to extract

pub mod dependency;
pub mod entry_point;
pub mod helpers;
pub mod purpose;
pub mod section_discovery;
pub mod structure;
pub mod technical;
pub mod terminology;

// Re-export helpers for convenience
pub use helpers::*;

use serde::{Deserialize, Serialize};

// Re-export agent implementations
pub use dependency::DependencyAgent;
pub use entry_point::EntryPointAgent;
pub use purpose::PurposeAgent;
pub use section_discovery::SectionDiscoveryAgent;
pub use structure::StructureAgent;
pub use technical::TechnicalAgent;
pub use terminology::TerminologyAgent;

// =============================================================================
// Turn 1 Agent Output Types
// =============================================================================

/// Output from Structure Agent (Turn 1)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureInsight {
    /// Detected directory organization patterns
    pub directory_patterns: Vec<String>,
    /// Module/package boundaries
    pub module_boundaries: Vec<ModuleBoundary>,
    /// Organization style classification
    pub organization_style: String,
    /// Naming conventions detected
    pub naming_conventions: Vec<String>,
    /// Test file organization
    pub test_organization: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleBoundary {
    pub name: String,
    pub path: String,
    pub purpose: Option<String>,
}

/// Output from Dependency Agent (Turn 1)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyInsight {
    /// Internal module dependencies
    pub internal_deps: Vec<InternalDependency>,
    /// External library dependencies
    pub external_deps: Vec<String>,
    /// Framework indicators
    pub framework_indicators: Vec<String>,
    /// Detected circular dependencies
    pub circular_deps: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalDependency {
    pub from: String,
    pub to: String,
    pub dependency_type: Option<String>,
}

/// Output from Entry Point Agent (Turn 1)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPointInsight {
    /// Application entry points
    pub entry_points: Vec<EntryPointInfo>,
    /// Public API surfaces
    pub public_surface: Vec<String>,
    /// CLI commands (if applicable)
    pub cli_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPointInfo {
    /// Type: main, api, handler, export, etc.
    pub entry_type: String,
    /// File path
    pub file: String,
    /// Function/symbol name
    pub symbol: Option<String>,
}

// =============================================================================
// Turn 2 Agent Output Types
// =============================================================================

/// Output from Purpose Agent (Turn 2)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurposeInsight {
    /// Discovered project purposes
    pub purposes: Vec<String>,
    /// Target users
    pub target_users: Vec<String>,
    /// Problems solved
    pub problems_solved: Vec<String>,
}

/// Output from Technical Agent (Turn 2)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicalInsight {
    /// Technical patterns detected
    pub technical_traits: Vec<String>,
    /// Architecture patterns
    pub architecture_patterns: Vec<String>,
    /// Quality focus areas
    pub quality_focus: Vec<String>,
    /// Async patterns detected
    pub async_patterns: Vec<String>,
}

/// Output from Terminology Agent (Turn 2)
///
/// Note: Uses unified DomainTerm from types module
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainTraitsInsight {
    /// Domain-specific traits
    pub domain_traits: Vec<String>,
    /// Domain terminology (uses unified DomainTerm type)
    pub terminology: Vec<crate::types::DomainTerm>,
    /// Domain patterns
    pub domain_patterns: Vec<String>,
}

// =============================================================================
// Turn 3 Agent Output Types
// =============================================================================

/// Output from Section Discovery Agent (Turn 3)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionDiscoveryInsight {
    /// Discovered domain-specific sections to extract
    pub sections: Vec<DiscoveredSection>,
}

/// A domain-specific section discovered by AI analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredSection {
    /// Section name (e.g., "Payment Flow", "Delivery State Transitions")
    pub name: String,
    /// What this section covers
    pub description: String,
    /// Content type for structured extraction
    pub content_type: String,
    /// Hints for extraction
    pub extraction_hints: Vec<String>,
    /// Importance: critical, high, medium, low
    pub importance: String,
    /// File patterns where this section is likely found
    #[serde(default)]
    pub file_patterns: Vec<String>,
}
