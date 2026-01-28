use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::ai::response::generate_schema;
use crate::ai::validation::deserialize_llm_response;
use crate::ai::LlmProvider;
use crate::pipeline::analysis::SynthesizedInsights;
use crate::pipeline::context::VerifiedFileRegistry;
use crate::types::domain::DomainAnalysisResult;
use crate::types::module_map::{DetectedModule, ModuleGroup};
use crate::types::node::EvidenceLocation;
use crate::types::Result;

use super::constraint_extraction::ExtractedConstraints;
use super::convention_inference::InferredConventions;
use super::project_detection::ProjectDetection;

use crate::pipeline::analysis::SynthesizedAnalysis;

pub struct ModuleDetectionResult {
    pub modules: Vec<DetectedModule>,
    pub groups: Vec<ModuleGroup>,
}

pub struct ModuleDetector {
    provider: Arc<dyn LlmProvider>,
    min_modules_for_grouping: usize,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
struct ModuleDetectionOutput {
    modules: Vec<LlmDetectedModule>,
    #[serde(default)]
    groups: Vec<LlmModuleGroup>,
    confidence: f32,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
struct LlmDetectedModule {
    module_id: String,
    paths: Vec<String>,
    key_files: Vec<String>,
    dependencies: Vec<String>,
    responsibility: String,
    conventions: Vec<String>,
    known_issues: Vec<String>,
    value_score: f32,
    risk_score: f32,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
struct LlmModuleGroup {
    group_id: String,
    name: String,
    module_ids: Vec<String>,
    responsibility: String,
}

impl ModuleDetector {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            min_modules_for_grouping: 6,
        }
    }

