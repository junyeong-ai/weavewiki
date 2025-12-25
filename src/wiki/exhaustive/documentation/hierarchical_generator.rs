//! Hierarchical documentation generator
//! Creates documentation files based on DocumentationBlueprint

use std::collections::HashMap;

use crate::types::Result;
use crate::wiki::exhaustive::characterization::profile::ProjectProfile;
use crate::wiki::exhaustive::consolidation::DomainInsight;
use crate::wiki::exhaustive::top_down::insights::ProjectInsight;

use super::blueprint::{
    BaseSection, ContentType, DiscoveredSection, DocumentationBlueprint, DomainDocStructure,
    DomainStructureType, Subsection,
};

/// Generated documentation output
#[derive(Debug, Clone)]
pub struct GeneratedDocumentation {
    /// All generated files (path relative to wiki root -> content)
    pub files: HashMap<String, String>,

    /// Navigation structure for index
    pub navigation: NavigationTree,

    /// Statistics
    pub stats: GenerationStats,
}

/// Navigation tree for documentation
#[derive(Debug, Clone)]
pub struct NavigationTree {
    pub items: Vec<NavigationItem>,
}

#[derive(Debug, Clone)]
pub struct NavigationItem {
    pub title: String,
    pub path: String,
    pub children: Vec<NavigationItem>,
}

#[derive(Debug, Clone, Default)]
pub struct GenerationStats {
    pub total_pages: usize,
    pub total_words: usize,
    pub hierarchy_depth: usize,
    pub domains_documented: usize,
}

/// Configuration for document generation
#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    /// Maximum words before splitting into sub-pages
    pub split_threshold: usize,

    /// Minimum words for a standalone page
    pub min_page_words: usize,

    /// Whether to generate navigation links
    pub generate_nav_links: bool,

    /// Whether to include source file references
    pub include_source_refs: bool,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            split_threshold: 5000,
            min_page_words: 100,
            generate_nav_links: true,
            include_source_refs: true,
        }
    }
}

/// Hierarchical documentation generator
pub struct HierarchicalDocGenerator {
    config: GeneratorConfig,
}

impl HierarchicalDocGenerator {
    pub fn new(config: GeneratorConfig) -> Self {
        Self { config }
    }

    /// Generate all documentation from blueprint and insights
    pub fn generate(
        &self,
        blueprint: &DocumentationBlueprint,
        profile: &ProjectProfile,
        domains: &[DomainInsight],
        project_insights: &[ProjectInsight],
    ) -> Result<GeneratedDocumentation> {
        let mut files = HashMap::new();
        let mut nav_items = Vec::new();
        let mut stats = GenerationStats::default();

        // 1. Generate index page
        let index_content = self.generate_index(blueprint, profile, domains);
        files.insert("index.md".to_string(), index_content);
        stats.total_pages += 1;

        // 2. Generate base sections
        for section in &blueprint.base_sections {
            let (section_files, section_nav) =
                self.generate_base_section(section, profile, domains, project_insights)?;
            files.extend(section_files);
            nav_items.push(section_nav);
            stats.total_pages += 1 + section.subsections.len();
        }

        // 3. Generate discovered sections
        for section in &blueprint.discovered_sections {
            let (section_files, section_nav) =
                self.generate_discovered_section(section, domains)?;
            files.extend(section_files);
            nav_items.push(section_nav);
            stats.total_pages += 1 + section.subsections.len();
        }

        // 4. Generate domain documentation
        let domains_nav = self.generate_domains_section(
            &blueprint.domain_structures,
            domains,
            &mut files,
            &mut stats,
        )?;
        nav_items.push(domains_nav);

        // 5. Generate llms.txt
        let llms_txt = self.generate_llms_txt(profile, domains);
        files.insert("llms.txt".to_string(), llms_txt);

        // Calculate word counts
        stats.total_words = files.values().map(|c| c.split_whitespace().count()).sum();
        stats.hierarchy_depth = blueprint.hierarchy_depth as usize;
        stats.domains_documented = domains.len();

        Ok(GeneratedDocumentation {
            files,
            navigation: NavigationTree { items: nav_items },
            stats,
        })
    }

