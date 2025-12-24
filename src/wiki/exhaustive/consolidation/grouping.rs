//! Semantic Domain Grouping
//!
//! Groups file insights by semantic domain using LLM-based analysis.
//! Leverages project profile context (organization_style, domain_traits, terminology)
//! to inform intelligent grouping decisions.

use crate::ai::provider::SharedProvider;
use crate::types::error::WeaveError;
use crate::wiki::exhaustive::bottom_up::FileInsight;
use crate::wiki::exhaustive::characterization::profile::ProjectProfile;
use serde_json::json;
use std::collections::HashMap;

/// LLM-based semantic domain grouper
///
/// Uses project profile and file purposes to semantically group files
/// into logical domains, producing more meaningful organization than
/// simple path-based grouping.
pub struct SemanticDomainGrouper<'a> {
    profile: &'a ProjectProfile,
    provider: SharedProvider,
}

impl<'a> SemanticDomainGrouper<'a> {
    pub fn new(profile: &'a ProjectProfile, provider: SharedProvider) -> Self {
        Self { profile, provider }
    }

    /// Group file insights by semantic domain using LLM
    pub async fn group(
        &self,
        insights: &[FileInsight],
    ) -> Result<HashMap<String, Vec<FileInsight>>, WeaveError> {
        if insights.is_empty() {
            return Ok(HashMap::new());
        }

        // For small projects (<= 20 files), use LLM grouping
        // For larger projects, use batched approach
        if insights.len() <= 50 {
            self.group_with_llm(insights).await
        } else {
            // For large projects, pre-group by path then refine
            let path_groups = self.group_by_path(insights);
            self.refine_groups_with_llm(path_groups).await
        }
    }

    /// LLM-based grouping for smaller file sets
    async fn group_with_llm(
        &self,
        insights: &[FileInsight],
    ) -> Result<HashMap<String, Vec<FileInsight>>, WeaveError> {
        let file_summaries = self.build_file_summaries(insights);
        let profile_context = self.build_profile_context();

        let prompt = format!(
            r#"Group these source files into semantic domains based on their purpose and functionality.

## Project Context
{}

## Files to Group
{}

## Instructions
1. Analyze each file's purpose and relationships
2. Group files by logical/semantic domain (NOT just directory structure)
3. Merge small groups (< 3 files) into related larger groups
4. CRITICAL: Every file MUST be assigned to exactly one domain - no file can be left out
5. Domain names MUST use only lowercase letters, numbers, and hyphens (e.g., "authentication", "data-storage", "cli-commands")
6. DO NOT use special characters like "&" or spaces in domain names
7. DO NOT create a catch-all "other" domain - every file should have a meaningful categorization

Return a JSON object with domain assignments."#,
            profile_context, file_summaries
        );

        let schema = json!({
            "type": "object",
            "description": "Semantic domain grouping of source files",
            "required": ["domains"],
            "additionalProperties": false,
            "properties": {
                "domains": {
                    "type": "array",
                    "description": "List of semantic domains with their files",
                    "items": {
                        "type": "object",
                        "required": ["name", "description", "files"],
                        "additionalProperties": false,
                        "properties": {
                            "name": {
                                "type": "string",
                                "description": "Domain name (lowercase, hyphenated)"
                            },
                            "description": {
                                "type": "string",
                                "description": "Brief description of what this domain handles"
                            },
                            "files": {
                                "type": "array",
                                "description": "File paths belonging to this domain",
                                "items": {"type": "string"}
                            }
                        }
                    }
                }
            }
        });

        let result = self.provider.generate(&prompt, &schema).await?.content;

        // Parse result and build HashMap
        self.parse_grouping_result(&result, insights)
    }

