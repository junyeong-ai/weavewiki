//! Framework Rule Generator
//!
//! Generates framework-specific rules (priority 85).
//! Triggered by paths and keywords (e.g., "actix", "tokio", "react").

use super::RuleGenerationContext;
use crate::types::Rule;
use crate::utils::capitalize_first;

pub struct FrameworkRuleGenerator;

struct FrameworkDef {
    name: &'static str,
    paths: &'static [&'static str],
    triggers: &'static [&'static str],
}

const FRAMEWORKS: &[FrameworkDef] = &[
    // Rust frameworks
    FrameworkDef {
        name: "tokio",
        paths: &[],
        triggers: &["tokio", "async_trait", "#[tokio::main]", "#[tokio::test]"],
    },
    FrameworkDef {
        name: "actix",
        paths: &["**/handlers/**", "**/routes/**"],
        triggers: &["actix", "actix_web", "HttpServer", "App::new"],
    },
    FrameworkDef {
        name: "axum",
        paths: &["**/handlers/**", "**/routes/**"],
        triggers: &["axum", "Router", "axum::extract"],
    },
    // JavaScript/TypeScript frameworks
    FrameworkDef {
        name: "react",
        paths: &["**/components/**", "**/pages/**", "**/app/**"],
        triggers: &["React", "useState", "useEffect", "jsx", "tsx"],
    },
    FrameworkDef {
        name: "nextjs",
        paths: &["**/app/**", "**/pages/**"],
        triggers: &["next", "getServerSideProps", "getStaticProps", "NextPage"],
    },
    FrameworkDef {
        name: "express",
        paths: &["**/routes/**", "**/middleware/**"],
        triggers: &["express", "app.get", "app.post", "router"],
    },
    // Python frameworks
    FrameworkDef {
        name: "django",
        paths: &["**/views/**", "**/models/**", "**/urls/**"],
        triggers: &["django", "HttpResponse", "render", "models.Model"],
    },
    FrameworkDef {
        name: "fastapi",
        paths: &["**/routers/**", "**/api/**"],
        triggers: &["FastAPI", "Depends", "@app.get", "@app.post"],
    },
    // Go frameworks
    FrameworkDef {
        name: "gin",
        paths: &["**/handlers/**", "**/routes/**"],
        triggers: &["gin", "gin.Context", "gin.Engine"],
    },
    // Java/Kotlin frameworks
    FrameworkDef {
        name: "spring",
        paths: &["**/controller/**", "**/service/**", "**/repository/**"],
        triggers: &[
            "@Controller",
            "@Service",
            "@Repository",
            "@Autowired",
            "SpringBootApplication",
        ],
    },
];

impl FrameworkRuleGenerator {
    pub fn generate(ctx: &RuleGenerationContext<'_>) -> Vec<Rule> {
        let detected_frameworks = Self::detect_frameworks(ctx);

        detected_frameworks
            .iter()
            .filter_map(|fw| Self::generate_for_framework(ctx, fw))
            .collect()
    }

