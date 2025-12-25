//! Documentation blueprint types

use crate::wiki::exhaustive::types::Importance;
use serde::{Deserialize, Serialize};

/// Project scale for documentation hierarchy
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProjectScale {
    Small,
    Medium,
    Large,
    Enterprise,
}

/// Complete documentation structure blueprint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentationBlueprint {
    /// Project scale determines base hierarchy depth
    pub scale: ProjectScale,

    /// Base sections (always included)
    pub base_sections: Vec<BaseSection>,

    /// Project-specific sections discovered by AI
    pub discovered_sections: Vec<DiscoveredSection>,

    /// Domain-specific document structures
    pub domain_structures: Vec<DomainDocStructure>,

    /// Maximum hierarchy depth (1-4)
    pub hierarchy_depth: u8,

    /// Estimated total pages
    pub estimated_pages: usize,
}

/// Standard sections present in all projects
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseSection {
    pub name: String,
    pub path: String,
    pub subsections: Vec<Subsection>,
}

/// AI-discovered project-specific section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredSection {
    pub name: String,
    pub path: String,
    pub reason: String, // Why this section was discovered
    pub importance: Importance,
    pub subsections: Vec<Subsection>,
    pub source_files: Vec<String>, // Evidence files
    pub estimated_pages: usize,
}

/// Subsection within a section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subsection {
    pub name: String,
    pub path: String,
    pub content_type: ContentType,
    pub source_files: Vec<String>,
}

/// Type of content in a subsection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentType {
    Overview,
    Tutorial,
    Reference,
    Guide,
    Concepts,
    ApiDocs,
    Patterns,
    Workflows,
    Configuration,
    Troubleshooting,
    HowTo,
    Explanation,
    Flows,
}

/// Domain-specific document structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainDocStructure {
    pub domain_name: String,
    pub path: String,
    pub importance: Importance,
    pub structure_type: DomainStructureType,
    pub subsections: Vec<Subsection>,
    pub file_count: usize,
}

/// How to structure a domain's documentation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DomainStructureType {
    /// Single page (small domain, < 5 files)
    SinglePage,
    /// Index + subpages (medium domain)
    IndexWithPages,
    /// Full hierarchy (large domain with sub-modules)
    FullHierarchy,
}

impl DocumentationBlueprint {
    /// Create default blueprint based on scale
    pub fn default_for_scale(scale: ProjectScale) -> Self {
        let hierarchy_depth = match scale {
            ProjectScale::Small => 1,
            ProjectScale::Medium => 2,
            ProjectScale::Large => 3,
            ProjectScale::Enterprise => 4,
        };

        Self {
            scale,
            base_sections: Self::default_base_sections(scale),
            discovered_sections: vec![],
            domain_structures: vec![],
            hierarchy_depth,
            estimated_pages: 0,
        }
    }

    fn default_base_sections(scale: ProjectScale) -> Vec<BaseSection> {
        match scale {
            ProjectScale::Small => vec![
                BaseSection {
                    name: "Overview".into(),
                    path: "index.md".into(),
                    subsections: vec![],
                },
                BaseSection {
                    name: "Getting Started".into(),
                    path: "getting-started.md".into(),
                    subsections: vec![],
                },
                BaseSection {
                    name: "API Reference".into(),
                    path: "api-reference.md".into(),
                    subsections: vec![],
                },
            ],
            ProjectScale::Medium | ProjectScale::Large | ProjectScale::Enterprise => vec![
                BaseSection {
                    name: "Overview".into(),
                    path: "index.md".into(),
                    subsections: vec![],
                },
                BaseSection {
                    name: "Getting Started".into(),
                    path: "getting-started/".into(),
                    subsections: vec![
                        Subsection {
                            name: "Installation".into(),
                            path: "installation.md".into(),
                            content_type: ContentType::Tutorial,
                            source_files: vec![],
                        },
                        Subsection {
                            name: "Configuration".into(),
                            path: "configuration.md".into(),
                            content_type: ContentType::Guide,
                            source_files: vec![],
                        },
                        Subsection {
                            name: "Quick Start".into(),
                            path: "quick-start.md".into(),
                            content_type: ContentType::Tutorial,
                            source_files: vec![],
                        },
                    ],
                },
                BaseSection {
                    name: "Architecture".into(),
                    path: "architecture/".into(),
                    subsections: vec![
                        Subsection {
                            name: "Overview".into(),
                            path: "index.md".into(),
                            content_type: ContentType::Overview,
                            source_files: vec![],
                        },
                        Subsection {
                            name: "Data Flow".into(),
                            path: "data-flow.md".into(),
                            content_type: ContentType::Concepts,
                            source_files: vec![],
                        },
                        Subsection {
                            name: "Patterns".into(),
                            path: "patterns.md".into(),
                            content_type: ContentType::Patterns,
                            source_files: vec![],
                        },
                    ],
                },
                BaseSection {
                    name: "Development".into(),
                    path: "development/".into(),
                    subsections: vec![
                        Subsection {
                            name: "Setup".into(),
                            path: "setup.md".into(),
                            content_type: ContentType::Guide,
                            source_files: vec![],
                        },
                        Subsection {
                            name: "Contributing".into(),
                            path: "contributing.md".into(),
                            content_type: ContentType::Guide,
                            source_files: vec![],
                        },
                    ],
                },
            ],
        }
    }