    /// Pre-group by path for large projects
    fn group_by_path<'b>(
        &self,
        insights: &'b [FileInsight],
    ) -> HashMap<String, Vec<&'b FileInsight>> {
        let mut groups: HashMap<String, Vec<&FileInsight>> = HashMap::new();

        for insight in insights {
            let domain = self.extract_path_domain(&insight.file_path);
            groups.entry(domain).or_default().push(insight);
        }

        groups
    }

    /// Extract domain from path (for pre-grouping)
    /// Uses deeper path hierarchy (2-3 levels) for more meaningful grouping
    fn extract_path_domain(&self, path: &str) -> String {
        let parts: Vec<&str> = path.split('/').collect();

        // Skip common prefixes and find meaningful domain
        for (i, part) in parts.iter().enumerate() {
            if *part == "src" && i + 1 < parts.len() {
                let next = parts[i + 1];
                // Skip single-file "domains" like lib.rs, main.rs
                if next.contains('.') {
                    continue;
                }

                // For deeper modules, use 2-level path for better grouping
                // e.g., "wiki/exhaustive/bottom_up" → "wiki-exhaustive-bottom-up"
                if i + 3 < parts.len() && !parts[i + 2].contains('.') && !parts[i + 3].contains('.')
                {
                    return format!("{}-{}-{}", next, parts[i + 2], parts[i + 3]);
                }

                // For 2-level modules, use hyphenated path
                // e.g., "wiki/exhaustive" → "wiki-exhaustive"
                if i + 2 < parts.len() && !parts[i + 2].contains('.') {
                    return format!("{}-{}", next, parts[i + 2]);
                }

                return next.to_string();
            }
        }

        // Fallback
        if parts.len() > 1 {
            parts[0].to_string()
        } else {
            "core".to_string()
        }
    }

    /// Refine path-based groups with LLM
    async fn refine_groups_with_llm(
        &self,
        path_groups: HashMap<String, Vec<&FileInsight>>,
    ) -> Result<HashMap<String, Vec<FileInsight>>, WeaveError> {
        // Collect ALL file paths for the LLM to reference
        let all_file_paths: Vec<String> = path_groups
            .values()
            .flatten()
            .map(|fi| fi.file_path.clone())
            .collect();

        let group_summary = self.build_group_summary(&path_groups);

        let prompt = format!(
            r#"Review and refine these pre-grouped domains for a documentation wiki.

## Project Context
Organization: {:?}
Domain traits: {}

## Current Groups
{}

## ALL File Paths (must all be assigned)
{}

## Instructions
1. CRITICAL: Every file path listed above MUST appear in exactly one domain
2. Merge groups that belong together semantically (e.g., related subsystems)
3. Split large groups (>15 files) into meaningful sub-domains if they have distinct purposes
4. Rename groups to be human-readable and descriptive
5. Domain names MUST use only lowercase letters, numbers, and hyphens (e.g., "ai-integration", "code-analysis")
6. DO NOT use special characters like "&" in domain names
7. Keep groups meaningful (3+ files each)
8. DO NOT create a catch-all "other" domain - every file should have a proper home

Return the complete refined grouping with ALL files assigned."#,
            self.profile.organization_style,
            self.profile.domain_traits.join(", "),
            group_summary,
            all_file_paths.join("\n")
        );

        let schema = json!({
            "type": "object",
            "description": "Refined domain grouping",
            "required": ["domains"],
            "additionalProperties": false,
            "properties": {
                "domains": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["name", "files"],
                        "additionalProperties": false,
                        "properties": {
                            "name": {"type": "string", "description": "Domain name"},
                            "description": {"type": "string", "description": "Domain description"},
                            "files": {"type": "array", "items": {"type": "string"}}
                        }
                    }
                }
            }
        });

        let result = self.provider.generate(&prompt, &schema).await?.content;

        // Convert path_groups to owned insights for result parsing
        let all_insights: Vec<FileInsight> = path_groups.into_values().flatten().cloned().collect();

        self.parse_grouping_result(&result, &all_insights)
    }

    /// Build file summaries for LLM prompt
    fn build_file_summaries(&self, insights: &[FileInsight]) -> String {
        let mut summaries = String::new();

        for (i, fi) in insights.iter().enumerate() {
            summaries.push_str(&format!("{}. {} - {}\n", i + 1, fi.file_path, fi.purpose));

            // Include content summary for better grouping (limited)
            if fi.has_content() {
                let summary: String = fi.content.chars().take(100).collect();
                let first_line = summary.lines().next().unwrap_or("");
                summaries.push_str(&format!("   Key: {}\n", first_line));
            }
        }

        summaries
    }

    /// Build profile context for LLM prompt
    fn build_profile_context(&self) -> String {
        let mut ctx = String::new();

        ctx.push_str(&format!("Project: {}\n", self.profile.name));
        ctx.push_str(&format!(
            "Organization: {:?}\n",
            self.profile.organization_style
        ));

        if !self.profile.purposes.is_empty() {
            ctx.push_str(&format!("Purpose: {}\n", self.profile.purposes.join(", ")));
        }

        if !self.profile.domain_traits.is_empty() {
            ctx.push_str(&format!(
                "Domain: {}\n",
                self.profile.domain_traits.join(", ")
            ));
        }

        if !self.profile.terminology.is_empty() {
            ctx.push_str("Key terms: ");
            let terms: Vec<&str> = self
                .profile
                .terminology
                .iter()
                .take(5)
                .map(|t| t.term.as_str())
                .collect();
            ctx.push_str(&terms.join(", "));
            ctx.push('\n');
        }

        ctx
    }

    /// Build summary of pre-grouped domains
    /// Shows more files per group for better LLM context
    fn build_group_summary(&self, groups: &HashMap<String, Vec<&FileInsight>>) -> String {
        let mut summary = String::new();

        // Sort groups by size (largest first) for better LLM attention
        let mut sorted_groups: Vec<_> = groups.iter().collect();
        sorted_groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

        for (domain, files) in sorted_groups {
            summary.push_str(&format!("\n### {} ({} files)\n", domain, files.len()));

            // Show more files for larger groups (up to 15)
            let show_count = if files.len() > 20 {
                15
            } else {
                10.min(files.len())
            };

            for fi in files.iter().take(show_count) {
                summary.push_str(&format!("  - {}: {}\n", fi.file_path, fi.purpose));
            }
            if files.len() > show_count {
                summary.push_str(&format!("  ... and {} more\n", files.len() - show_count));
            }
        }

        summary
    }

    /// Sanitize domain name to ensure it follows naming conventions
    ///
    /// - Converts to lowercase
    /// - Allows only alphanumeric characters and hyphens
    /// - Replaces other characters with hyphens
    /// - Removes consecutive hyphens
    /// - Trims leading/trailing hyphens
    fn sanitize_domain_name(name: &str) -> String {
        name.to_lowercase()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    }

    /// Parse LLM grouping result into HashMap
    fn parse_grouping_result(
        &self,
        result: &serde_json::Value,
        insights: &[FileInsight],
    ) -> Result<HashMap<String, Vec<FileInsight>>, WeaveError> {
        let mut groups: HashMap<String, Vec<FileInsight>> = HashMap::new();
        let mut assigned: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Build lookup map
        let insight_map: HashMap<&str, &FileInsight> = insights
            .iter()
            .map(|fi| (fi.file_path.as_str(), fi))
            .collect();

        // Parse domains from result
        if let Some(domains) = result.get("domains").and_then(|d| d.as_array()) {
            for domain in domains {
                let raw_name = domain
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown");
                let name = Self::sanitize_domain_name(raw_name);

                if let Some(files) = domain.get("files").and_then(|f| f.as_array()) {
                    for file in files {
                        if let Some(path) = file.as_str()
                            && let Some(insight) = insight_map.get(path)
                            && !assigned.contains(path)
                        {
                            groups
                                .entry(name.clone())
                                .or_default()
                                .push((*insight).clone());
                            assigned.insert(path.to_string());
                        }
                    }
                }
            }
        }

        // Assign unassigned files to nearest existing domain by path prefix
        for fi in insights {
            if !assigned.contains(&fi.file_path) {
                // Find best matching domain based on path prefix
                let best_domain = groups
                    .keys()
                    .filter(|domain| *domain != "other")
                    .max_by_key(|domain| {
                        // Count common path components
                        let domain_lower = domain.to_lowercase();
                        let path_parts: Vec<&str> = fi.file_path.split('/').collect();
                        path_parts
                            .iter()
                            .filter(|part| domain_lower.contains(&part.to_lowercase()))
                            .count()
                    })
                    .cloned();

                if let Some(domain) = best_domain {
                    groups.entry(domain.clone()).or_default().push(fi.clone());
                    tracing::debug!(
                        "Assigned orphan file {} to nearest domain {}",
                        fi.file_path,
                        domain
                    );
                } else {
                    // If no domains exist yet, create one from path
                    let fallback_domain = fi
                        .file_path
                        .split('/')
                        .find(|p| !p.is_empty() && *p != "src" && *p != "lib")
                        .unwrap_or("core")
                        .to_string();
                    groups
                        .entry(fallback_domain.clone())
                        .or_default()
                        .push(fi.clone());
                    tracing::debug!(
                        "Created domain {} for orphan file {}",
                        fallback_domain,
                        fi.file_path
                    );
                }
            }
        }

        tracing::debug!(
            "Semantic grouping: {} domains, {} files assigned",
            groups.len(),
            assigned.len()
        );

        Ok(groups)
    }
}

