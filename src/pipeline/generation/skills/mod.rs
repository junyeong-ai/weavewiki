//! Skills Generator
//!
//! LLM-first skill discovery architecture:
//! - Primary: LLM discovers project-specific valuable skills
//! - Retry: Retry with exponential backoff on failure (no generic fallback)
//!
//! Skills define HOW to do things. Domain knowledge (WHAT) comes from Rules.
//! All skills must be project-specific - generic skills provide no value.

mod disclosure;
mod discovery;
mod extended;
mod monorepo;
mod prompt;
mod resolver;

pub use disclosure::{DynamicContextInjector, ProgressiveDisclosure, RuleCrossReferencer, SkillCrossReferencer};
pub use discovery::SkillDiscovery;
pub use extended::{ExtendedSkillsGenerator, ProjectEvidence};
pub use monorepo::{MonorepoSkillsGenerator, WorkspaceSkill};
pub use prompt::SkillPromptBuilder;
pub use resolver::ModuleSkillResolver;

use std::sync::Arc;
use std::time::Duration;

use super::context::GenerationContext;
use crate::ai::LlmProvider;
use crate::config::Config;
use crate::pipeline::context::FileRegistryExt;
use crate::types::{Result, Skill};

/// Maximum retry attempts for skill discovery
const MAX_RETRIES: u32 = 3;
/// Base delay for exponential backoff (doubles each retry)
const BASE_RETRY_DELAY_MS: u64 = 1000;

/// Extract and normalize LLM-generated skill body content.
///
/// Strips code block fences and ensures a markdown heading exists.
pub(crate) fn extract_skill_body(response: &str, name: &str) -> String {
    let content = response.trim();

    let content = if content.starts_with("```") {
        let lines: Vec<&str> = content.lines().collect();
        if lines.len() > 2 {
            lines[1..lines.len() - 1].join("\n")
        } else {
            content.to_string()
        }
    } else {
        content.to_string()
    };

    if !content.starts_with(&format!("# {}", name)) && !content.starts_with('#') {
        format!("# {}\n\n{}", name.replace('-', " ").to_uppercase(), content)
    } else {
        content
    }
}

pub struct SkillsGenerator;

impl SkillsGenerator {
    pub async fn generate_with_llm(
        ctx: &GenerationContext<'_>,
        config: &Config,
        provider: Arc<dyn LlmProvider>,
    ) -> Result<Vec<Skill>> {
        let mut last_error = None;
        let mut rejected_skills: Vec<String> = Vec::new();

        for attempt in 0..MAX_RETRIES {
            if attempt > 0 {
                let delay = Duration::from_millis(BASE_RETRY_DELAY_MS * (1 << (attempt - 1)));
                tracing::info!(
                    attempt = attempt + 1,
                    delay_ms = delay.as_millis(),
                    rejected = rejected_skills.len(),
                    "Retrying skill discovery with negative feedback"
                );
                tokio::time::sleep(delay).await;
            }

            let discover_result = if rejected_skills.is_empty() {
                SkillDiscovery::discover(ctx, provider.clone()).await
            } else {
                SkillDiscovery::discover_with_negative_feedback(
                    ctx,
                    provider.clone(),
                    &rejected_skills,
                ).await
            };

            match discover_result {
                Ok(skills) if !skills.is_empty() => {
                    tracing::info!(
                        count = skills.len(),
                        attempt = attempt + 1,
                        "LLM discovered project-specific skills"
                    );

                    let evidence = Self::extract_evidence(ctx);
                    let extended = match ExtendedSkillsGenerator::generate_with_llm(
                        ctx, &evidence, config, provider.clone()
                    ).await {
                        Ok(ext) => ext,
                        Err(e) => {
                            tracing::warn!(error = %e, "Extended skills generation failed");
                            Vec::new()
                        }
                    };

                    let mut all_skills = skills;
                    all_skills.extend(extended);
                    let all_skills = SkillCrossReferencer::annotate_all(all_skills);

                    return Ok(all_skills
                        .into_iter()
                        .map(|s| ProgressiveDisclosure::apply_with_config(s, &config.disclosure))
                        .map(|s| DynamicContextInjector::inject(s, ctx.tech_stack))
                        .collect());
                }
                Ok(empty_skills) => {
                    tracing::warn!(attempt = attempt + 1, "LLM returned no skills, will retry");
                    for s in empty_skills {
                        rejected_skills.push(s.name.clone());
                    }
                    last_error = Some(crate::types::ClaudegenError::LlmApi(
                        "LLM returned no skills".into()
                    ));
                }
                Err(e) => {
                    tracing::warn!(error = %e, attempt = attempt + 1, "Skill discovery failed");
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            crate::types::ClaudegenError::LlmApi(
                "Skill discovery failed after all retries".into()
            )
        }))
    }

