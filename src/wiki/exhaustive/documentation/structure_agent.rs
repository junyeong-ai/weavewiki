//! AI agent for discovering optimal documentation structure
//! Analyzes project characteristics to design tailored documentation hierarchy

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ai::provider::SharedProvider;
use crate::types::Result;
use crate::wiki::exhaustive::characterization::profile::ProjectProfile;
use crate::wiki::exhaustive::consolidation::DomainInsight;

use super::blueprint::{
    ContentType, DiscoveredSection, DocumentationBlueprint, DomainDocStructure,
    DomainStructureType, ProjectScale, Subsection,
};

/// Agent that discovers optimal documentation structure
pub struct DocumentationStructureAgent {
    provider: SharedProvider,
}

/// Output schema for structure discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureDiscoveryOutput {
    /// Project-specific sections to add
    pub discovered_sections: Vec<DiscoveredSectionSpec>,

    /// How to structure each domain
    pub domain_structures: Vec<DomainStructureSpec>,

    /// Recommended hierarchy depth
    pub recommended_depth: u8,

    /// Reasoning for decisions
    pub reasoning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredSectionSpec {
    pub name: String,
    pub path: String,
    pub reason: String,
    pub importance: String, // "critical", "high", "medium", "low"
    pub subsections: Vec<SubsectionSpec>,
    pub evidence_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsectionSpec {
    pub name: String,
    pub content_type: String, // "overview", "tutorial", "reference", etc.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainStructureSpec {
    pub domain_name: String,
    pub structure_type: String, // "single_page", "index_with_pages", "full_hierarchy"
    pub subsections: Vec<SubsectionSpec>,
}

impl DocumentationStructureAgent {
    pub fn new(provider: SharedProvider) -> Self {
        Self { provider }
    }

    /// Discover optimal documentation structure
    pub async fn discover(
        &self,
        profile: &ProjectProfile,
        domains: &[DomainInsight],
    ) -> Result<DocumentationBlueprint> {
        let scale = Self::determine_scale(profile);
        let mut blueprint = DocumentationBlueprint::default_for_scale(scale);

        // Build analysis context
        let context = self.build_context(profile, domains);

        // Call LLM to discover structure
        let prompt = self.build_prompt(&context, scale);
        let schema = self.output_schema();

        let response = self.provider.generate(&prompt, &schema).await?;
        let output: StructureDiscoveryOutput = serde_json::from_value(response.content)?;

        // Apply discovered structure
        blueprint.discovered_sections = output
            .discovered_sections
            .into_iter()
            .map(|s| self.convert_discovered_section(s))
            .collect();

        blueprint.domain_structures = output
            .domain_structures
            .into_iter()
            .zip(domains.iter())
            .map(|(spec, domain)| self.convert_domain_structure(spec, domain))
            .collect();

        blueprint.hierarchy_depth = output.recommended_depth.clamp(1, 4);
        blueprint.estimated_pages = self.estimate_pages(&blueprint);

        Ok(blueprint)
    }

    fn determine_scale(profile: &ProjectProfile) -> ProjectScale {
        match profile.scale {
            crate::config::ProjectScale::Small => ProjectScale::Small,
            crate::config::ProjectScale::Medium => ProjectScale::Medium,
            crate::config::ProjectScale::Large => ProjectScale::Large,
            crate::config::ProjectScale::Enterprise => ProjectScale::Enterprise,
        }
    }

    fn build_context(&self, profile: &ProjectProfile, domains: &[DomainInsight]) -> String {
        let mut ctx = String::new();

        ctx.push_str(&format!("## Project: {}\n\n", profile.name));
        ctx.push_str(&format!("**Purposes**: {}\n", profile.purposes.join(", ")));
        ctx.push_str(&format!(
            "**Technical Stack**: {}\n",
            profile.technical_traits.join(", ")
        ));
        ctx.push_str(&format!(
            "**Domain Traits**: {}\n\n",
            profile.domain_traits.join(", ")
        ));

        ctx.push_str("## Domains\n\n");
        for domain in domains {
            ctx.push_str(&format!(
                "- **{}**: {} files, importance: {:?}\n",
                domain.name,
                domain.files.len(),
                domain.importance
            ));
        }

        if !profile.key_areas.is_empty() {
            ctx.push_str("\n## Key Areas\n\n");
            for area in &profile.key_areas {
                ctx.push_str(&format!("- {}: {:?}\n", area.path, area.importance));
            }
        }

        ctx
    }

    fn build_prompt(&self, context: &str, scale: ProjectScale) -> String {
        let scale_guidance = match scale {
            ProjectScale::Small => {
                "This is a small project. Keep documentation simple with 1 level of hierarchy."
            }
            ProjectScale::Medium => {
                "This is a medium project. Use 2 levels of hierarchy where beneficial."
            }
            ProjectScale::Large => {
                "This is a large project. Use up to 3 levels of hierarchy. Consider splitting large topics."
            }
            ProjectScale::Enterprise => {
                "This is an enterprise project. Use up to 4 levels of hierarchy. Create comprehensive documentation with multiple subsections."
            }
        };

        format!(
            r#"
<ROLE>
You are a documentation architect analyzing a codebase to design optimal documentation structure.
</ROLE>

<CONTEXT>
{context}
</CONTEXT>

<SCALE_GUIDANCE>
{scale_guidance}
</SCALE_GUIDANCE>

<TASK>
Design the optimal documentation structure for this project.

1. **Discover Project-Specific Sections**
   - What unique aspects of this project need dedicated documentation sections?
   - Examples: "External Integrations" if multiple API clients, "State Machines" if complex state logic, "Plugins" if extensible architecture
   - Only suggest sections with real content from the codebase

2. **Structure Each Domain**
   - Small domains (< 5 files): single_page
   - Medium domains (5-15 files): index_with_pages
   - Large domains (> 15 files): full_hierarchy
   - What subsections does each domain need?

3. **Recommend Hierarchy Depth**
   - 1: Flat structure (small projects)
   - 2: Sections with pages (medium)
   - 3: Sections with subsections (large)
   - 4: Full nested hierarchy (enterprise)

<RULES>
- 100% fact-based: Only suggest sections with actual content in the codebase
- No speculation: Don't add sections for features that don't exist
- Evidence required: List files that support each discovered section
- Practical: Don't over-structure small content
</RULES>
</TASK>
"#,
            context = context,
            scale_guidance = scale_guidance
        )
    }

    fn output_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "discovered_sections": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "path": { "type": "string" },
                            "reason": { "type": "string" },
                            "importance": { "type": "string", "enum": ["critical", "high", "medium", "low"] },
                            "subsections": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "name": { "type": "string" },
                                        "content_type": { "type": "string" }
                                    }
                                }
                            },
                            "evidence_files": { "type": "array", "items": { "type": "string" } }
                        }
                    }
                },
                "domain_structures": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "domain_name": { "type": "string" },
                            "structure_type": { "type": "string", "enum": ["single_page", "index_with_pages", "full_hierarchy"] },
                            "subsections": { "type": "array", "items": { "type": "object" } }
                        }
                    }
                },
                "recommended_depth": { "type": "integer", "minimum": 1, "maximum": 4 },
                "reasoning": { "type": "string" }
            },
            "required": ["discovered_sections", "domain_structures", "recommended_depth", "reasoning"]
        })
    }

    fn convert_discovered_section(&self, spec: DiscoveredSectionSpec) -> DiscoveredSection {
        use crate::wiki::exhaustive::types::Importance;

        let importance = match spec.importance.as_str() {
            "critical" => Importance::Critical,
            "high" => Importance::High,
            "medium" => Importance::Medium,
            _ => Importance::Low,
        };

        DiscoveredSection {
            name: spec.name,
            path: spec.path,
            reason: spec.reason,
            importance,
            subsections: spec
                .subsections
                .into_iter()
                .map(|s| Subsection {
                    name: s.name,
                    path: String::new(),
                    content_type: self.parse_content_type(&s.content_type),
                    source_files: vec![],
                })
                .collect(),
            source_files: spec.evidence_files,
            estimated_pages: 1,
        }
    }

    fn convert_domain_structure(
        &self,
        spec: DomainStructureSpec,
        domain: &DomainInsight,
    ) -> DomainDocStructure {
        let structure_type = match spec.structure_type.as_str() {
            "single_page" => DomainStructureType::SinglePage,
            "index_with_pages" => DomainStructureType::IndexWithPages,
            "full_hierarchy" => DomainStructureType::FullHierarchy,
            _ => DomainStructureType::SinglePage,
        };

        DomainDocStructure {
            domain_name: spec.domain_name,
            path: format!("domains/{}/", domain.name),
            importance: domain.importance,
            structure_type,
            subsections: spec
                .subsections
                .into_iter()
                .map(|s| Subsection {
                    name: s.name,
                    path: String::new(),
                    content_type: self.parse_content_type(&s.content_type),
                    source_files: vec![],
                })
                .collect(),
            file_count: domain.files.len(),
        }
    }

    fn parse_content_type(&self, s: &str) -> ContentType {
        match s.to_lowercase().as_str() {
            "overview" => ContentType::Overview,
            "tutorial" => ContentType::Tutorial,
            "reference" => ContentType::Reference,
            "guide" => ContentType::Guide,
            "concepts" => ContentType::Concepts,
            "api" | "apidocs" => ContentType::ApiDocs,
            "patterns" => ContentType::Patterns,
            "workflows" => ContentType::Workflows,
            "configuration" | "config" => ContentType::Configuration,
            "troubleshooting" => ContentType::Troubleshooting,
            _ => ContentType::Guide,
        }
    }

    fn estimate_pages(&self, blueprint: &DocumentationBlueprint) -> usize {
        let base = blueprint
            .base_sections
            .iter()
            .map(|s| 1 + s.subsections.len())
            .sum::<usize>();

        let discovered = blueprint
            .discovered_sections
            .iter()
            .map(|s| 1 + s.subsections.len())
            .sum::<usize>();

        let domains = blueprint
            .domain_structures
            .iter()
            .map(|d| match d.structure_type {
                DomainStructureType::SinglePage => 1,
                DomainStructureType::IndexWithPages => 1 + d.subsections.len(),
                DomainStructureType::FullHierarchy => 2 + d.subsections.len() * 2,
            })
            .sum::<usize>();

        base + discovered + domains
    }
}