#[cfg(test)]
mod tests {
    fn test_extract_domain(path: &str) -> String {
        let parts: Vec<&str> = path.split('/').collect();

        for (i, part) in parts.iter().enumerate() {
            if *part == "src" && i + 1 < parts.len() {
                let next = parts[i + 1];
                if next.contains('.') {
                    continue;
                }

                // 3-level path
                if i + 3 < parts.len() && !parts[i + 2].contains('.') && !parts[i + 3].contains('.')
                {
                    return format!("{}-{}-{}", next, parts[i + 2], parts[i + 3]);
                }

                // 2-level path
                if i + 2 < parts.len() && !parts[i + 2].contains('.') {
                    return format!("{}-{}", next, parts[i + 2]);
                }

                return next.to_string();
            }
        }

        if parts.len() > 1 {
            parts[0].to_string()
        } else {
            "core".to_string()
        }
    }

    #[test]
    fn test_extract_path_domain() {
        // Single level module
        assert_eq!(test_extract_domain("src/types/mod.rs"), "types");

        // Two level module
        assert_eq!(
            test_extract_domain("src/cli/commands/build.rs"),
            "cli-commands"
        );
        assert_eq!(
            test_extract_domain("src/analyzer/parser/rust.rs"),
            "analyzer-parser"
        );

        // Three level module
        assert_eq!(
            test_extract_domain("src/wiki/exhaustive/bottom_up/mod.rs"),
            "wiki-exhaustive-bottom_up"
        );

        // Non-src paths
        assert_eq!(test_extract_domain("tests/integration.rs"), "tests");

        // Root level files
        assert_eq!(test_extract_domain("src/lib.rs"), "src");
        assert_eq!(test_extract_domain("main.rs"), "core");
    }