    /// Generate main index page
    fn generate_index(
        &self,
        blueprint: &DocumentationBlueprint,
        profile: &ProjectProfile,
        domains: &[DomainInsight],
    ) -> String {
        let mut content = String::new();

        content.push_str(&format!("# {}\n\n", profile.name));

        // Project overview
        if !profile.purposes.is_empty() {
            content.push_str(&format!("{}\n\n", profile.purposes.join(". ")));
        }

        // Quick stats
        content.push_str("## Overview\n\n");
        content.push_str(&format!("- **Domains**: {}\n", domains.len()));
        content.push_str(&format!(
            "- **Documentation Pages**: {}\n",
            blueprint.estimated_pages
        ));
        content.push_str(&format!(
            "- **Tech Stack**: {}\n\n",
            profile.technical_traits.join(", ")
        ));

        // Navigation
        content.push_str("## Documentation\n\n");

        for section in &blueprint.base_sections {
            content.push_str(&format!("- [{}]({})\n", section.name, section.path));
        }

        for section in &blueprint.discovered_sections {
            content.push_str(&format!(
                "- [{}]({}) - {}\n",
                section.name, section.path, section.reason
            ));
        }

        content.push_str("\n## Domains\n\n");
        for domain in domains {
            content.push_str(&format!(
                "- [{}](domains/{}/index.md) - {} files\n",
                domain.name,
                domain.name,
                domain.files.len()
            ));
        }

        content
    }

    /// Generate a base section
    fn generate_base_section(
        &self,
        section: &BaseSection,
        profile: &ProjectProfile,
        domains: &[DomainInsight],
        project_insights: &[ProjectInsight],
    ) -> Result<(HashMap<String, String>, NavigationItem)> {
        let mut files = HashMap::new();
        let mut children = Vec::new();

        let section_path = if section.path.ends_with('/') {
            format!("{}index.md", section.path)
        } else {
            section.path.clone()
        };

        // Generate section index
        let mut content = String::new();
        content.push_str(&format!("# {}\n\n", section.name));

        // Add content based on section type
        match section.name.to_lowercase().as_str() {
            "architecture" => {
                content.push_str(&self.generate_architecture_content(project_insights));
            }
            "getting started" => {
                content.push_str(&self.generate_getting_started_content(profile));
            }
            "development" => {
                content.push_str(&self.generate_development_content(profile));
            }
            _ => {
                content.push_str(&format!("Documentation for {}.\n", section.name));
            }
        }

        // Add subsection links
        if !section.subsections.is_empty() {
            content.push_str("\n## Contents\n\n");
            for sub in &section.subsections {
                let sub_path = if section.path.ends_with('/') {
                    format!("{}{}", section.path, sub.path)
                } else {
                    sub.path.clone()
                };
                content.push_str(&format!("- [{}]({})\n", sub.name, sub.path));

                // Generate subsection file
                let sub_content = self.generate_subsection_content(sub, profile, domains);
                files.insert(sub_path.clone(), sub_content);

                children.push(NavigationItem {
                    title: sub.name.clone(),
                    path: sub_path,
                    children: vec![],
                });
            }
        }

        files.insert(section_path.clone(), content);

        Ok((
            files,
            NavigationItem {
                title: section.name.clone(),
                path: section_path,
                children,
            },
        ))
    }

