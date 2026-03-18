//! Extended Skills Generator
//!
//! LLM-first generation of conditional skills with full project context.
//! Extended skills are generated when relevant project evidence is detected.

use std::sync::Arc;

use crate::pipeline::generation::context::GenerationContext;
use crate::ai::LlmProvider;
use crate::config::{Config, SkillGeneration};
use crate::types::{Result, Skill};
use crate::types::skill::ContextMode;

pub struct ExtendedSkillsGenerator;

#[derive(Debug, Clone, Default)]
pub struct ProjectEvidence {
    pub test_files: Vec<String>,
    pub test_framework: Option<String>,
    pub docs_directory: bool,
    pub readme_exists: bool,
    pub auth_modules: Vec<String>,
    pub crypto_usage: bool,
    pub database_access: bool,
}

impl ProjectEvidence {
    pub fn has_test_evidence(&self) -> bool {
        !self.test_files.is_empty()
    }

    pub fn has_document_evidence(&self) -> bool {
        self.docs_directory || self.readme_exists
    }

    pub fn has_security_evidence(&self) -> bool {
        !self.auth_modules.is_empty() || self.crypto_usage || self.database_access
    }
}

impl ExtendedSkillsGenerator {
    pub async fn generate_with_llm(
        ctx: &GenerationContext<'_>,
        evidence: &ProjectEvidence,
        config: &Config,
        provider: Arc<dyn LlmProvider>,
    ) -> Result<Vec<Skill>> {
        let mut skills = Vec::new();

        if Self::should_generate_test(evidence, config) {
            skills.push(Self::generate_test_skill(ctx, evidence, &provider).await?);
        }

        if Self::should_generate_document(evidence, config) {
            skills.push(Self::generate_document_skill(ctx, evidence, &provider).await?);
        }

        if Self::should_generate_security_audit(evidence, config) {
            skills.push(Self::generate_security_audit_skill(ctx, evidence, &provider).await?);
        }

        Ok(skills)
    }

    pub fn generate(evidence: &ProjectEvidence, config: &Config) -> Vec<Skill> {
        let mut skills = Vec::new();

        if Self::should_generate_test(evidence, config) {
            skills.push(Self::build_test_skill(evidence));
        }

        if Self::should_generate_document(evidence, config) {
            skills.push(Self::build_document_skill(evidence));
        }

        if Self::should_generate_security_audit(evidence, config) {
            skills.push(Self::build_security_audit_skill(evidence));
        }

        skills
    }

    fn should_generate_test(evidence: &ProjectEvidence, config: &Config) -> bool {
        match config.generation.skills.test {
            SkillGeneration::Auto => evidence.has_test_evidence(),
            SkillGeneration::Enabled => true,
            SkillGeneration::Disabled => false,
        }
    }

    fn should_generate_document(evidence: &ProjectEvidence, config: &Config) -> bool {
        match config.generation.skills.document {
            SkillGeneration::Auto => evidence.has_document_evidence(),
            SkillGeneration::Enabled => true,
            SkillGeneration::Disabled => false,
        }
    }

    fn should_generate_security_audit(evidence: &ProjectEvidence, config: &Config) -> bool {
        match config.generation.skills.security_audit {
            SkillGeneration::Auto => evidence.has_security_evidence(),
            SkillGeneration::Enabled => true,
            SkillGeneration::Disabled => false,
        }
    }

    async fn generate_test_skill(
        ctx: &GenerationContext<'_>,
        evidence: &ProjectEvidence,
        provider: &Arc<dyn LlmProvider>,
    ) -> Result<Skill> {
        let prompt = Self::build_test_prompt(ctx, evidence);
        let response = provider.generate(&prompt, &serde_json::json!({})).await?;
        let body = Self::extract_body(&response.content.to_string(), "test");

        Ok(Skill::new(
            "test",
            "Write and run tests for code. Use when asked to test, add coverage, or verify behavior.",
            &body,
        )
        .tools(vec![
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
            "Edit".into(),
            "Write".into(),
        ])
        .user_invocable(true)
        .argument_hint("<file-or-module-to-test>"))
    }

    async fn generate_document_skill(
        ctx: &GenerationContext<'_>,
        evidence: &ProjectEvidence,
        provider: &Arc<dyn LlmProvider>,
    ) -> Result<Skill> {
        let prompt = Self::build_document_prompt(ctx, evidence);
        let response = provider.generate(&prompt, &serde_json::json!({})).await?;
        let body = Self::extract_body(&response.content.to_string(), "document");

        Ok(Skill::new(
            "document",
            "Write or update documentation. Use when asked to document code, APIs, or features.",
            &body,
        )
        .tools(vec![
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
            "Edit".into(),
            "Write".into(),
        ])
        .user_invocable(true)
        .argument_hint("<file-or-feature-to-document>"))
    }