    #[test]
    fn test_sanitize_domain_name() {
        use super::SemanticDomainGrouper;

        // Basic lowercase conversion
        assert_eq!(
            SemanticDomainGrouper::sanitize_domain_name("MyDomain"),
            "mydomain"
        );

        // Special characters replacement
        assert_eq!(
            SemanticDomainGrouper::sanitize_domain_name("data&storage"),
            "data-storage"
        );
        assert_eq!(
            SemanticDomainGrouper::sanitize_domain_name("cli commands"),
            "cli-commands"
        );
        assert_eq!(
            SemanticDomainGrouper::sanitize_domain_name("api/endpoints"),
            "api-endpoints"
        );

        // Consecutive hyphens removal
        assert_eq!(
            SemanticDomainGrouper::sanitize_domain_name("data---storage"),
            "data-storage"
        );
        assert_eq!(
            SemanticDomainGrouper::sanitize_domain_name("cli  &  commands"),
            "cli-commands"
        );

        // Leading/trailing hyphens removal
        assert_eq!(
            SemanticDomainGrouper::sanitize_domain_name("-domain-"),
            "domain"
        );
        assert_eq!(
            SemanticDomainGrouper::sanitize_domain_name("---domain---"),
            "domain"
        );
        assert_eq!(
            SemanticDomainGrouper::sanitize_domain_name(" domain "),
            "domain"
        );

        // Already clean names
        assert_eq!(
            SemanticDomainGrouper::sanitize_domain_name("authentication"),
            "authentication"
        );
        assert_eq!(
            SemanticDomainGrouper::sanitize_domain_name("data-storage"),
            "data-storage"
        );
        assert_eq!(
            SemanticDomainGrouper::sanitize_domain_name("cli-commands"),
            "cli-commands"
        );

        // Numbers are allowed
        assert_eq!(
            SemanticDomainGrouper::sanitize_domain_name("oauth2-flow"),
            "oauth2-flow"
        );
        assert_eq!(
            SemanticDomainGrouper::sanitize_domain_name("http2-server"),
            "http2-server"
        );

        // Complex cases
        assert_eq!(
            SemanticDomainGrouper::sanitize_domain_name("AI & ML Processing"),
            "ai-ml-processing"
        );
        assert_eq!(
            SemanticDomainGrouper::sanitize_domain_name("User@Authentication"),
            "user-authentication"
        );
    }
}
