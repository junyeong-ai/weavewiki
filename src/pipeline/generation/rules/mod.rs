//! Hierarchical Rule Generation System
//!
//! Generates rules in priority order:
//! - Project (100): Global rules, always injected
//! - Tech (90): Language-specific rules by file extension
//! - Framework (85): Framework-specific rules by path/keyword
//! - Module (80): Module-specific rules by path prefix
//! - Group (70): Cross-module group rules
//! - Domain (60): Domain-specific rules by keyword trigger

mod domain;
mod framework;
mod group;
mod module;
mod project;
mod tech;

pub use domain::DomainRuleGenerator;
pub use framework::FrameworkRuleGenerator;
pub use group::GroupRuleGenerator;
pub use module::ModuleRuleGenerator;
pub use project::ProjectRuleGenerator;
pub use tech::TechRuleGenerator;

use crate::pipeline::phases::constraint_extraction::ExtractedConstraints;
use crate::pipeline::phases::convention_inference::InferredConventions;
use crate::pipeline::phases::project_detection::ProjectDetection;
use crate::types::module_map::{DetectedModule, ModuleGroup, TechStack};
use crate::types::Rule;

/// Context for rule generation containing all analysis results
pub struct RuleGenerationContext<'a> {
    pub detection: &'a ProjectDetection,
    pub conventions: &'a InferredConventions,
    pub constraints: &'a ExtractedConstraints,
    pub tech_stack: &'a TechStack,
    pub modules: &'a [DetectedModule],
    pub groups: &'a [ModuleGroup],
    pub project_name: &'a str,
}

/// Main rule generator that orchestrates all rule types
pub struct RulesGenerator;

impl RulesGenerator {
    /// Generate all rules in priority order
    pub fn generate(ctx: &RuleGenerationContext<'_>) -> Vec<Rule> {
        let mut rules = Vec::new();

        // Project rule (priority 100)
        if let Some(rule) = ProjectRuleGenerator::generate(ctx) {
            rules.push(rule);
        }

        // Tech rules (priority 90)
        rules.extend(TechRuleGenerator::generate(ctx));

        // Framework rules (priority 85)
        rules.extend(FrameworkRuleGenerator::generate(ctx));

        // Module rules (priority 80)
        rules.extend(ModuleRuleGenerator::generate(ctx));

        // Group rules (priority 70)
        rules.extend(GroupRuleGenerator::generate(ctx));

        // Domain rules (priority 60)
        rules.extend(DomainRuleGenerator::generate(ctx));

        // Sort by priority (highest first)
        rules.sort_by(|a, b| b.priority.cmp(&a.priority));

        rules
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::phases::convention_inference::{
        ArchitectureConvention, AsyncPattern, ErrorHandlingPattern, FileOrganization,
        NamingConventions, TestingConvention,
    };
    use crate::pipeline::phases::project_detection::LanguageInfo;
    use crate::types::module_map::Convention;

    fn create_test_context<'a>(
        detection: &'a ProjectDetection,
        conventions: &'a InferredConventions,
        constraints: &'a ExtractedConstraints,
        tech_stack: &'a TechStack,
        modules: &'a [DetectedModule],
        groups: &'a [ModuleGroup],
    ) -> RuleGenerationContext<'a> {
        RuleGenerationContext {
            detection,
            conventions,
            constraints,
            tech_stack,
            modules,
            groups,
            project_name: "test-project",
        }
    }

    #[test]
    fn test_rules_sorted_by_priority() {
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
        let modules = vec![DetectedModule::new("auth", "Authentication module")
            .with_paths(vec!["src/auth/".into()])
            .with_conventions(vec![Convention::new("secure-defaults", "Use secure defaults")])];
        let groups = vec![];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rules = RulesGenerator::generate(&ctx);

        // Verify rules are sorted by priority (highest first)
        for window in rules.windows(2) {
            assert!(
                window[0].priority >= window[1].priority,
                "Rules should be sorted by priority"
            );
        }
    }
}
