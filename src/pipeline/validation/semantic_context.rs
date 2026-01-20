//! Semantic Context Validator (Layer 2)
//!
//! Validates that claims in artifacts match the referenced code context.
//! For each @file:line reference, reads the surrounding code and uses LLM
//! to assess whether the claim is supported by the actual code.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::fs;
use tracing::{debug, warn};

use crate::ai::{with_timeout, LlmProvider};
use crate::config::SemanticContextValidationConfig;

/// Default timeout for LLM validation calls (30 seconds)
const LLM_VALIDATION_TIMEOUT_SECS: u64 = 30;

use crate::pipeline::context::VerifiedFileRegistry;
use crate::types::Result;

use super::layers::{IssueCode, IssueSeverity, LayerResult, ValidationIssue, ValidationLayer};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimContext {
    pub claim: String,
    pub file_path: String,
    pub line_number: usize,
    pub code_context: String,
    pub artifact_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMatch {
    pub claim: ClaimContext,
    pub similarity: f32,
    pub supported: bool,
    pub reasoning: String,
}

#[derive(Debug, Clone, Default)]
pub struct SemanticContextResult {
    pub passed: bool,
    pub total_claims: usize,
    pub supported_claims: usize,
    pub unsupported_claims: usize,
    pub matches: Vec<ContextMatch>,
    pub issues: Vec<ValidationIssue>,
}

pub struct SemanticContextValidator {
    provider: Arc<dyn LlmProvider>,
    config: SemanticContextValidationConfig,
    file_registry: VerifiedFileRegistry,
    project_root: std::path::PathBuf,
    context_cache: HashMap<String, String>,
}

impl SemanticContextValidator {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        config: SemanticContextValidationConfig,
        file_registry: VerifiedFileRegistry,
        project_root: impl AsRef<Path>,
    ) -> Self {
        Self {
            provider,
            config,
            file_registry,
            project_root: project_root.as_ref().to_path_buf(),
            context_cache: HashMap::new(),
        }
    }

    pub async fn validate(&mut self, artifacts: &[(String, String)]) -> Result<LayerResult> {
        if !self.config.enabled {
            return Ok(LayerResult::pass(ValidationLayer::SemanticContext));
        }

        let mut all_matches = Vec::new();
        let mut all_issues = Vec::new();
        let mut total_claims = 0;
        let mut supported_claims = 0;

        for (name, content) in artifacts {
            let claims = self.extract_claims_with_refs(name, content);
            let claims_to_validate: Vec<_> = claims
                .into_iter()
                .take(self.config.max_refs_per_artifact)
                .collect();

            for claim in claims_to_validate {
                total_claims += 1;

                match self.validate_claim(&claim).await {
                    Ok(match_result) => {
                        if match_result.supported {
                            supported_claims += 1;
                        } else if match_result.similarity < self.config.min_similarity {
                            all_issues.push(
                                ValidationIssue::error(
                                    ValidationLayer::SemanticContext,
                                    &claim.artifact_name,
                                    IssueCode::ClaimContextMismatch,
                                    format!(
                                        "Claim not supported by code context (similarity: {:.2})",
                                        match_result.similarity
                                    ),
                                )
                                .with_location(&format!("@{}:{}", claim.file_path, claim.line_number))
                                .with_suggestion(&match_result.reasoning),
                            );
                        }
                        all_matches.push(match_result);
                    }
                    Err(e) => {
                        warn!(
                            claim = %claim.claim,
                            error = %e,
                            "Failed to validate claim"
                        );
                        all_issues.push(
                            ValidationIssue::warning(
                                ValidationLayer::SemanticContext,
                                &claim.artifact_name,
                                IssueCode::LlmValidationFailed,
                                format!("Could not validate claim: {}", e),
                            )
                            .with_location(&format!("@{}:{}", claim.file_path, claim.line_number)),
                        );
                    }
                }
            }
        }

        // Pass if no error-level issues (warnings are allowed)
        let no_errors = all_issues.iter().all(|i| i.severity != IssueSeverity::Error);
        let score = if total_claims > 0 {
            supported_claims as f32 / total_claims as f32
        } else {
            1.0
        };

        if no_errors {
            Ok(LayerResult::pass(ValidationLayer::SemanticContext)
                .with_score(score)
                .with_issues(all_issues) // Include warnings in pass result
                .with_metadata("total_claims", total_claims.to_string())
                .with_metadata("supported_claims", supported_claims.to_string()))
        } else {
            Ok(LayerResult::fail(ValidationLayer::SemanticContext, all_issues)
                .with_score(score)
                .with_metadata("total_claims", total_claims.to_string())
                .with_metadata("supported_claims", supported_claims.to_string()))
        }
    }

    fn extract_claims_with_refs(&self, artifact_name: &str, content: &str) -> Vec<ClaimContext> {
        let ref_pattern = Regex::new(r"@([a-zA-Z0-9_./\-]+):(\d+)").expect("Invalid regex");
        let mut claims = Vec::new();

        let lines: Vec<&str> = content.lines().collect();

        for (idx, line) in lines.iter().enumerate() {
            if let Some(cap) = ref_pattern.captures(line) {
                let file_path = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                let line_num: usize = cap
                    .get(2)
                    .and_then(|m| m.as_str().parse().ok())
                    .unwrap_or(1);

                if file_path.is_empty() || file_path.starts_with("http") {
                    continue;
                }

                let claim = self.extract_claim_text(&lines, idx);
                if !claim.is_empty() {
                    claims.push(ClaimContext {
                        claim,
                        file_path: file_path.to_string(),
                        line_number: line_num,
                        code_context: String::new(),
                        artifact_name: artifact_name.to_string(),
                    });
                }
            }
        }

        claims
    }

    fn extract_claim_text(&self, lines: &[&str], ref_line_idx: usize) -> String {
        let mut claim_parts = Vec::new();

        if ref_line_idx > 0 {
            let prev = lines[ref_line_idx - 1].trim();
            if !prev.is_empty() && !prev.starts_with('#') && !prev.starts_with('-') {
                claim_parts.push(prev);
            }
        }

        let current = lines[ref_line_idx].trim();
        let ref_pattern = Regex::new(r"@[a-zA-Z0-9_./\-]+:\d+").expect("Invalid regex");
        let cleaned = ref_pattern.replace_all(current, "").trim().to_string();
        if !cleaned.is_empty() {
            claim_parts.push(&cleaned);
        }

        claim_parts.join(" ")
    }

    async fn validate_claim(&mut self, claim: &ClaimContext) -> Result<ContextMatch> {
        let context = self.get_code_context(&claim.file_path, claim.line_number).await?;

        if context.is_empty() {
            return Ok(ContextMatch {
                claim: claim.clone(),
                similarity: 0.0,
                supported: false,
                reasoning: "Could not read code context".to_string(),
            });
        }

        let prompt = format!(
            r#"Analyze if the following claim is supported by the code context.

CLAIM: "{claim}"

CODE CONTEXT (from {file}:{line}):
```
{context}
```

Evaluate:
1. Does the code context support or contradict the claim?
2. Is the claim accurate based on what's in the code?
3. Rate similarity/support from 0.0 (contradicts) to 1.0 (strongly supports)

Respond in JSON:
{{
  "supported": true/false,
  "similarity": 0.0-1.0,
  "reasoning": "brief explanation"
}}"#,
            claim = claim.claim,
            file = claim.file_path,
            line = claim.line_number,
            context = context
        );

        let schema = json!({
            "type": "object",
            "properties": {
                "supported": { "type": "boolean" },
                "similarity": { "type": "number" },
                "reasoning": { "type": "string" }
            },
            "required": ["supported", "similarity", "reasoning"]
        });

        let timeout = Duration::from_secs(LLM_VALIDATION_TIMEOUT_SECS);
        let response = with_timeout(
            timeout,
            self.provider.generate(&prompt, &schema),
            "semantic_context_validation",
        )
        .await?;

        #[derive(Deserialize)]
        struct LlmResponse {
            supported: bool,
            similarity: f32,
            reasoning: String,
        }

        let parsed: LlmResponse = serde_json::from_value(response.content)?;

        Ok(ContextMatch {
            claim: ClaimContext {
                code_context: context,
                ..claim.clone()
            },
            similarity: parsed.similarity,
            supported: parsed.supported,
            reasoning: parsed.reasoning,
        })
    }

    async fn get_code_context(&mut self, file_path: &str, line_number: usize) -> Result<String> {
        let cache_key = format!("{}:{}", file_path, line_number);

        if self.config.cache_context {
            if let Some(cached) = self.context_cache.get(&cache_key) {
                return Ok(cached.clone());
            }
        }

        let resolved = self.resolve_path(file_path);
        let full_path = match resolved {
            Some(p) => p,
            None => {
                debug!(file = %file_path, "File not found in registry");
                return Ok(String::new());
            }
        };

        let content = match fs::read_to_string(&full_path).await {
            Ok(c) => c,
            Err(e) => {
                debug!(file = %file_path, error = %e, "Failed to read file");
                return Ok(String::new());
            }
        };

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        if line_number == 0 || line_number > total_lines {
            return Ok(String::new());
        }

        let context_size = self.config.context_lines;
        let start = line_number.saturating_sub(context_size + 1);
        let end = (line_number + context_size).min(total_lines);

        let context: String = lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:4} | {}", start + i + 1, line))
            .collect::<Vec<_>>()
            .join("\n");

        if self.config.cache_context {
            self.context_cache.insert(cache_key, context.clone());
        }

        Ok(context)
    }

    fn resolve_path(&self, file_path: &str) -> Option<std::path::PathBuf> {
        let candidates = [
            self.project_root.join(file_path),
            self.project_root.join("src").join(file_path),
        ];

        for candidate in candidates {
            if candidate.exists() {
                return Some(candidate);
            }
        }

        if self.file_registry.contains(file_path) {
            return Some(self.project_root.join(file_path));
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_claims_basic() {
        let content = r#"
The LlmProvider trait defines the core interface.
See @src/ai/provider/mod.rs:42 for implementation.
"#;

        let validator = create_test_validator();
        let claims = validator.extract_claims_with_refs("test", content);

        assert_eq!(claims.len(), 1);
        assert!(claims[0].claim.contains("LlmProvider"));
        assert_eq!(claims[0].file_path, "src/ai/provider/mod.rs");
        assert_eq!(claims[0].line_number, 42);
    }

    fn create_test_validator() -> SemanticContextValidator {
        use crate::pipeline::context::VerifiedFileRegistry;
        use serde_json::Value;
        use std::sync::Arc;

        struct MockProvider;

        #[async_trait::async_trait]
        impl LlmProvider for MockProvider {
            async fn generate(
                &self,
                _prompt: &str,
                _schema: &Value,
            ) -> crate::types::Result<crate::ai::LlmResponse> {
                Ok(crate::ai::LlmResponse::content_only(json!({
                    "supported": true,
                    "similarity": 0.9,
                    "reasoning": "test"
                })))
            }
            fn name(&self) -> &str {
                "mock"
            }
            fn model(&self) -> &str {
                "mock"
            }
            async fn health_check(&self) -> crate::types::Result<bool> {
                Ok(true)
            }
        }

        SemanticContextValidator::new(
            Arc::new(MockProvider),
            SemanticContextValidationConfig::default(),
            VerifiedFileRegistry::empty(),
            "/tmp",
        )
    }
}