    async fn generate_security_audit_skill(
        ctx: &GenerationContext<'_>,
        evidence: &ProjectEvidence,
        provider: &Arc<dyn LlmProvider>,
    ) -> Result<Skill> {
        let prompt = Self::build_security_audit_prompt(ctx, evidence);
        let response = provider.generate(&prompt, &serde_json::json!({})).await?;
        let body = Self::extract_body(&response.content.to_string(), "security-audit");

        Ok(Skill::new(
            "security-audit",
            "Perform security audit on code. Use when explicitly asked to audit, scan for vulnerabilities, or review security.",
            &body,
        )
        .tools(vec!["Read".into(), "Grep".into(), "Glob".into()])
        .user_invocable(true)
        .disable_model_invocation(true)
        .context(ContextMode::Fork)
        .agent("Explore")
        .argument_hint("<scope-or-module-to-audit>"))
    }

    fn build_test_prompt(ctx: &GenerationContext<'_>, evidence: &ProjectEvidence) -> String {
        let framework = evidence
            .test_framework
            .as_deref()
            .unwrap_or("unknown");
        let test_files = evidence.test_files.join(", ");
        let patterns = ctx.format_patterns(&ctx.all_patterns());
        let tier3 = ctx.format_discovered_insights();
        let constraints = ctx.format_constraints();
        let domain = ctx.domain_knowledge();
        let domain_text = ctx.format_domain(&domain);

        format!(
            r#"Generate a project-specific test skill for Claude Code.

## PROJECT CONTEXT
- Test Framework: {framework}
- Existing Test Files: {test_files}
- Primary Language: {lang}

## TEST PATTERNS DISCOVERED
{patterns}

## CRITICAL GOTCHAS (Must address in tests)
{tier3}

## PROJECT CONSTRAINTS
{constraints}

## DOMAIN CONTEXT
{domain_text}

## REQUIREMENTS
1. Include specific test framework commands and patterns for {framework}
2. Reference discovered test patterns and conventions
3. Include Critical Gotchas as test cases to verify
4. Use @file:line references where available
5. Be prescriptive: "DO X" not "Consider X"

Return the skill body starting with # Test Skill."#,
            framework = framework,
            test_files = if test_files.is_empty() { "(none detected)".into() } else { test_files },
            lang = ctx.tech_stack.primary_language,
            patterns = if patterns.is_empty() { "(no patterns)".into() } else { patterns },
            tier3 = if tier3.is_empty() { "(no critical insights)".into() } else { tier3 },
            constraints = if constraints.is_empty() { "(no constraints)".into() } else { constraints },
            domain_text = if domain_text.is_empty() { "(no domain context)".into() } else { domain_text },
        )
    }

    fn build_document_prompt(ctx: &GenerationContext<'_>, evidence: &ProjectEvidence) -> String {
        let has_docs = evidence.docs_directory;
        let has_readme = evidence.readme_exists;
        let patterns = ctx.format_patterns(&ctx.all_patterns());
        let domain = ctx.domain_knowledge();
        let domain_text = ctx.format_domain(&domain);
        let files = ctx.all_files_with_context();
        let key_files: Vec<_> = files.iter().map(|f| format!("@{}", f.path)).collect();

        format!(
            r#"Generate a project-specific document skill for Claude Code.

## PROJECT CONTEXT
- Documentation Directory Exists: {has_docs}
- README Exists: {has_readme}
- Primary Language: {lang}

## KEY FILES TO DOCUMENT
{key_files}

## PROJECT PATTERNS
{patterns}

## DOMAIN CONTEXT
{domain_text}

## REQUIREMENTS
1. Include specific documentation format/tool for this project
2. Reference key files and their purposes
3. Include domain terminology and concepts
4. Use @file references where available
5. Specify documentation structure and standards

Return the skill body starting with # Document Skill."#,
            has_docs = has_docs,
            has_readme = has_readme,
            lang = ctx.tech_stack.primary_language,
            key_files = key_files.join("\n"),
            patterns = if patterns.is_empty() { "(no patterns)".into() } else { patterns },
            domain_text = if domain_text.is_empty() { "(no domain context)".into() } else { domain_text },
        )
    }

