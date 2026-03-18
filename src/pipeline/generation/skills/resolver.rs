//! Module-level skill resolution based on evidence patterns.
//!
//! Determines which skills are most relevant for a module based on:
//! - Path patterns (auth/, db/, api/, etc.)
//! - Module responsibility text
//!
//! This follows claudegen's "No generation without evidence" principle,
//! using file system patterns instead of LLM calls for deterministic,
//! cost-free skill resolution.
//!
//! ## Design Notes
//!
//! - **Pattern overlap is intentional**: "jwt" and "token" appear in both
//!   security and crypto patterns because JWT/token modules ARE both
//!   security-critical AND cryptographically sensitive.
//! - **Asymmetric checking**: `is_test_related` and `is_docs_related` only
//!   check paths (not responsibility) because test/docs modules are reliably
//!   identified by directory patterns alone.
//! - **No `plan` skill**: Planning is an architect-level function. Module
//!   specialists execute plans created by the architect agent.

use std::collections::HashSet;

use crate::config::SkillMappingConfig;
use crate::types::module_map::Module;

pub struct ModuleSkillResolver;

impl ModuleSkillResolver {
    pub fn resolve(
        module: &Module,
        available_skills: &[String],
        config: &SkillMappingConfig,
    ) -> Vec<String> {
        let candidate_names = Self::resolve_candidate_names(module, config);

        if available_skills.is_empty() {
            return candidate_names;
        }

        let mut seen = HashSet::new();
        let mut matched = Vec::new();
        for candidate in &candidate_names {
            let candidate_lower = candidate.to_lowercase();
            if let Some(found) = available_skills.iter().find(|s| {
                let s_lower = s.to_lowercase();
                s_lower == candidate_lower || s_lower.contains(&candidate_lower)
            })
                && seen.insert(found.to_lowercase())
            {
                matched.push(found.clone());
            }
        }

        if matched.is_empty()
            && let Some(impl_skill) = available_skills
                .iter()
                .find(|s| s.to_lowercase().contains("implement"))
        {
            matched.push(impl_skill.clone());
        }

        matched
    }

    fn resolve_candidate_names(module: &Module, config: &SkillMappingConfig) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut skills = Vec::new();

        let mut push = |s: &str| {
            if seen.insert(s.to_string()) {
                skills.push(s.to_string());
            }
        };

        push("implement");

        let paths_lower = module.paths.join(" ").to_lowercase();
        let responsibility_lower = module.responsibility.to_lowercase();
        let combined = format!("{} {}", paths_lower, responsibility_lower);

        for entry in &config.tag_skill_map {
            if entry.tags.iter().any(|tag: &String| combined.contains(tag.as_str())) {
                for keyword in &entry.skills {
                    push(keyword);
                }
            }
        }

        skills
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_module(id: &str, paths: Vec<&str>, responsibility: &str) -> Module {
        use crate::types::module_map::ModuleMetrics;

        Module {
            id: id.into(),
            name: id.into(),
            paths: paths.into_iter().map(String::from).collect(),
            key_files: vec![],
            dependencies: vec![],
            dependents: vec![],
            responsibility: responsibility.into(),
            primary_language: "Rust".into(),
            metrics: ModuleMetrics::default(),
            conventions: vec![],
            known_issues: vec![],
            evidence: vec![],
        }
    }

    #[test]
    fn test_default_module_gets_implement() {
        let module = test_module("utils", vec!["src/utils/"], "General utilities");
        let skills = ModuleSkillResolver::resolve(&module, &[], &SkillMappingConfig::default());

        assert_eq!(skills, vec!["implement".to_string()]);
    }

    #[test]
    fn test_auth_module_gets_security_skills() {
        let module = test_module("auth", vec!["src/auth/"], "User authentication");
        let skills = ModuleSkillResolver::resolve(&module, &[], &SkillMappingConfig::default());

        assert!(skills.contains(&"implement".to_string()));
        assert!(skills.contains(&"security-audit".to_string()));
    }

    #[test]
    fn test_crypto_module_gets_security_audit() {
        let module = test_module("crypto", vec!["src/crypto/"], "Encryption utilities");
        let skills = ModuleSkillResolver::resolve(&module, &[], &SkillMappingConfig::default());

        assert!(skills.contains(&"security-audit".to_string()));
    }

    #[test]
    fn test_db_module_gets_implement() {
        let module = test_module("db", vec!["src/db/"], "Database access layer");
        let skills = ModuleSkillResolver::resolve(&module, &[], &SkillMappingConfig::default());

        assert!(skills.contains(&"implement".to_string()));
    }

    #[test]
    fn test_api_module_gets_code_review() {
        let module = test_module("api", vec!["src/api/"], "REST API handlers");
        let skills = ModuleSkillResolver::resolve(&module, &[], &SkillMappingConfig::default());

        assert!(skills.contains(&"code-review".to_string()));
    }

