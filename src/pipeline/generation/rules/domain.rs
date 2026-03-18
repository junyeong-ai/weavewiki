//! Domain Rule Generator
//!
//! Generates domain-specific rules (priority 60) from project evidence.
//! Only emits rules when concrete evidence exists in RuleGenerationContext.

use super::RuleGenerationContext;
use crate::types::Rule;
use crate::utils::capitalize_first;

pub struct DomainRuleGenerator;

struct DomainDef {
    name: &'static str,
    triggers: &'static [&'static str],
}

const DOMAINS: &[DomainDef] = &[
    DomainDef {
        name: "security",
        triggers: &["auth", "password", "token", "credential", "encrypt", "hash", "secret"],
    },
    DomainDef {
        name: "error-handling",
        triggers: &["error", "Result", "Error", "exception", "panic", "unwrap"],
    },
    DomainDef {
        name: "concurrency",
        triggers: &["async", "await", "spawn", "mutex", "lock", "channel", "thread"],
    },
    DomainDef {
        name: "testing",
        triggers: &["test", "mock", "assert", "expect", "fixture"],
    },
    DomainDef {
        name: "performance",
        triggers: &["cache", "optimize", "benchmark", "latency", "throughput"],
    },
    DomainDef {
        name: "api",
        triggers: &[
            "endpoint", "route", "handler", "request", "response", "REST", "GraphQL",
            "API", "HttpResponse", "StatusCode", "Json", "Query", "Path",
        ],
    },
    DomainDef {
        name: "data",
        triggers: &[
            "model", "schema", "database", "db", "query", "migration", "ORM",
            "repository", "entity", "table", "column", "relation",
        ],
    },
    DomainDef {
        name: "logging",
        triggers: &[
            "log", "trace", "debug", "info", "warn", "error", "span", "metrics",
            "tracing", "logger", "slog", "log4j",
        ],
    },
];

impl DomainRuleGenerator {
    pub fn generate(ctx: &RuleGenerationContext<'_>) -> Vec<Rule> {
        DOMAINS
            .iter()
            .filter_map(|domain| Self::generate_for_domain(ctx, domain))
            .collect()
    }

    fn generate_for_domain(ctx: &RuleGenerationContext<'_>, domain: &DomainDef) -> Option<Rule> {
        let mut content = Vec::new();

        content.push(format!(
            "# Domain: {}",
            capitalize_first(domain.name.replace('-', " ").as_str())
        ));
        content.push(String::new());

        // Add evidence-based content from conventions
        match domain.name {
            "security" => Self::add_security_evidence(ctx, &mut content),
            "error-handling" => Self::add_error_handling_evidence(ctx, &mut content),
            "concurrency" => Self::add_concurrency_evidence(ctx, &mut content),
            "testing" => Self::add_testing_evidence(ctx, &mut content),
            _ => {}
        }

        // Add domain-related gotchas from constraints
        let domain_gotchas: Vec<_> = ctx
            .constraints
            .gotchas
            .iter()
            .filter(|g| {
                domain.triggers.iter().any(|t| {
                    g.title.to_lowercase().contains(&t.to_lowercase())
                        || g.description.to_lowercase().contains(&t.to_lowercase())
                })
            })
            .collect();

        if !domain_gotchas.is_empty() {
            content.push("## Gotchas".into());
            content.push(String::new());
            for gotcha in domain_gotchas {
                content.push(format!("### {}", gotcha.title));
                content.push(gotcha.description.clone());
                content.push(format!("**Solution**: {}", gotcha.solution));
                content.push(String::new());
            }
        }

        // Add domain-related anti-patterns from constraints
        let domain_anti_patterns: Vec<_> = ctx
            .constraints
            .anti_patterns
            .iter()
            .filter(|ap| {
                domain.triggers.iter().any(|t| {
                    ap.name.to_lowercase().contains(&t.to_lowercase())
                        || ap.description.to_lowercase().contains(&t.to_lowercase())
                })
            })
            .collect();

        if !domain_anti_patterns.is_empty() {
            content.push("## Anti-Patterns".into());
            content.push(String::new());
            for ap in domain_anti_patterns {
                content.push(format!("### {} (DON'T)", ap.name));
                content.push(ap.description.clone());
                content.push(format!("**Instead**: {}", ap.correct_approach));
                content.push(String::new());
            }
        }

        // Skip domains with no evidence (only header)
        if content.len() <= 2 {
            return None;
        }

        let triggers: Vec<String> = domain.triggers.iter().map(|s| (*s).into()).collect();
        Some(Rule::domain(domain.name, triggers, content))
    }

