//! Tech/Language Rule Generator
//!
//! Generates language-specific rules (priority 90).
//! Matched by file extension patterns (e.g., "**/*.rs" for Rust).

use crate::pipeline::phases::convention_inference::AsyncStyle;
use crate::utils::capitalize_first;

use super::RuleGenerationContext;
use crate::types::Rule;

pub struct TechRuleGenerator;

impl TechRuleGenerator {
    pub fn generate(ctx: &RuleGenerationContext<'_>) -> Vec<Rule> {
        ctx.detection
            .languages
            .iter()
            .filter_map(|lang| Self::generate_for_language(ctx, &lang.language))
            .collect()
    }

    fn generate_for_language(ctx: &RuleGenerationContext<'_>, language: &str) -> Option<Rule> {
        let mut content = Vec::new();
        let paths = Self::language_paths(language);

        if paths.is_empty() {
            return None;
        }

        content.push(format!("# {} Conventions", capitalize_first(language)));
        content.push(String::new());

        // Error handling patterns
        let error = &ctx.conventions.error_handling;
        content.push("## Error Handling".into());
        content.push(String::new());
        content.push(format!("Style: {:?}", error.style));
        if !error.error_types.is_empty() {
            content.push(format!("Types: {}", error.error_types.join(", ")));
        }
        if !error.propagation_pattern.is_empty() {
            content.push(format!("Propagation: {}", error.propagation_pattern));
        }
        content.push(String::new());

        // Async patterns
        if ctx.conventions.async_pattern.style != AsyncStyle::Synchronous {
            content.push("## Async Patterns".into());
            content.push(String::new());
            content.push(format!("Style: {:?}", ctx.conventions.async_pattern.style));
            if let Some(runtime) = &ctx.conventions.async_pattern.runtime {
                content.push(format!("Runtime: {runtime}"));
            }
            content.push(String::new());
        }

        // Naming conventions
        let naming = &ctx.conventions.naming;
        content.push("## Naming".into());
        content.push(String::new());
        content.push(format!("- Files: {:?}", naming.file_naming.case));
        content.push(format!("- Types: {:?}", naming.type_naming.case));
        content.push(format!("- Functions: {:?}", naming.function_naming.case));
        content.push(String::new());

        // Testing conventions
        let testing = &ctx.conventions.testing;
        if testing.framework.is_some() || !testing.naming_pattern.is_empty() {
            content.push("## Testing".into());
            content.push(String::new());
            if let Some(framework) = &testing.framework {
                content.push(format!("Framework: {framework}"));
            }
            content.push(format!("Location: {:?}", testing.location));
            if !testing.naming_pattern.is_empty() {
                content.push(format!("Naming: {}", testing.naming_pattern));
            }
            content.push(String::new());
        }

        Some(Rule::tech(language, paths, content))
    }

    fn language_paths(language: &str) -> Vec<String> {
        match language.to_lowercase().as_str() {
            "rust" => vec!["**/*.rs".into()],
            "python" => vec!["**/*.py".into()],
            "typescript" => vec!["**/*.ts".into(), "**/*.tsx".into()],
            "javascript" => vec!["**/*.js".into(), "**/*.jsx".into()],
            "go" => vec!["**/*.go".into()],
            "java" => vec!["**/*.java".into()],
            "kotlin" => vec!["**/*.kt".into(), "**/*.kts".into()],
            "swift" => vec!["**/*.swift".into()],
            "ruby" => vec!["**/*.rb".into()],
            "php" => vec!["**/*.php".into()],
            "c" => vec!["**/*.c".into(), "**/*.h".into()],
            "cpp" | "c++" => vec!["**/*.cpp".into(), "**/*.hpp".into(), "**/*.cc".into()],
            "csharp" | "c#" => vec!["**/*.cs".into()],
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::phases::constraint_extraction::ExtractedConstraints;
    use crate::pipeline::phases::convention_inference::{
        ArchitectureConvention, AsyncPattern, ErrorHandlingPattern, FileOrganization,
        InferredConventions, NamingConventions, TestingConvention,
    };
    use crate::pipeline::phases::project_detection::{LanguageInfo, ProjectDetection};
    use crate::types::module_map::TechStack;

    #[test]
    fn test_tech_rule_generation() {
        let detection = ProjectDetection {
            languages: vec![LanguageInfo {
                language: "rust".into(),
                file_count: 50,
                percentage: 0.8,
                primary_manifest: Some("Cargo.toml".into()),
            }],
            ..Default::default()
        };
        let conventions = InferredConventions {
            architecture: ArchitectureConvention::default(),
            naming: NamingConventions::default(),
            file_organization: FileOrganization::default(),
            error_handling: ErrorHandlingPattern::default(),
            async_pattern: AsyncPattern::default(),
            patterns: Vec::new(),
            testing: TestingConvention::default(),
        };
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");
        let modules = vec![];
        let groups = vec![];

        let ctx = RuleGenerationContext {
            detection: &detection,
            conventions: &conventions,
            constraints: &constraints,
            tech_stack: &tech_stack,
            modules: &modules,
            groups: &groups,
            project_name: "test-project",
        };

        let rules = TechRuleGenerator::generate(&ctx);
        assert_eq!(rules.len(), 1);

        let rule = &rules[0];
        assert_eq!(rule.name, "rust");
        assert_eq!(rule.priority, 90);
        assert!(rule.paths.as_ref().unwrap().contains(&"**/*.rs".into()));
    }

    #[test]
    fn test_language_paths() {
        assert!(!TechRuleGenerator::language_paths("rust").is_empty());
        assert!(!TechRuleGenerator::language_paths("python").is_empty());
        assert!(!TechRuleGenerator::language_paths("typescript").is_empty());
        assert!(TechRuleGenerator::language_paths("unknown_lang").is_empty());
    }
}