    /// Calculate total estimated pages across all sections
    pub fn total_estimated_pages(&self) -> usize {
        let base_pages = self
            .base_sections
            .iter()
            .map(|s| 1 + s.subsections.len())
            .sum::<usize>();

        let discovered_pages = self
            .discovered_sections
            .iter()
            .map(|s| s.estimated_pages)
            .sum::<usize>();

        let domain_pages = self
            .domain_structures
            .iter()
            .map(|d| match d.structure_type {
                DomainStructureType::SinglePage => 1,
                DomainStructureType::IndexWithPages => 1 + d.subsections.len(),
                DomainStructureType::FullHierarchy => {
                    // Estimate: index + subsections + potential nested pages
                    1 + d.subsections.len() + (d.file_count / 5).max(1)
                }
            })
            .sum::<usize>();

        base_pages + discovered_pages + domain_pages
    }

    /// Validate blueprint structure
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = vec![];

        // Check hierarchy depth is valid
        if !(1..=4).contains(&self.hierarchy_depth) {
            errors.push(format!(
                "Invalid hierarchy_depth: {}. Must be 1-4",
                self.hierarchy_depth
            ));
        }

        // Check hierarchy depth matches scale
        let expected_depth = match self.scale {
            ProjectScale::Small => 1,
            ProjectScale::Medium => 2,
            ProjectScale::Large => 3,
            ProjectScale::Enterprise => 4,
        };

        if self.hierarchy_depth != expected_depth {
            errors.push(format!(
                "Hierarchy depth {} doesn't match scale {:?} (expected {})",
                self.hierarchy_depth, self.scale, expected_depth
            ));
        }

        // Check for path collisions
        let mut paths = std::collections::HashSet::new();

        for section in &self.base_sections {
            if !paths.insert(&section.path) {
                errors.push(format!("Duplicate path in base sections: {}", section.path));
            }
        }

        for section in &self.discovered_sections {
            if !paths.insert(&section.path) {
                errors.push(format!(
                    "Duplicate path in discovered sections: {}",
                    section.path
                ));
            }
        }

        for domain in &self.domain_structures {
            if !paths.insert(&domain.path) {
                errors.push(format!(
                    "Duplicate path in domain structures: {}",
                    domain.path
                ));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

impl Default for DocumentationBlueprint {
    fn default() -> Self {
        Self::default_for_scale(ProjectScale::Medium)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_blueprint_small() {
        let blueprint = DocumentationBlueprint::default_for_scale(ProjectScale::Small);
        assert_eq!(blueprint.hierarchy_depth, 1);
        assert_eq!(blueprint.base_sections.len(), 3);
        assert!(blueprint.base_sections[0].subsections.is_empty());
    }

    #[test]
    fn test_default_blueprint_medium() {
        let blueprint = DocumentationBlueprint::default_for_scale(ProjectScale::Medium);
        assert_eq!(blueprint.hierarchy_depth, 2);
        assert_eq!(blueprint.base_sections.len(), 4);

        let getting_started = &blueprint.base_sections[1];
        assert_eq!(getting_started.name, "Getting Started");
        assert_eq!(getting_started.subsections.len(), 3);
    }

    #[test]
    fn test_default_blueprint_large() {
        let blueprint = DocumentationBlueprint::default_for_scale(ProjectScale::Large);
        assert_eq!(blueprint.hierarchy_depth, 3);
        assert_eq!(blueprint.base_sections.len(), 4);
    }

    #[test]
    fn test_default_blueprint_enterprise() {
        let blueprint = DocumentationBlueprint::default_for_scale(ProjectScale::Enterprise);
        assert_eq!(blueprint.hierarchy_depth, 4);
        assert_eq!(blueprint.base_sections.len(), 4);
    }

    #[test]
    fn test_total_estimated_pages() {
        let mut blueprint = DocumentationBlueprint::default_for_scale(ProjectScale::Small);
        assert_eq!(blueprint.total_estimated_pages(), 3);

        blueprint.discovered_sections.push(DiscoveredSection {
            name: "Custom Section".into(),
            path: "custom/".into(),
            reason: "Test".into(),
            importance: Importance::Medium,
            subsections: vec![],
            source_files: vec![],
            estimated_pages: 5,
        });

        assert_eq!(blueprint.total_estimated_pages(), 8);
    }

    #[test]
    fn test_validate_blueprint() {
        let blueprint = DocumentationBlueprint::default_for_scale(ProjectScale::Medium);
        assert!(blueprint.validate().is_ok());

        let mut invalid = blueprint.clone();
        invalid.hierarchy_depth = 0;
        assert!(invalid.validate().is_err());

        let mut mismatch = blueprint.clone();
        mismatch.hierarchy_depth = 1;
        let errors = mismatch.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("doesn't match scale")));
    }

    #[test]
    fn test_validate_path_collisions() {
        let mut blueprint = DocumentationBlueprint::default_for_scale(ProjectScale::Small);

        blueprint.discovered_sections.push(DiscoveredSection {
            name: "Duplicate".into(),
            path: "index.md".into(),
            reason: "Test".into(),
            importance: Importance::Medium,
            subsections: vec![],
            source_files: vec![],
            estimated_pages: 1,
        });

        let errors = blueprint.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("Duplicate path")));
    }
}