    fn add_security_evidence(ctx: &RuleGenerationContext<'_>, content: &mut Vec<String>) {
        let security_deps: Vec<_> = ctx
            .constraints
            .hidden_dependencies
            .iter()
            .filter(|d| {
                d.description.to_lowercase().contains("auth")
                    || d.description.to_lowercase().contains("security")
            })
            .collect();

        if !security_deps.is_empty() {
            content.push("## Security Dependencies".into());
            content.push(String::new());
            for dep in security_deps {
                content.push(format!("- {} -> {}: {}", dep.source, dep.target, dep.description));
            }
            content.push(String::new());
        }
    }

    fn add_error_handling_evidence(
        ctx: &RuleGenerationContext<'_>,
        content: &mut Vec<String>,
    ) {
        let error = &ctx.conventions.error_handling;

        content.push("## Style".into());
        content.push(String::new());
        content.push(format!("Error handling style: {:?}", error.style));
        content.push(String::new());

        if !error.error_types.is_empty() {
            content.push("## Error Types".into());
            content.push(String::new());
            for err_type in &error.error_types {
                content.push(format!("- `{err_type}`"));
            }
            content.push(String::new());
        }

        if !error.propagation_pattern.is_empty() {
            content.push("## Propagation".into());
            content.push(String::new());
            content.push(error.propagation_pattern.clone());
            content.push(String::new());
        }

        if !error.recovery_strategy.is_empty() {
            content.push("## Recovery Strategy".into());
            content.push(String::new());
            content.push(error.recovery_strategy.clone());
            content.push(String::new());
        }
    }

    fn add_concurrency_evidence(ctx: &RuleGenerationContext<'_>, content: &mut Vec<String>) {
        use crate::pipeline::phases::convention_inference::AsyncStyle;
        let async_pattern = &ctx.conventions.async_pattern;

        if async_pattern.style != AsyncStyle::Synchronous {
            content.push("## Async Style".into());
            content.push(String::new());
            content.push(format!("Pattern: {:?}", async_pattern.style));
            if let Some(runtime) = &async_pattern.runtime {
                content.push(format!("Runtime: {runtime}"));
            }
            content.push(String::new());
        }

        if !async_pattern.concurrency_patterns.is_empty() {
            content.push("## Concurrency Patterns".into());
            content.push(String::new());
            for pattern in &async_pattern.concurrency_patterns {
                content.push(format!("- {pattern}"));
            }
            content.push(String::new());
        }
    }

