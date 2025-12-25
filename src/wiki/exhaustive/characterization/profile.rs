//! ProjectProfile - Unified project characteristics

use crate::config::{AnalysisMode, ProjectScale};
use crate::types::DomainTerm;
use crate::wiki::exhaustive::types::Importance;
use serde::{Deserialize, Serialize};

/// Unified project profile synthesized from all characterization agents
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectProfile {
    /// Project name
    pub name: String,
    /// Detected scale
    pub scale: ProjectScale,
    /// Analysis mode
    pub mode: AnalysisMode,

    // === Purposes ===
    /// Discovered project purposes (e.g., "CLI Tool", "API Server", "Library")
    pub purposes: Vec<String>,
    /// Target users
    pub target_users: Vec<String>,

    // === Technical Characteristics ===
    /// Technical traits (e.g., "Async", "Event-driven", "Multi-tenant")
    pub technical_traits: Vec<String>,
    /// Architecture patterns (e.g., "Hexagonal", "DDD", "CQRS")
    pub architecture_hints: Vec<String>,
    /// Organization style
    pub organization_style: OrganizationStyle,

    // === Domain Characteristics ===
    /// Domain traits (e.g., "Logistics", "Financial", "Code analysis")
    pub domain_traits: Vec<String>,
    /// Domain terminology
    pub terminology: Vec<DomainTerm>,

    // === Dynamic Domain Sections ===
    /// AI-discovered domain-specific sections to extract (e.g., "Payment Flow", "Delivery States")
    #[serde(default)]
    pub dynamic_sections: Vec<DynamicSection>,

    // === Analysis Guidance ===
    /// Key areas for focused analysis
    pub key_areas: Vec<KeyArea>,
    /// Entry points for top-down analysis
    pub entry_points: Vec<EntryPoint>,

    // === Metadata ===
    /// Characterization timestamp
    pub characterized_at: String,
    /// Turn count used for characterization
    pub characterization_turns: u8,
}

impl ProjectProfile {
    pub fn new(name: String, scale: ProjectScale, mode: AnalysisMode) -> Self {
        Self {
            name,
            scale,
            mode,
            purposes: vec!["Unknown".to_string()],
            target_users: vec![],
            technical_traits: vec![],
            architecture_hints: vec![],
            organization_style: OrganizationStyle::default(),
            domain_traits: vec![],
            terminology: vec![],
            dynamic_sections: vec![],
            key_areas: vec![],
            entry_points: vec![],
            characterized_at: chrono::Utc::now().to_rfc3339(),
            characterization_turns: 0,
        }
    }

    /// Validate profile completeness (structural validation only)
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = vec![];

        if self.purposes.is_empty() {
            errors.push("purposes must have at least 1 entry".to_string());
        }

        // Check for duplicate terminology
        let mut terms: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for term in &self.terminology {
            if !terms.insert(&term.term) {
                errors.push(format!("Duplicate terminology term: {}", term.term));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validate profile against actual project paths
    pub fn validate_paths(&self, project_root: &std::path::Path) -> Result<(), Vec<String>> {
        let mut warnings = vec![];

        // Validate key area paths exist
        for area in &self.key_areas {
            let path = project_root.join(&area.path);
            if !path.exists() {
                warnings.push(format!("Key area path does not exist: {}", area.path));
            }
        }

        // Validate entry point paths exist
        for entry in &self.entry_points {
            // Parse file:line format
            let file_path = entry.file.split(':').next().unwrap_or(&entry.file);
            let path = project_root.join(file_path);
            if !path.exists() {
                warnings.push(format!("Entry point file does not exist: {}", file_path));
            }
        }

        if warnings.is_empty() {
            Ok(())
        } else {
            Err(warnings)
        }
    }

    /// Check if profile is sufficiently complete for analysis
    pub fn is_complete(&self) -> bool {
        !self.purposes.is_empty()
            && !self.purposes.iter().all(|p| p == "Unknown")
            && self.characterization_turns > 0
    }

    /// Get a summary string for logging
    pub fn summary(&self) -> String {
        format!(
            "ProjectProfile{{ name: {}, scale: {}, purposes: {:?}, traits: {} technical, {} domain }}",
            self.name,
            self.scale,
            self.purposes,
            self.technical_traits.len(),
            self.domain_traits.len()
        )
    }
}

/// Organization style classification
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OrganizationStyle {
    /// Domain-driven design structure
    DomainDriven,
    /// Layer-based (controllers, services, repositories)
    LayerBased,
    /// Feature-based organization
    FeatureBased,
    /// Flat directory structure
    Flat,
    /// Mixed organization
    #[default]
    Hybrid,
}

// DomainTerm is now imported from crate::types::DomainTerm

/// Key area for focused analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyArea {
    pub path: String,
    pub importance: Importance,
    pub focus_reasons: Vec<String>,
}

/// Entry point for top-down analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPoint {
    /// Type: main, api, handler, export, etc.
    pub entry_type: String,
    /// File path
    pub file: String,
    /// Function/symbol name
    pub symbol: Option<String>,
}

/// AI-discovered domain-specific section for extraction
///
/// These sections are dynamically determined based on project domain analysis.
/// Examples: "Payment Flow" for e-commerce, "Delivery States" for logistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicSection {
    /// Section name (e.g., "Payment Flow", "Delivery State Transitions")
    pub name: String,
    /// What this section covers and why it's important
    pub description: String,
    /// Content type hint for structured extraction
    pub content_type: SectionContentType,
    /// Hints for AI about what to look for in code
    pub extraction_hints: Vec<String>,
    /// Importance level for prioritization
    pub importance: Importance,
    /// File patterns where this section is likely found (e.g., "**/payment/**")
    #[serde(default)]
    pub file_patterns: Vec<String>,
}

/// Content type for dynamic sections - guides extraction format
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SectionContentType {
    /// State transitions (e.g., order status, delivery states)
    StateTransitions,
    /// Multi-step flow or process (e.g., checkout flow, auth flow)
    Flow,
    /// Business rules and constraints (e.g., dispatch rules, pricing rules)
    Rules,
    /// Data transformations (e.g., data pipeline stages)
    DataTransform,
    /// API contracts and interfaces
    ApiContract,
    /// Configuration and settings
    Configuration,
    /// Free-form insights (AI decides structure)
    #[default]
    Freeform,
}