    #[test]
    fn test_test_module_gets_test_skill() {
        let module = test_module("tests", vec!["tests/"], "Integration tests");
        let skills = ModuleSkillResolver::resolve(&module, &[], &SkillMappingConfig::default());

        assert!(skills.contains(&"test".to_string()));
    }

    #[test]
    fn test_docs_module_gets_document_skill() {
        let module = test_module("docs", vec!["docs/"], "Documentation");
        let skills = ModuleSkillResolver::resolve(&module, &[], &SkillMappingConfig::default());

        assert!(skills.contains(&"document".to_string()));
    }

    #[test]
    fn test_responsibility_based_detection() {
        let module = test_module(
            "core",
            vec!["src/core/"],
            "Handles database connections and query execution",
        );
        let skills = ModuleSkillResolver::resolve(&module, &[], &SkillMappingConfig::default());

        // "database" tag triggers "implement" skill
        assert!(skills.contains(&"implement".to_string()));
    }

    #[test]
    fn test_combined_patterns() {
        let module = test_module(
            "auth-api",
            vec!["src/api/auth/"],
            "Authentication API endpoints with JWT token handling",
        );
        let skills = ModuleSkillResolver::resolve(&module, &[], &SkillMappingConfig::default());

        assert!(skills.contains(&"implement".to_string()));
        assert!(skills.contains(&"security-audit".to_string())); // auth + jwt/token
        assert!(skills.contains(&"code-review".to_string())); // api
    }

    #[test]
    fn test_repository_module() {
        let module = test_module(
            "user-repository",
            vec!["src/repository/user.rs"],
            "User data persistence",
        );
        let skills = ModuleSkillResolver::resolve(&module, &[], &SkillMappingConfig::default());

        // "repository" tag triggers "implement" skill
        assert!(skills.contains(&"implement".to_string()));
    }

    #[test]
    fn test_workflow_module_gets_implement() {
        let module = test_module(
            "scheduler",
            vec!["src/scheduler/"],
            "Job scheduling and orchestration",
        );
        let skills = ModuleSkillResolver::resolve(&module, &[], &SkillMappingConfig::default());

        // No matching tags, so only default "implement"
        assert!(skills.contains(&"implement".to_string()));
    }

    #[test]
    fn test_ml_module_gets_implement() {
        let module = test_module(
            "ml-pipeline",
            vec!["src/ml/"],
            "Model training and inference",
        );
        let skills = ModuleSkillResolver::resolve(&module, &[], &SkillMappingConfig::default());

        // No matching tags, so only default "implement"
        assert!(skills.contains(&"implement".to_string()));
    }

    #[test]
    fn test_payment_module_gets_implement() {
        let module = test_module(
            "billing",
            vec!["src/billing/"],
            "Payment processing and invoicing",
        );
        let skills = ModuleSkillResolver::resolve(&module, &[], &SkillMappingConfig::default());

        // No matching tags for billing, so only default "implement"
        assert!(skills.contains(&"implement".to_string()));
    }

    #[test]
    fn test_compliance_module_gets_implement() {
        let module = test_module(
            "compliance",
            vec!["src/compliance/"],
            "GDPR and regulatory compliance",
        );
        let skills = ModuleSkillResolver::resolve(&module, &[], &SkillMappingConfig::default());

        // No matching tags for compliance, so only default "implement"
        assert!(skills.contains(&"implement".to_string()));
    }

    #[test]
    fn test_resolve_with_available_skills() {
        let module = test_module("auth", vec!["src/auth/"], "User authentication");
        let available = vec![
            "implement-feature".to_string(),
            "code-review".to_string(),
            "security-audit".to_string(),
        ];
        let skills = ModuleSkillResolver::resolve(&module, &available, &SkillMappingConfig::default());

        assert!(!skills.is_empty());
        // Should match "implement-feature" via "implement" candidate
        assert!(skills.contains(&"implement-feature".to_string()));
        // Should match "security-audit" via "security-audit" candidate
        assert!(skills.contains(&"security-audit".to_string()));
    }

    #[test]
    fn test_resolve_no_available_skills_fallback() {
        let module = test_module("api", vec!["src/api/"], "REST API");
        let skills = ModuleSkillResolver::resolve(&module, &[], &SkillMappingConfig::default());

        // Should return candidate names directly when no available skills
        assert!(skills.contains(&"implement".to_string()));
        assert!(skills.contains(&"code-review".to_string()));
    }

    #[test]
    fn test_no_duplicate_skills() {
        let module = test_module(
            "secure-payments",
            vec!["src/payments/auth/"],
            "Secure payment processing with encryption",
        );
        let skills = ModuleSkillResolver::resolve(&module, &[], &SkillMappingConfig::default());

        let mut seen = std::collections::HashSet::new();
        for skill in &skills {
            assert!(
                seen.insert(skill),
                "Duplicate skill found: {}",
                skill
            );
        }
    }
}