    fn add_testing_evidence(ctx: &RuleGenerationContext<'_>, content: &mut Vec<String>) {
        let testing = &ctx.conventions.testing;

        if let Some(framework) = &testing.framework {
            content.push("## Framework".into());
            content.push(String::new());
            content.push(format!("Primary: {framework}"));
            content.push(String::new());
        }

        content.push("## Test Location".into());
        content.push(String::new());
        content.push(format!("{:?}", testing.location));
        content.push(String::new());

        if !testing.naming_pattern.is_empty() {
            content.push("## Naming".into());
            content.push(String::new());
            content.push(format!("Pattern: {}", testing.naming_pattern));
            content.push(String::new());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::phases::constraint_extraction::{
        AntiPattern, ExtractedConstraints, Gotcha,
    };
    use crate::pipeline::phases::convention_inference::{
        ArchitectureConvention, AsyncPattern, AsyncStyle, ErrorHandlingPattern, ErrorStyle,
        FileOrganization, InferredConventions, NamingConventions, TestingConvention,
    };
    use crate::pipeline::phases::project_detection::ProjectDetection;
    use crate::types::module_map::TechStack;

    fn make_ctx<'a>(
        detection: &'a ProjectDetection,
        conventions: &'a InferredConventions,
        constraints: &'a ExtractedConstraints,
        tech_stack: &'a TechStack,
    ) -> RuleGenerationContext<'a> {
        RuleGenerationContext {
            detection,
            conventions,
            constraints,
            tech_stack,
            modules: &[],
            groups: &[],
            project_name: "test-project",
        }
    }

    #[test]
    fn test_error_handling_evidence() {
        let detection = ProjectDetection::default();
        let conventions = InferredConventions {
            architecture: ArchitectureConvention::default(),
            naming: NamingConventions::default(),
            file_organization: FileOrganization::default(),
            error_handling: ErrorHandlingPattern {
                style: ErrorStyle::ResultType,
                result_count: 50,
                exception_count: 0,
                error_types: vec!["AppError".into()],
                propagation_pattern: "Use ? operator".into(),
                recovery_strategy: "Log and return default".into(),
            },
            async_pattern: AsyncPattern::default(),
            patterns: Vec::new(),
            testing: TestingConvention::default(),
        };
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");

        let ctx = make_ctx(&detection, &conventions, &constraints, &tech_stack);
        let rules = DomainRuleGenerator::generate(&ctx);

        let error_rule = rules.iter().find(|r| r.name == "error-handling");
        assert!(error_rule.is_some());
        let error_rule = error_rule.unwrap();
        assert_eq!(error_rule.priority, 60);
        assert!(error_rule.triggers.as_ref().unwrap().contains(&"error".into()));
        assert!(error_rule.content.iter().any(|c| c.contains("ResultType")));
        assert!(error_rule.content.iter().any(|c| c.contains("AppError")));
        assert!(error_rule.content.iter().any(|c| c.contains("? operator")));
    }

    #[test]
    fn test_concurrency_evidence() {
        let detection = ProjectDetection::default();
        let conventions = InferredConventions {
            architecture: ArchitectureConvention::default(),
            naming: NamingConventions::default(),
            file_organization: FileOrganization::default(),
            error_handling: ErrorHandlingPattern::default(),
            async_pattern: AsyncPattern {
                style: AsyncStyle::AsyncAwait,
                async_count: 20,
                sync_count: 5,
                runtime: Some("tokio".into()),
                concurrency_patterns: vec!["spawn".into()],
            },
            patterns: Vec::new(),
            testing: TestingConvention::default(),
        };
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");

        let ctx = make_ctx(&detection, &conventions, &constraints, &tech_stack);
        let rules = DomainRuleGenerator::generate(&ctx);

        let concurrency_rule = rules.iter().find(|r| r.name == "concurrency");
        assert!(concurrency_rule.is_some());
        let concurrency_rule = concurrency_rule.unwrap();
        assert!(concurrency_rule.content.iter().any(|c| c.contains("AsyncAwait")));
        assert!(concurrency_rule.content.iter().any(|c| c.contains("spawn")));
        assert!(concurrency_rule.content.iter().any(|c| c.contains("tokio")));
    }