    fn build_security_audit_prompt(ctx: &GenerationContext<'_>, evidence: &ProjectEvidence) -> String {
        let auth_modules = evidence.auth_modules.join(", ");
        let has_crypto = evidence.crypto_usage;
        let has_db = evidence.database_access;
        let tier3 = ctx.format_discovered_insights();
        let constraints = ctx.format_constraints();
        let hidden_deps: Vec<_> = ctx
            .all_hidden_dependencies()
            .iter()
            .filter(|d| {
                d.from_module.contains("auth")
                    || d.to_module.contains("auth")
                    || d.description.to_lowercase().contains("security")
            })
            .map(|d| format!("{} → {}: {}", d.from_module, d.to_module, d.description))
            .collect();

        format!(
            r#"Generate a project-specific security audit skill for Claude Code.

## PROJECT SECURITY CONTEXT
- Auth Modules: {auth_modules}
- Crypto Usage: {has_crypto}
- Database Access: {has_db}
- Primary Language: {lang}

## SECURITY-RELATED DEPENDENCIES
{hidden_deps}

## CRITICAL SECURITY INSIGHTS
{tier3}

## PROJECT CONSTRAINTS
{constraints}

## REQUIREMENTS
1. Focus on actual security patterns found in this project
2. Include specific auth module review guidance
3. Reference discovered security constraints
4. Use @file:line references for security-sensitive code
5. Prioritize project-specific vulnerabilities over generic OWASP checklist

Return the skill body starting with # Security Audit Skill."#,
            auth_modules = if auth_modules.is_empty() { "(none detected)".into() } else { auth_modules },
            has_crypto = has_crypto,
            has_db = has_db,
            lang = ctx.tech_stack.primary_language,
            hidden_deps = if hidden_deps.is_empty() { "(none)".into() } else { hidden_deps.join("\n") },
            tier3 = if tier3.is_empty() { "(no critical insights)".into() } else { tier3 },
            constraints = if constraints.is_empty() { "(no constraints)".into() } else { constraints },
        )
    }

    fn extract_body(response: &str, name: &str) -> String {
        super::extract_skill_body(response, name)
    }

    /// Build template-based test skill when LLM is not available.
    fn build_test_skill(evidence: &ProjectEvidence) -> Skill {
        let framework = evidence.test_framework.as_deref().unwrap_or("test");
        let body = format!(
            r#"# Test Skill

## Framework
{framework}

## Process
1. Analyze target code
2. Design test cases (happy path, edge cases, error conditions)
3. Write tests following project conventions
4. Run tests and verify coverage

## Input
$ARGUMENTS"#,
            framework = framework
        );

        Skill::new(
            "test",
            "Write and run tests for code. Use when asked to test, add coverage, or verify behavior.",
            &body,
        )
        .tools(vec![
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
            "Edit".into(),
            "Write".into(),
        ])
        .user_invocable(true)
        .argument_hint("<file-or-module-to-test>")
    }

    /// Build template-based document skill when LLM is not available.
    fn build_document_skill(evidence: &ProjectEvidence) -> Skill {
        let doc_location = if evidence.docs_directory { "docs/" } else { "README.md" };
        let body = format!(
            r#"# Document Skill

## Documentation Location
{doc_location}

## Process
1. Identify documentation scope
2. Gather information from code
3. Write documentation following project conventions
4. Verify accuracy and completeness

## Input
$ARGUMENTS"#,
            doc_location = doc_location
        );

        Skill::new(
            "document",
            "Write or update documentation. Use when asked to document code, APIs, or features.",
            &body,
        )
        .tools(vec![
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
            "Edit".into(),
            "Write".into(),
        ])
        .user_invocable(true)
        .argument_hint("<file-or-feature-to-document>")
    }