    pub fn with_grouping_threshold(mut self, min_modules: usize) -> Self {
        self.min_modules_for_grouping = min_modules;
        self
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn detect(
        &self,
        detection: &ProjectDetection,
        file_registry: &VerifiedFileRegistry,
        synthesis: Option<&SynthesizedAnalysis>,
        domain_analysis: Option<&DomainAnalysisResult>,
        cross_insights: Option<&SynthesizedInsights>,
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
    ) -> Result<ModuleDetectionResult> {
        let file_context = file_registry.to_prompt_context(200);
        let modules_by_dir = file_registry.files_by_module();

        let mut context_sections = String::new();

        // Directory structure summary
        context_sections.push_str("## Directory Structure (files by top-level module)\n");
        let mut sorted_modules: Vec<_> = modules_by_dir.iter().collect();
        sorted_modules.sort_by_key(|(name, _)| (*name).clone());
        for (module, files) in &sorted_modules {
            let total_lines: usize = files.iter().map(|f| f.line_count).sum();
            context_sections.push_str(&format!(
                "- {}: {} files, {} lines\n",
                module,
                files.len(),
                total_lines
            ));
        }

        // Synthesis context
        if let Some(synth) = synthesis {
            context_sections.push_str("\n## Synthesized Modules\n");
            for module in &synth.modules {
                context_sections.push_str(&format!(
                    "- {} ({}): {}\n  deps: {}\n  constraints: {}\n",
                    module.name,
                    module.path,
                    module.responsibility,
                    module.internal_deps.join(", "),
                    module.constraints.join("; ")
                ));
            }
        }

        // Architecture
        if !conventions.architecture.pattern_name.is_empty() {
            context_sections.push_str(&format!(
                "\n## Architecture: {}\nLayers: {}\n",
                conventions.architecture.pattern_name,
                conventions
                    .architecture
                    .layers
                    .iter()
                    .map(|l| l.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" → ")
            ));
        }

        // Domain context
        if let Some(domain) = domain_analysis.filter(|d| !d.core_logic.is_empty()) {
            context_sections.push_str("\n## Domain Logic\n");
            for logic in &domain.core_logic {
                context_sections.push_str(&format!(
                    "- {} ({:?}): {}\n",
                    logic.name, logic.logic_type, logic.description
                ));
            }
        }

        // Cross-synthesis insights
        if let Some(insights) = cross_insights.filter(|i| !i.hidden_dependencies.is_empty()) {
            context_sections.push_str("\n## Hidden Dependencies\n");
            for dep in &insights.hidden_dependencies {
                context_sections.push_str(&format!(
                    "- {} → {}: {}\n",
                    dep.from_module, dep.to_module, dep.description
                ));
            }
        }

        // Constraints
        if !constraints.hidden_dependencies.is_empty() || !constraints.gotchas.is_empty() {
            context_sections.push_str("\n## Known Constraints\n");
            for dep in &constraints.hidden_dependencies {
                context_sections.push_str(&format!("- Hidden dep: {} → {}\n", dep.source, dep.target));
            }
            for gotcha in &constraints.gotchas {
                context_sections.push_str(&format!("- Gotcha: {}\n", gotcha.title));
            }
        }

        let langs: Vec<_> = detection
            .languages
            .iter()
            .map(|l| l.language.as_str())
            .collect();

        let grouping_instruction = if sorted_modules.len() >= self.min_modules_for_grouping {
            "\n\
            GROUPING (required when 6+ modules detected):\n\
            Group related modules into logical clusters. Each group:\n\
            - group_id: kebab-case identifier\n\
            - name: human-readable group name\n\
            - module_ids: list of module_ids belonging to this group\n\
            - responsibility: what this group collectively handles\n\
            Return groups in the \"groups\" field.\n"
        } else {
            ""
        };

        let prompt = format!(
            "Analyze this {project_type} project ({langs}) and identify distinct functional modules \
            that would benefit from specialized Claude Code agents.\n\n\
            {file_context}\n\
            {context_sections}\n\
            REQUIREMENTS:\n\
            1. Each module must have clear boundaries (specific directory paths)\n\
            2. module_id must be kebab-case\n\
            3. key_files must reference actual files from the AVAILABLE FILES list\n\
            4. dependencies list other module_ids this module depends on\n\
            5. responsibility is a concise description of what this module does\n\
            6. value_score (0.0-1.0): how much value a specialized agent adds\n\
            7. risk_score (0.0-1.0): how likely mistakes are without specialized knowledge\n\
            8. conventions: module-specific coding patterns\n\
            9. known_issues: gotchas specific to this module\n\
            10. Only detect modules with genuine complexity - skip trivial directories\n\
            {grouping_instruction}\n\
            Return as JSON: {{\"modules\": [...], \"groups\": [...], \"confidence\": 0.0-1.0}}\n",
            project_type = detection.primary_type.as_str(),
            langs = langs.join(", "),
            file_context = file_context,
            context_sections = context_sections,
            grouping_instruction = grouping_instruction,
        );

        let schema = generate_schema::<ModuleDetectionOutput>();

        let response = self.provider.generate(&prompt, &schema).await?;
        let output: ModuleDetectionOutput =
            deserialize_llm_response(&response.content, "module_detection")?;

        let confidence = output.confidence;
        let llm_groups = output.groups;
        let mut modules = self.validate_and_convert_modules(output.modules, file_registry);
        self.validate_modules(&mut modules);

        let groups = self.validate_and_convert_groups(llm_groups, &modules);

        tracing::info!(
            detected = modules.len(),
            groups = groups.len(),
            confidence = confidence,
            "Module detection complete"
        );

        Ok(ModuleDetectionResult { modules, groups })
    }

    fn validate_and_convert_modules(
        &self,
        modules: Vec<LlmDetectedModule>,
        file_registry: &VerifiedFileRegistry,
    ) -> Vec<DetectedModule> {
        modules
            .into_iter()
            .filter_map(|llm_module| {
                // Validate key_files exist
                let verified_key_files: Vec<String> = llm_module
                    .key_files
                    .into_iter()
                    .filter(|f| file_registry.contains(f))
                    .collect();

                // Validate paths reference real directories or files
                let verified_paths: Vec<String> = llm_module
                    .paths
                    .into_iter()
                    .filter(|p| {
                        file_registry.directory_exists(p.trim_end_matches('/'))
                            || !file_registry
                                .files_in_directory(p.trim_end_matches('/'))
                                .is_empty()
                    })
                    .collect();

                if verified_paths.is_empty() {
                    tracing::debug!(
                        module = %llm_module.module_id,
                        "Skipping module with no verified paths"
                    );
                    return None;
                }

                // Calculate coverage from verified files
                let files_in_module: Vec<String> = verified_paths
                    .iter()
                    .flat_map(|p| file_registry.files_in_directory(p.trim_end_matches('/')))
                    .collect();
                let total_files = file_registry.file_count();
                let coverage_ratio = if total_files > 0 {
                    files_in_module.len() as f32 / total_files as f32
                } else {
                    0.0
                };

                // Build evidence from key files
                let evidence: Vec<EvidenceLocation> = verified_key_files
                    .iter()
                    .map(|f| {
                        let line_count = file_registry.line_count(f).unwrap_or(1) as u32;
                        EvidenceLocation {
                            file: f.clone(),
                            start_line: 1,
                            end_line: line_count,
                            start_column: None,
                            end_column: None,
                        }
                    })
                    .collect();

                Some(DetectedModule {
                    module_id: llm_module.module_id,
                    paths: verified_paths,
                    key_files: verified_key_files,
                    dependencies: llm_module.dependencies,
                    responsibility: llm_module.responsibility,
                    coverage_ratio,
                    value_score: llm_module.value_score.clamp(0.0, 1.0),
                    risk_score: llm_module.risk_score.clamp(0.0, 1.0),
                    conventions: llm_module.conventions,
                    known_issues: llm_module.known_issues,
                    evidence,
                })
            })
            .collect()
    }

    fn validate_modules(&self, modules: &mut Vec<DetectedModule>) {
        self.resolve_path_overlaps(modules);
        self.prune_invalid_dependencies(modules);
    }

    fn resolve_path_overlaps(&self, modules: &mut Vec<DetectedModule>) {
        let mut best_score: std::collections::HashMap<String, f32> =
            std::collections::HashMap::new();

        for module in modules.iter() {
            for path in &module.paths {
                let normalized = path.trim_end_matches('/').to_string();
                let entry = best_score.entry(normalized).or_insert(0.0);
                if module.value_score > *entry {
                    *entry = module.value_score;
                }
            }
        }

        for module in modules.iter_mut() {
            module.paths.retain(|path| {
                let normalized = path.trim_end_matches('/').to_string();
                best_score
                    .get(&normalized)
                    .map(|score| (*score - module.value_score).abs() < f32::EPSILON)
                    .unwrap_or(true)
            });
        }

        modules.retain(|m| !m.paths.is_empty());
    }

    fn prune_invalid_dependencies(&self, modules: &mut [DetectedModule]) {
        let valid_ids: std::collections::HashSet<String> =
            modules.iter().map(|m| m.module_id.clone()).collect();
        for module in modules.iter_mut() {
            module
                .dependencies
                .retain(|dep| valid_ids.contains(dep));
        }
    }

    fn validate_and_convert_groups(
        &self,
        llm_groups: Vec<LlmModuleGroup>,
        modules: &[DetectedModule],
    ) -> Vec<ModuleGroup> {
        let valid_ids: std::collections::HashSet<&str> =
            modules.iter().map(|m| m.module_id.as_str()).collect();
        let mut assigned: std::collections::HashSet<String> = std::collections::HashSet::new();

        llm_groups
            .into_iter()
            .filter_map(|g| {
                let module_ids: Vec<String> = g
                    .module_ids
                    .into_iter()
                    .filter(|id| valid_ids.contains(id.as_str()) && !assigned.contains(id))
                    .collect();

                if module_ids.is_empty() {
                    return None;
                }

                for id in &module_ids {
                    assigned.insert(id.clone());
                }

                let external_dependencies = self.compute_external_deps(&module_ids, modules);

                Some(ModuleGroup {
                    group_id: g.group_id,
                    name: g.name,
                    module_ids,
                    responsibility: g.responsibility,
                    external_dependencies,
                })
            })
            .collect()
    }

    fn compute_external_deps(
        &self,
        group_module_ids: &[String],
        modules: &[DetectedModule],
    ) -> Vec<String> {
        let group_set: std::collections::HashSet<&str> =
            group_module_ids.iter().map(|s| s.as_str()).collect();
        let mut external = std::collections::HashSet::new();

        for module in modules {
            if group_set.contains(module.module_id.as_str()) {
                for dep in &module.dependencies {
                    if !group_set.contains(dep.as_str()) {
                        external.insert(dep.clone());
                    }
                }
            }
        }

        let mut result: Vec<String> = external.into_iter().collect();
        result.sort();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_detection_output_schema() {
        let schema = generate_schema::<ModuleDetectionOutput>();
        assert!(schema.get("properties").is_some());
    }
}