    /// Generate discovered section
    fn generate_discovered_section(
        &self,
        section: &DiscoveredSection,
        _domains: &[DomainInsight],
    ) -> Result<(HashMap<String, String>, NavigationItem)> {
        let mut files = HashMap::new();
        let mut children = Vec::new();

        let section_path = format!("{}index.md", section.path);

        let mut content = String::new();
        content.push_str(&format!("# {}\n\n", section.name));
        content.push_str(&format!("> {}\n\n", section.reason));

        // List source files as evidence
        if self.config.include_source_refs && !section.source_files.is_empty() {
            content.push_str("## Source Files\n\n");
            for file in &section.source_files {
                content.push_str(&format!("- `{}`\n", file));
            }
            content.push('\n');
        }

        // Generate subsections
        for sub in &section.subsections {
            let sub_path = format!(
                "{}{}.md",
                section.path,
                sub.name.to_lowercase().replace(' ', "-")
            );
            content.push_str(&format!("- [{}]({})\n", sub.name, sub.path));

            let sub_content = format!("# {}\n\nDocumentation for {}.\n", sub.name, sub.name);
            files.insert(sub_path.clone(), sub_content);

            children.push(NavigationItem {
                title: sub.name.clone(),
                path: sub_path,
                children: vec![],
            });
        }

        files.insert(section_path.clone(), content);

        Ok((
            files,
            NavigationItem {
                title: section.name.clone(),
                path: section_path,
                children,
            },
        ))
    }

    /// Generate domains section
    fn generate_domains_section(
        &self,
        structures: &[DomainDocStructure],
        domains: &[DomainInsight],
        files: &mut HashMap<String, String>,
        stats: &mut GenerationStats,
    ) -> Result<NavigationItem> {
        let mut children = Vec::new();

        // Create domain index
        let mut index_content = String::from("# Domains\n\n");
        index_content.push_str("Project domains and their documentation.\n\n");

        for structure in structures {
            let domain = domains.iter().find(|d| d.name == structure.domain_name);
            if let Some(domain) = domain {
                let domain_nav = self.generate_domain_docs(structure, domain, files)?;
                children.push(domain_nav);
                stats.total_pages += match structure.structure_type {
                    DomainStructureType::SinglePage => 1,
                    DomainStructureType::IndexWithPages => 1 + structure.subsections.len(),
                    DomainStructureType::FullHierarchy => 2 + structure.subsections.len(),
                };

                index_content.push_str(&format!(
                    "- [{}]({}) - {} files, {:?}\n",
                    domain.name,
                    structure.path,
                    domain.files.len(),
                    domain.importance
                ));
            }
        }

        files.insert("domains/index.md".to_string(), index_content);

        Ok(NavigationItem {
            title: "Domains".to_string(),
            path: "domains/index.md".to_string(),
            children,
        })
    }

    /// Generate documentation for a single domain
    fn generate_domain_docs(
        &self,
        structure: &DomainDocStructure,
        domain: &DomainInsight,
        files: &mut HashMap<String, String>,
    ) -> Result<NavigationItem> {
        let mut children = Vec::new();
        let base_path = format!("domains/{}/", domain.name);

        match structure.structure_type {
            DomainStructureType::SinglePage => {
                // Single page with all content
                let mut content = String::new();
                content.push_str(&format!("# {}\n\n", domain.name));
                content.push_str(&domain.content);

                if let Some(diagram) = &domain.diagram {
                    content.push_str(&format!(
                        "\n## Architecture\n\n```mermaid\n{}\n```\n",
                        diagram
                    ));
                }

                content.push_str("\n## Files\n\n");
                for file in &domain.files {
                    content.push_str(&format!("- `{}`\n", file));
                }

                files.insert(format!("{}index.md", base_path), content);
            }
            DomainStructureType::IndexWithPages | DomainStructureType::FullHierarchy => {
                // Index page
                let mut index = String::new();
                index.push_str(&format!("# {}\n\n", domain.name));
                index.push_str(&format!("{}\n\n", domain.description));

                if let Some(diagram) = &domain.diagram {
                    index.push_str(&format!("## Overview\n\n```mermaid\n{}\n```\n\n", diagram));
                }

                index.push_str("## Contents\n\n");

                // Generate subsection pages
                for sub in &structure.subsections {
                    let sub_path = format!(
                        "{}{}.md",
                        base_path,
                        sub.name.to_lowercase().replace(' ', "-")
                    );
                    index.push_str(&format!("- [{}]({})\n", sub.name, sub.path));

                    let sub_content =
                        format!("# {}\n\nPart of {} domain.\n", sub.name, domain.name);
                    files.insert(sub_path.clone(), sub_content);

                    children.push(NavigationItem {
                        title: sub.name.clone(),
                        path: sub_path,
                        children: vec![],
                    });
                }

                // Files section
                index.push_str("\n## Files\n\n");
                for file in &domain.files {
                    index.push_str(&format!("- `{}`\n", file));
                }

                files.insert(format!("{}index.md", base_path), index);
            }
        }

        Ok(NavigationItem {
            title: domain.name.clone(),
            path: format!("{}index.md", base_path),
            children,
        })
    }

