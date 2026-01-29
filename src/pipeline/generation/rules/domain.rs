//! Domain Rule Generator
//!
//! Generates domain-specific rules (priority 60).
//! Triggered by keywords (e.g., "auth", "async", "error").

use super::RuleGenerationContext;
use crate::types::Rule;
use crate::utils::capitalize_first;

pub struct DomainRuleGenerator;

/// Domain definitions with their triggers and content generators
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

        match domain.name {
            "security" => Self::generate_security_content(ctx, &mut content),
            "error-handling" => Self::generate_error_handling_content(ctx, &mut content),
            "concurrency" => Self::generate_concurrency_content(ctx, &mut content),
            "testing" => Self::generate_testing_content(ctx, &mut content),
            "performance" => Self::generate_performance_content(ctx, &mut content),
            "api" => Self::generate_api_content(ctx, &mut content),
            "data" => Self::generate_data_content(ctx, &mut content),
            "logging" => Self::generate_logging_content(ctx, &mut content),
            _ => {}
        }

        // Get domain-related gotchas
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

        // Get domain-related anti-patterns
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

        // Skip domains with minimal content (only header)
        if content.len() <= 2 {
            return None;
        }

        let triggers: Vec<String> = domain.triggers.iter().map(|s| (*s).into()).collect();
        Some(Rule::domain(domain.name, triggers, content))
    }

    fn generate_security_content(ctx: &RuleGenerationContext<'_>, content: &mut Vec<String>) {
        content.push("## Principles".into());
        content.push(String::new());
        content.push("1. Never trust user input".into());
        content.push("2. Use secure defaults".into());
        content.push("3. Fail securely".into());
        content.push("4. Minimize attack surface".into());
        content.push(String::new());

        // Add project-specific security patterns from constraints
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
                content.push(format!("- {} → {}: {}", dep.source, dep.target, dep.description));
            }
            content.push(String::new());
        }
    }

    fn generate_error_handling_content(
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

    fn generate_concurrency_content(ctx: &RuleGenerationContext<'_>, content: &mut Vec<String>) {
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

        content.push("## Best Practices".into());
        content.push(String::new());
        content.push("- Avoid blocking in async contexts".into());
        content.push("- Use structured concurrency".into());
        content.push("- Prefer message passing over shared state".into());
        content.push(String::new());
    }

    fn generate_testing_content(ctx: &RuleGenerationContext<'_>, content: &mut Vec<String>) {
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

    fn generate_performance_content(_ctx: &RuleGenerationContext<'_>, content: &mut Vec<String>) {
        content.push("## Principles".into());
        content.push(String::new());
        content.push("1. Measure before optimizing".into());
        content.push("2. Optimize hot paths only".into());
        content.push("3. Consider cache locality".into());
        content.push("4. Avoid premature optimization".into());
        content.push(String::new());
    }

    fn generate_api_content(_ctx: &RuleGenerationContext<'_>, content: &mut Vec<String>) {
        content.push("## Request Handling".into());
        content.push(String::new());
        content.push("1. **Validate input** - Never trust client data".into());
        content.push("2. **Use typed extractors** - Leverage framework type safety".into());
        content.push("3. **Return appropriate status codes** - 2xx success, 4xx client error, 5xx server error".into());
        content.push(String::new());

        content.push("## Response Format".into());
        content.push(String::new());
        content.push("- Use consistent response structure".into());
        content.push("- Include error details in error responses".into());
        content.push("- Set appropriate Content-Type headers".into());
        content.push(String::new());

        content.push("## Error Responses".into());
        content.push(String::new());
        content.push("```json".into());
        content.push(r#"{"error": {"code": "ERROR_CODE", "message": "Human readable message"}}"#.into());
        content.push("```".into());
        content.push(String::new());

        content.push("## Best Practices".into());
        content.push(String::new());
        content.push("- Document endpoints with OpenAPI/Swagger".into());
        content.push("- Version APIs when breaking changes needed".into());
        content.push("- Use pagination for list endpoints".into());
        content.push("- Implement rate limiting".into());
        content.push(String::new());
    }

    fn generate_data_content(_ctx: &RuleGenerationContext<'_>, content: &mut Vec<String>) {
        content.push("## Query Patterns".into());
        content.push(String::new());
        content.push("1. **Use parameterized queries** - Prevent SQL injection".into());
        content.push("2. **Minimize N+1 queries** - Use joins or batch loading".into());
        content.push("3. **Index appropriately** - Index frequently queried columns".into());
        content.push(String::new());

        content.push("## Transaction Handling".into());
        content.push(String::new());
        content.push("- Keep transactions short".into());
        content.push("- Use appropriate isolation levels".into());
        content.push("- Handle rollback scenarios".into());
        content.push(String::new());

        content.push("## Data Validation".into());
        content.push(String::new());
        content.push("- Validate at domain boundary".into());
        content.push("- Use database constraints as safety net".into());
        content.push("- Define allowed value ranges".into());
        content.push(String::new());

        content.push("## Migration Patterns".into());
        content.push(String::new());
        content.push("- Migrations must be reversible".into());
        content.push("- Test migrations on copy of production data".into());
        content.push("- Avoid destructive changes (prefer additive)".into());
        content.push(String::new());
    }

    fn generate_logging_content(_ctx: &RuleGenerationContext<'_>, content: &mut Vec<String>) {
        content.push("## Log Levels".into());
        content.push(String::new());
        content.push("| Level | When to Use |".into());
        content.push("|-------|-------------|".into());
        content.push("| ERROR | Unrecoverable failures requiring attention |".into());
        content.push("| WARN | Recoverable issues, degraded functionality |".into());
        content.push("| INFO | Significant business events, state changes |".into());
        content.push("| DEBUG | Detailed diagnostic information |".into());
        content.push("| TRACE | Very detailed debugging (performance impact) |".into());
        content.push(String::new());

        content.push("## What to Log".into());
        content.push(String::new());
        content.push("**DO log:**".into());
        content.push("- Request/response boundaries".into());
        content.push("- Business event outcomes".into());
        content.push("- Error conditions with context".into());
        content.push("- Performance-relevant operations".into());
        content.push(String::new());

        content.push("**DON'T log:**".into());
        content.push("- Passwords, tokens, API keys".into());
        content.push("- PII (personal identifiable information)".into());
        content.push("- Full request/response bodies in production".into());
        content.push(String::new());

        content.push("## Structured Logging".into());
        content.push(String::new());
        content.push("- Use key=value or JSON format".into());
        content.push("- Include correlation/request IDs".into());
        content.push("- Add relevant context fields".into());
        content.push(String::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::phases::constraint_extraction::ExtractedConstraints;
    use crate::pipeline::phases::convention_inference::{
        ArchitectureConvention, AsyncPattern, AsyncStyle, ErrorHandlingPattern, ErrorStyle,
        FileOrganization, InferredConventions, NamingConventions, TestingConvention,
    };
    use crate::pipeline::phases::project_detection::ProjectDetection;
    use crate::types::module_map::TechStack;

    #[test]
    fn test_domain_rule_generation() {
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
            async_pattern: AsyncPattern {
                style: AsyncStyle::AsyncAwait,
                async_count: 20,
                sync_count: 5,
                runtime: Some("tokio".into()),
                concurrency_patterns: vec!["spawn".into()],
            },
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

        let rules = DomainRuleGenerator::generate(&ctx);

        // Should generate rules for domains with content
        assert!(!rules.is_empty());

        // Check error-handling rule
        let error_rule = rules.iter().find(|r| r.name == "error-handling");
        assert!(error_rule.is_some());
        let error_rule = error_rule.unwrap();
        assert_eq!(error_rule.priority, 60);
        assert!(error_rule.triggers.as_ref().unwrap().contains(&"error".into()));

        // Check concurrency rule
        let concurrency_rule = rules.iter().find(|r| r.name == "concurrency");
        assert!(concurrency_rule.is_some());
        assert!(concurrency_rule
            .unwrap()
            .triggers
            .as_ref()
            .unwrap()
            .contains(&"async".into()));

        // Check API rule
        let api_rule = rules.iter().find(|r| r.name == "api");
        assert!(api_rule.is_some());
        let api_rule = api_rule.unwrap();
        assert!(api_rule.triggers.as_ref().unwrap().contains(&"endpoint".into()));
        assert!(api_rule.content.iter().any(|c| c.contains("Validate input")));

        // Check data rule
        let data_rule = rules.iter().find(|r| r.name == "data");
        assert!(data_rule.is_some());
        let data_rule = data_rule.unwrap();
        assert!(data_rule.triggers.as_ref().unwrap().contains(&"database".into()));
        assert!(data_rule.content.iter().any(|c| c.contains("parameterized queries")));

        // Check logging rule
        let logging_rule = rules.iter().find(|r| r.name == "logging");
        assert!(logging_rule.is_some());
        let logging_rule = logging_rule.unwrap();
        assert!(logging_rule.triggers.as_ref().unwrap().contains(&"trace".into()));
        assert!(logging_rule.content.iter().any(|c| c.contains("Log Levels")));
    }
}