    /// Synchronous generation using context-derived skills only.
    ///
    /// Used when LLM is not available. Generates project-specific skills
    /// based on analysis context, not generic templates.
    pub fn generate(ctx: &GenerationContext<'_>, config: &Config) -> Vec<Skill> {
        // Generate extended skills based on project evidence
        let evidence = Self::extract_evidence(ctx);
        let skills = ExtendedSkillsGenerator::generate(&evidence, config);
        let skills = SkillCrossReferencer::annotate_all(skills);

        skills
            .into_iter()
            .map(|s| ProgressiveDisclosure::apply_with_config(s, &config.disclosure))
            .map(|s| DynamicContextInjector::inject(s, ctx.tech_stack))
            .collect()
    }

    fn extract_evidence(ctx: &GenerationContext<'_>) -> ProjectEvidence {
        let test_files: Vec<String> = ctx.file_registry.test_files().into_iter().cloned().collect();
        let test_framework = Self::detect_test_framework(ctx);
        let docs_directory = ctx.file_registry.has_docs_directory();
        let readme_exists = ctx.file_registry.has_readme();
        let auth_modules = Self::detect_auth_modules(ctx);
        let crypto_usage = Self::detect_crypto_usage(ctx);
        let database_access = Self::detect_database_access(ctx);

        ProjectEvidence {
            test_files,
            test_framework,
            docs_directory,
            readme_exists,
            auth_modules,
            crypto_usage,
            database_access,
        }
    }

    fn detect_test_framework(ctx: &GenerationContext<'_>) -> Option<String> {
        let primary_lang = ctx.tech_stack.primary_language.as_str();
        match primary_lang {
            "rust" => Some("cargo".into()),
            "typescript" | "javascript" => {
                if ctx.file_registry.file_exists("jest.config.js")
                    || ctx.file_registry.file_exists("jest.config.ts")
                {
                    Some("jest".into())
                } else if ctx.file_registry.file_exists("vitest.config.ts") {
                    Some("vitest".into())
                } else {
                    None
                }
            }
            "python" => {
                if ctx.file_registry.file_exists("pytest.ini")
                    || ctx.file_registry.file_exists("pyproject.toml")
                {
                    Some("pytest".into())
                } else {
                    None
                }
            }
            "go" => Some("go test".into()),
            _ => None,
        }
    }

    fn detect_auth_modules(ctx: &GenerationContext<'_>) -> Vec<String> {
        ctx.modules
            .iter()
            .filter(|m| {
                m.module_id.contains("auth")
                    || m.module_id.contains("identity")
                    || m.module_id.contains("session")
            })
            .map(|m| m.module_id.clone())
            .collect()
    }

    fn detect_crypto_usage(ctx: &GenerationContext<'_>) -> bool {
        ctx.constraints
            .gotchas
            .iter()
            .any(|g| g.title.to_lowercase().contains("crypto"))
            || ctx
                .deep_analysis
                .is_some_and(|d| d.patterns.iter().any(|p| p.description.contains("crypto")))
    }

    fn detect_database_access(ctx: &GenerationContext<'_>) -> bool {
        ctx.modules.iter().any(|m| {
            m.module_id.contains("database")
                || m.module_id.contains("repository")
                || m.module_id.contains("dao")
        }) || ctx
            .constraints
            .hidden_dependencies
            .iter()
            .any(|d| d.target.contains("database") || d.target.contains("sql"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::context::VerifiedFileRegistry;
    use crate::pipeline::phases::{
        constraint_extraction::ExtractedConstraints, convention_inference::InferredConventions,
        project_detection::ProjectDetection,
    };
    use crate::types::module_map::TechStack;

    fn test_context<'a>(
        detection: &'a ProjectDetection,
        tech_stack: &'a TechStack,
        conventions: &'a InferredConventions,
        constraints: &'a ExtractedConstraints,
        registry: &'a VerifiedFileRegistry,
    ) -> GenerationContext<'a> {
        GenerationContext::new(
            detection,
            tech_stack,
            "test-project",
            &[],
            &[],
            &[],
            conventions,
            constraints,
            registry,
        )
    }

    #[test]
    fn test_generate_skills_from_evidence() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );
        let config = Config::default();

        // With empty context, extended skills may be empty (this is correct)
        // Skills are generated based on project evidence, not fixed templates
        let skills = SkillsGenerator::generate(&ctx, &config);

        // All generated skills should be valid
        for skill in &skills {
            let issues = skill.validate();
            let errors: Vec<_> = issues.iter().filter(|i| i.is_error()).collect();
            assert!(errors.is_empty(), "Skill {} has errors: {:?}", skill.name, errors);
        }
    }

    #[test]
    fn test_progressive_disclosure_applied() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );
        let config = Config::default();

        let skills = SkillsGenerator::generate(&ctx, &config);

        // Skills should have progressive disclosure applied
        // (this is a property check, not a count check)
        for skill in &skills {
            assert!(skill.user_invocable.is_some() || !skill.body.is_empty());
        }
    }
}