    // Helper content generators
    fn generate_architecture_content(&self, insights: &[ProjectInsight]) -> String {
        let mut content = String::new();

        for insight in insights {
            if let Some(pattern) = &insight.architecture_pattern {
                content.push_str(&format!("**Architecture Pattern**: {}\n\n", pattern));
            }

            if !insight.layers.is_empty() {
                content.push_str("## Layers\n\n");
                for layer in &insight.layers {
                    content.push_str(&format!(
                        "- **{}**: {} files\n",
                        layer.name,
                        layer.files.len()
                    ));
                }
                content.push('\n');
            }
        }

        if content.is_empty() {
            content.push_str("Project architecture documentation.\n");
        }

        content
    }

    fn generate_getting_started_content(&self, profile: &ProjectProfile) -> String {
        let mut content = String::new();

        content.push_str(&format!("Welcome to {}!\n\n", profile.name));
        content.push_str("## Prerequisites\n\n");
        content.push_str(&format!("- {}\n\n", profile.technical_traits.join("\n- ")));
        content.push_str("## Installation\n\n");
        content.push_str("See installation guide for setup instructions.\n");

        content
    }

    fn generate_development_content(&self, _profile: &ProjectProfile) -> String {
        let mut content = String::new();

        content.push_str("## Development Setup\n\n");
        content.push_str("Guide for developers contributing to this project.\n\n");
        content.push_str("## Code Style\n\n");
        content.push_str("Follow the project's coding conventions.\n");

        content
    }

    fn generate_subsection_content(
        &self,
        sub: &Subsection,
        profile: &ProjectProfile,
        _domains: &[DomainInsight],
    ) -> String {
        let mut content = String::new();
        content.push_str(&format!("# {}\n\n", sub.name));

        match sub.content_type {
            ContentType::Overview => {
                content.push_str(&format!("Overview of {} for {}.\n", sub.name, profile.name));
            }
            ContentType::Tutorial => {
                content.push_str(&format!("Step-by-step guide for {}.\n", sub.name));
            }
            ContentType::Reference => {
                content.push_str(&format!("Reference documentation for {}.\n", sub.name));
            }
            ContentType::Patterns => {
                content.push_str("## Detected Patterns\n\n");
                content.push_str("Code patterns found in this project.\n");
            }
            ContentType::HowTo => {
                content.push_str(&format!("How-to guide for {}.\n", sub.name));
            }
            ContentType::Explanation => {
                content.push_str(&format!("Explanation of {}.\n", sub.name));
            }
            ContentType::Flows => {
                content.push_str(&format!("Flows and processes for {}.\n", sub.name));
            }
            ContentType::ApiDocs => {
                content.push_str(&format!("API documentation for {}.\n", sub.name));
            }
            _ => {
                content.push_str(&format!("Documentation for {}.\n", sub.name));
            }
        }

        content
    }

    fn generate_llms_txt(&self, profile: &ProjectProfile, domains: &[DomainInsight]) -> String {
        let mut content = String::new();

        content.push_str(&format!("# {}\n\n", profile.name));
        content.push_str(&format!("{}\n\n", profile.purposes.join(". ")));

        content.push_str("## Domains\n\n");
        for domain in domains {
            content.push_str(&format!("### {}\n", domain.name));
            content.push_str(&format!("{}\n", domain.description));
            content.push_str(&format!("Files: {}\n\n", domain.files.join(", ")));
        }

        content
    }
}