    /// Build template-based security audit skill when LLM is not available.
    fn build_security_audit_skill(evidence: &ProjectEvidence) -> Skill {
        let focus_areas = [
            if !evidence.auth_modules.is_empty() { Some("Authentication/Authorization") } else { None },
            if evidence.crypto_usage { Some("Cryptography") } else { None },
            if evidence.database_access { Some("SQL Injection") } else { None },
        ]
        .iter()
        .filter_map(|&x| x)
        .collect::<Vec<_>>()
        .join(", ");

        let body = format!(
            r#"# Security Audit Skill

## Focus Areas
{focus_areas}

## Process
1. Identify attack surfaces
2. Check for common vulnerabilities
3. Review security-sensitive code
4. Report findings with severity

## Input
$ARGUMENTS"#,
            focus_areas = if focus_areas.is_empty() { "General security review".into() } else { focus_areas }
        );

        Skill::new(
            "security-audit",
            "Perform security audit on code. Use when explicitly asked to audit, scan for vulnerabilities, or review security.",
            &body,
        )
        .tools(vec!["Read".into(), "Grep".into(), "Glob".into()])
        .user_invocable(true)
        .disable_model_invocation(true)
        .context(ContextMode::Fork)
        .agent("Explore")
        .argument_hint("<scope-or-module-to-audit>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_evidence() -> ProjectEvidence {
        ProjectEvidence::default()
    }

    fn test_evidence() -> ProjectEvidence {
        ProjectEvidence {
            test_files: vec!["src/lib_test.rs".into()],
            test_framework: Some("cargo".into()),
            ..Default::default()
        }
    }

    fn doc_evidence() -> ProjectEvidence {
        ProjectEvidence {
            docs_directory: true,
            readme_exists: true,
            ..Default::default()
        }
    }

    fn security_evidence() -> ProjectEvidence {
        ProjectEvidence {
            auth_modules: vec!["src/auth".into()],
            crypto_usage: true,
            database_access: true,
            ..Default::default()
        }
    }

    fn full_evidence() -> ProjectEvidence {
        ProjectEvidence {
            test_files: vec!["tests/unit.rs".into()],
            test_framework: Some("cargo".into()),
            docs_directory: true,
            readme_exists: true,
            auth_modules: vec!["src/auth".into()],
            crypto_usage: true,
            database_access: true,
        }
    }

    #[test]
    fn test_empty_evidence_generates_nothing() {
        let config = Config::default();
        let skills = ExtendedSkillsGenerator::generate(&empty_evidence(), &config);
        assert!(skills.is_empty());
    }

    #[test]
    fn test_generates_test_skill_with_evidence() {
        let config = Config::default();
        let skills = ExtendedSkillsGenerator::generate(&test_evidence(), &config);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "test");
    }

    #[test]
    fn test_generates_document_skill_with_evidence() {
        let config = Config::default();
        let skills = ExtendedSkillsGenerator::generate(&doc_evidence(), &config);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "document");
    }

    #[test]
    fn test_generates_security_audit_with_evidence() {
        let config = Config::default();
        let skills = ExtendedSkillsGenerator::generate(&security_evidence(), &config);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "security-audit");
    }

    #[test]
    fn test_generates_all_skills_with_full_evidence() {
        let config = Config::default();
        let skills = ExtendedSkillsGenerator::generate(&full_evidence(), &config);
        assert_eq!(skills.len(), 3);

        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"test"));
        assert!(names.contains(&"document"));
        assert!(names.contains(&"security-audit"));
    }

    #[test]
    fn test_config_enabled_overrides_evidence() {
        let mut config = Config::default();
        config.generation.skills.test = SkillGeneration::Enabled;

        let skills = ExtendedSkillsGenerator::generate(&empty_evidence(), &config);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "test");
    }

    #[test]
    fn test_config_disabled_overrides_evidence() {
        let mut config = Config::default();
        config.generation.skills.test = SkillGeneration::Disabled;

        let skills = ExtendedSkillsGenerator::generate(&test_evidence(), &config);
        assert!(skills.is_empty());
    }

    #[test]
    fn test_evidence_helpers() {
        let evidence = full_evidence();
        assert!(evidence.has_test_evidence());
        assert!(evidence.has_document_evidence());
        assert!(evidence.has_security_evidence());

        let empty = empty_evidence();
        assert!(!empty.has_test_evidence());
        assert!(!empty.has_document_evidence());
        assert!(!empty.has_security_evidence());
    }

    #[test]
    fn test_template_skills_have_arguments() {
        let evidence = full_evidence();
        let config = Config::default();
        let skills = ExtendedSkillsGenerator::generate(&evidence, &config);

        for skill in &skills {
            assert!(
                skill.body.contains("$ARGUMENTS"),
                "Skill '{}' should contain $ARGUMENTS",
                skill.name
            );
        }
    }

    #[test]
    fn test_build_test_includes_framework() {
        let evidence = test_evidence();
        let skill = ExtendedSkillsGenerator::build_test_skill(&evidence);
        assert!(skill.body.contains("cargo"));
    }

    #[test]
    fn test_build_security_includes_focus_areas() {
        let evidence = security_evidence();
        let skill = ExtendedSkillsGenerator::build_security_audit_skill(&evidence);
        assert!(skill.body.contains("Authentication"));
        assert!(skill.body.contains("Cryptography"));
        assert!(skill.body.contains("SQL Injection"));
    }
}