    #[test]
    fn test_testing_evidence() {
        let detection = ProjectDetection::default();
        let conventions = InferredConventions {
            architecture: ArchitectureConvention::default(),
            naming: NamingConventions::default(),
            file_organization: FileOrganization::default(),
            error_handling: ErrorHandlingPattern::default(),
            async_pattern: AsyncPattern::default(),
            patterns: Vec::new(),
            testing: TestingConvention {
                framework: Some("built-in".into()),
                location: crate::pipeline::phases::convention_inference::TestLocation::SameDirectory,
                naming_pattern: "test_*".into(),
                coverage_tools: vec![],
            },
        };
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");

        let ctx = make_ctx(&detection, &conventions, &constraints, &tech_stack);
        let rules = DomainRuleGenerator::generate(&ctx);

        let testing_rule = rules.iter().find(|r| r.name == "testing");
        assert!(testing_rule.is_some());
        let testing_rule = testing_rule.unwrap();
        assert!(testing_rule.content.iter().any(|c| c.contains("built-in")));
        assert!(testing_rule.content.iter().any(|c| c.contains("test_*")));
    }

    #[test]
    fn test_no_evidence_no_rules_for_api() {
        let detection = ProjectDetection::default();
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

        let ctx = make_ctx(&detection, &conventions, &constraints, &tech_stack);
        let rules = DomainRuleGenerator::generate(&ctx);

        // Domains with no evidence from conventions produce no rules
        assert!(rules.iter().find(|r| r.name == "api").is_none());
        assert!(rules.iter().find(|r| r.name == "data").is_none());
        assert!(rules.iter().find(|r| r.name == "logging").is_none());
        assert!(rules.iter().find(|r| r.name == "performance").is_none());
    }

    #[test]
    fn test_gotcha_evidence_creates_rule() {
        let detection = ProjectDetection::default();
        let conventions = InferredConventions {
            architecture: ArchitectureConvention::default(),
            naming: NamingConventions::default(),
            file_organization: FileOrganization::default(),
            error_handling: ErrorHandlingPattern::default(),
            async_pattern: AsyncPattern::default(),
            patterns: Vec::new(),
            testing: TestingConvention::default(),
        };
        let constraints = ExtractedConstraints {
            gotchas: vec![Gotcha {
                title: "Database connection leak".into(),
                description: "Connections not returned to pool in error path".into(),
                when: "Using raw database queries".into(),
                solution: "Use connection guard pattern".into(),
                related_files: vec![],
            }],
            ..Default::default()
        };
        let tech_stack = TechStack::new("rust");

        let ctx = make_ctx(&detection, &conventions, &constraints, &tech_stack);
        let rules = DomainRuleGenerator::generate(&ctx);

        // The "data" domain triggers on "database", and the gotcha contains "database"
        let data_rule = rules.iter().find(|r| r.name == "data");
        assert!(data_rule.is_some(), "Gotcha evidence should create a data domain rule");
        assert!(data_rule.unwrap().content.iter().any(|c| c.contains("connection leak")));
    }

    #[test]
    fn test_anti_pattern_evidence_creates_rule() {
        let detection = ProjectDetection::default();
        let conventions = InferredConventions {
            architecture: ArchitectureConvention::default(),
            naming: NamingConventions::default(),
            file_organization: FileOrganization::default(),
            error_handling: ErrorHandlingPattern::default(),
            async_pattern: AsyncPattern::default(),
            patterns: Vec::new(),
            testing: TestingConvention::default(),
        };
        let constraints = ExtractedConstraints {
            anti_patterns: vec![AntiPattern {
                name: "Hardcoded API keys".into(),
                description: "API keys embedded in route handlers".into(),
                why_bad: "Security risk".into(),
                correct_approach: "Use environment variables".into(),
                evidence: vec![],
                severity: crate::types::Severity::Critical,
            }],
            ..Default::default()
        };
        let tech_stack = TechStack::new("rust");

        let ctx = make_ctx(&detection, &conventions, &constraints, &tech_stack);
        let rules = DomainRuleGenerator::generate(&ctx);

        // "api" domain triggers on "API", and the anti-pattern contains "API"
        let api_rule = rules.iter().find(|r| r.name == "api");
        assert!(api_rule.is_some(), "Anti-pattern evidence should create an api domain rule");
        assert!(api_rule.unwrap().content.iter().any(|c| c.contains("Hardcoded API keys")));
    }
}