    fn detect_frameworks(ctx: &RuleGenerationContext<'_>) -> Vec<&'static FrameworkDef> {
        FRAMEWORKS
            .iter()
            .filter(|fw| {
                ctx.tech_stack
                    .frameworks
                    .iter()
                    .any(|f| f.name.to_lowercase() == fw.name)
                    || ctx.tech_stack.key_libraries.iter().any(|lib| {
                        lib.name.to_lowercase() == fw.name
                            || lib.name.to_lowercase().contains(fw.name)
                    })
            })
            .collect()
    }

    fn generate_for_framework(
        ctx: &RuleGenerationContext<'_>,
        framework: &FrameworkDef,
    ) -> Option<Rule> {
        let mut content = Vec::new();

        content.push(format!(
            "# {} Patterns",
            capitalize_first(framework.name)
        ));
        content.push(String::new());

        let framework_info = ctx
            .tech_stack
            .frameworks
            .iter()
            .find(|f| f.name.to_lowercase() == framework.name);

        if let Some(info) = framework_info {
            content.push("## Setup".into());
            content.push(String::new());
            content.push(format!("Purpose: {}", info.purpose));
            if let Some(version) = &info.version {
                content.push(format!("Version: {version}"));
            }
            content.push(String::new());
        }

        match framework.name {
            "tokio" => Self::generate_tokio_content(ctx, &mut content),
            "actix" | "axum" => Self::generate_http_framework_content(ctx, &mut content),
            "react" | "nextjs" => Self::generate_react_content(ctx, &mut content),
            "django" | "fastapi" => Self::generate_python_web_content(ctx, &mut content),
            "spring" => Self::generate_spring_content(ctx, &mut content),
            _ => Self::generate_generic_framework_content(ctx, &mut content, framework.name),
        }

        if content.len() <= 2 {
            return None;
        }

        let paths: Vec<String> = framework.paths.iter().map(|s| (*s).into()).collect();
        let triggers: Vec<String> = framework.triggers.iter().map(|s| (*s).into()).collect();

        Some(Rule::framework(framework.name, paths, triggers, content))
    }

    fn generate_tokio_content(ctx: &RuleGenerationContext<'_>, content: &mut Vec<String>) {
        use crate::pipeline::phases::convention_inference::AsyncStyle;
        let async_pattern = &ctx.conventions.async_pattern;

        content.push("## Async Runtime".into());
        content.push(String::new());
        if async_pattern.style != AsyncStyle::Synchronous {
            content.push(format!("Style: {:?}", async_pattern.style));
        }
        content.push(String::new());

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
        content.push("- Use `#[tokio::main]` for async entry points".into());
        content.push("- Prefer `tokio::spawn` for concurrent tasks".into());
        content.push("- Use `tokio::select!` for racing futures".into());
        content.push("- Avoid blocking calls in async context".into());
        content.push(String::new());
    }

    fn generate_http_framework_content(
        _ctx: &RuleGenerationContext<'_>,
        content: &mut Vec<String>,
    ) {
        content.push("## Request Handling".into());
        content.push(String::new());
        content.push("- Extract request data using typed extractors".into());
        content.push("- Return appropriate status codes".into());
        content.push("- Handle errors with proper error types".into());
        content.push(String::new());

        content.push("## Middleware".into());
        content.push(String::new());
        content.push("- Use middleware for cross-cutting concerns".into());
        content.push("- Keep middleware focused and composable".into());
        content.push(String::new());
    }

    fn generate_react_content(_ctx: &RuleGenerationContext<'_>, content: &mut Vec<String>) {
        content.push("## Component Patterns".into());
        content.push(String::new());
        content.push("- Prefer functional components with hooks".into());
        content.push("- Keep components small and focused".into());
        content.push("- Extract reusable logic into custom hooks".into());
        content.push(String::new());

        content.push("## State Management".into());
        content.push(String::new());
        content.push("- Use `useState` for local state".into());
        content.push("- Use `useReducer` for complex state logic".into());
        content.push("- Lift state only when necessary".into());
        content.push(String::new());
    }

    fn generate_python_web_content(_ctx: &RuleGenerationContext<'_>, content: &mut Vec<String>) {
        content.push("## Route Handlers".into());
        content.push(String::new());
        content.push("- Use type hints for request/response".into());
        content.push("- Validate input data explicitly".into());
        content.push("- Return structured responses".into());
        content.push(String::new());
    }

    fn generate_spring_content(_ctx: &RuleGenerationContext<'_>, content: &mut Vec<String>) {
        content.push("## Dependency Injection".into());
        content.push(String::new());
        content.push("- Prefer constructor injection".into());
        content.push("- Use interfaces for dependencies".into());
        content.push("- Avoid field injection".into());
        content.push(String::new());

        content.push("## Layer Patterns".into());
        content.push(String::new());
        content.push("- Controllers: HTTP handling only".into());
        content.push("- Services: Business logic".into());
        content.push("- Repositories: Data access".into());
        content.push(String::new());
    }

    fn generate_generic_framework_content(
        _ctx: &RuleGenerationContext<'_>,
        content: &mut Vec<String>,
        framework_name: &str,
    ) {
        content.push("## General Guidelines".into());
        content.push(String::new());
        content.push(format!(
            "Follow {} conventions and best practices.",
            capitalize_first(framework_name)
        ));
        content.push(String::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::phases::constraint_extraction::ExtractedConstraints;
    use crate::pipeline::phases::convention_inference::{
        ArchitectureConvention, AsyncPattern, AsyncStyle, ErrorHandlingPattern, FileOrganization,
        InferredConventions, NamingConventions, TestingConvention,
    };
    use crate::pipeline::phases::project_detection::ProjectDetection;
    use crate::types::module_map::{FrameworkInfo, TechStack};

    #[test]
    fn test_framework_rule_generation() {
        let detection = ProjectDetection::default();
        let conventions = InferredConventions {
            architecture: ArchitectureConvention::default(),
            naming: NamingConventions::default(),
            file_organization: FileOrganization::default(),
            error_handling: ErrorHandlingPattern::default(),
            async_pattern: AsyncPattern {
                style: AsyncStyle::AsyncAwait,
                async_count: 50,
                sync_count: 10,
                runtime: Some("tokio".into()),
                concurrency_patterns: vec!["spawn".into(), "select".into()],
            },
            patterns: Vec::new(),
            testing: TestingConvention::default(),
        };
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust")
            .with_framework(FrameworkInfo::new("tokio", "Async runtime"));
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

        let rules = FrameworkRuleGenerator::generate(&ctx);
        assert_eq!(rules.len(), 1);

        let rule = &rules[0];
        assert_eq!(rule.name, "tokio");
        assert_eq!(rule.priority, 85);
        assert!(rule.triggers.as_ref().unwrap().contains(&"tokio".into()));
        assert!(rule.content.iter().any(|c| c.contains("spawn")));
    }

    #[test]
    fn test_no_framework_no_rules() {
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

        let rules = FrameworkRuleGenerator::generate(&ctx);
        assert!(rules.is_empty());
    }
}
