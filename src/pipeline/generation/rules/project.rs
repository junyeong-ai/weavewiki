//! Project Rule Generator
//!
//! Generates the project-level rule (priority 100, always inject).
//! Contains: architecture overview, global conventions, build commands.

use super::RuleGenerationContext;
use crate::types::Rule;

pub struct ProjectRuleGenerator;

/// Project commands derived from detected language/build system
struct ProjectCommands {
    build: &'static str,
    test: &'static str,
    lint: Option<&'static str>,
    format: Option<&'static str>,
}

impl ProjectCommands {
    /// Derive commands from primary language and manifest
    fn from_detection(ctx: &RuleGenerationContext<'_>) -> Option<Self> {
        let lang = ctx.detection.languages.first()?;
        let manifest = lang.primary_manifest.as_deref();

        Some(match (lang.language.to_lowercase().as_str(), manifest) {
            ("rust", _) => Self {
                build: "cargo build",
                test: "cargo test",
                lint: Some("cargo clippy"),
                format: Some("cargo fmt"),
            },
            ("typescript" | "javascript", Some(m)) if m.contains("package.json") => Self {
                build: "npm run build",
                test: "npm test",
                lint: Some("npm run lint"),
                format: Some("npm run format"),
            },
            ("python", Some(m)) if m.contains("pyproject.toml") => Self {
                build: "python -m build",
                test: "pytest",
                lint: Some("ruff check ."),
                format: Some("ruff format ."),
            },
            ("python", _) => Self {
                build: "pip install -e .",
                test: "pytest",
                lint: Some("flake8"),
                format: Some("black ."),
            },
            ("go", _) => Self {
                build: "go build ./...",
                test: "go test ./...",
                lint: Some("golangci-lint run"),
                format: Some("gofmt -w ."),
            },
            ("java", Some(m)) if m.contains("pom.xml") => Self {
                build: "mvn compile",
                test: "mvn test",
                lint: Some("mvn checkstyle:check"),
                format: None,
            },
            ("java", Some(m)) if m.contains("gradle") => Self {
                build: "./gradlew build",
                test: "./gradlew test",
                lint: Some("./gradlew check"),
                format: None,
            },
            ("kotlin", _) => Self {
                build: "./gradlew build",
                test: "./gradlew test",
                lint: Some("./gradlew ktlintCheck"),
                format: Some("./gradlew ktlintFormat"),
            },
            ("ruby", _) => Self {
                build: "bundle install",
                test: "bundle exec rspec",
                lint: Some("bundle exec rubocop"),
                format: Some("bundle exec rubocop -a"),
            },
            ("php", _) => Self {
                build: "composer install",
                test: "vendor/bin/phpunit",
                lint: Some("vendor/bin/phpcs"),
                format: Some("vendor/bin/phpcbf"),
            },
            _ => return None,
        })
    }
}

impl ProjectRuleGenerator {
    pub fn generate(ctx: &RuleGenerationContext<'_>) -> Option<Rule> {
        let mut content = Vec::new();

        content.push(format!("# {}", ctx.project_name));
        content.push(String::new());

        // Project type and languages
        let project_type = ctx.detection.primary_type.as_str();
        let languages: Vec<_> = ctx
            .detection
            .languages
            .iter()
            .map(|l| l.language.as_str())
            .collect();

        content.push(format!(
            "A {} project{}.",
            project_type,
            if languages.is_empty() {
                String::new()
            } else {
                format!(" written in {}", languages.join(", "))
            }
        ));
        content.push(String::new());

        // Architecture
        if !ctx.conventions.architecture.pattern_name.is_empty() {
            content.push("## Architecture".into());
            content.push(String::new());
            content.push(format!(
                "**Pattern**: {}",
                ctx.conventions.architecture.pattern_name
            ));

            if !ctx.conventions.architecture.description.is_empty() {
                content.push(String::new());
                content.push(ctx.conventions.architecture.description.clone());
            }

            if !ctx.conventions.architecture.layers.is_empty() {
                content.push(String::new());
                content.push("### Layers".into());
                for layer in &ctx.conventions.architecture.layers {
                    content.push(format!(
                        "- `{}` - {}",
                        layer.path_pattern, layer.responsibility
                    ));
                }
            }
            content.push(String::new());
        }

        // Global conventions
        let global_conventions: Vec<_> = ctx
            .conventions
            .patterns
            .iter()
            .filter(|p| p.frequency > 0.7)
            .collect();

        if !global_conventions.is_empty() {
            content.push("## Conventions".into());
            content.push(String::new());
            for pattern in global_conventions {
                content.push(format!("- **{}**: {}", pattern.name, pattern.description));
            }
            content.push(String::new());
        }

        // Key directories
        if !ctx.conventions.file_organization.key_directories.is_empty() {
            content.push("## Structure".into());
            content.push(String::new());
            for dir in &ctx.conventions.file_organization.key_directories {
                content.push(format!("- `{}` - {}", dir.path, dir.role));
            }
            content.push(String::new());
        }

        // Anti-patterns (highest value content)
        if !ctx.constraints.anti_patterns.is_empty() {
            content.push("## Anti-Patterns".into());
            content.push(String::new());
            for ap in &ctx.constraints.anti_patterns {
                content.push(format!("### {} (DON'T)", ap.name));
                content.push(ap.description.clone());
                content.push(format!("**Instead**: {}", ap.correct_approach));
                if let Some(ev) = ap.evidence.first() {
                    content.push(format!("(@{}:{})", ev.file, ev.line.unwrap_or(1)));
                }
                content.push(String::new());
            }
        }

        // Gotchas (Tier 3 content)
        if !ctx.constraints.gotchas.is_empty() {
            content.push("## Gotchas".into());
            content.push(String::new());
            for gotcha in &ctx.constraints.gotchas {
                content.push(format!("### {} (WARNING)", gotcha.title));
                content.push(gotcha.description.clone());
                content.push(format!("**When**: {}", gotcha.when));
                content.push(format!("**Solution**: {}", gotcha.solution));
                content.push(String::new());
            }
        }

        // Commands (essential for AI to verify changes)
        if let Some(commands) = ProjectCommands::from_detection(ctx) {
            content.push("## Commands".into());
            content.push(String::new());
            content.push("| Action | Command |".into());
            content.push("|--------|---------|".into());
            content.push(format!("| Build | `{}` |", commands.build));
            content.push(format!("| Test | `{}` |", commands.test));
            if let Some(lint) = commands.lint {
                content.push(format!("| Lint | `{}` |", lint));
            }
            if let Some(format) = commands.format {
                content.push(format!("| Format | `{}` |", format));
            }
            content.push(String::new());
        }

        // Environment variables (from constraint extraction)
        if !ctx.constraints.environment_variables.is_empty() {
            content.push("## Environment Variables".into());
            content.push(String::new());
            content.push("| Variable | Required | Description |".into());
            content.push("|----------|----------|-------------|".into());
            for env_var in &ctx.constraints.environment_variables {
                let required = if env_var.required { "✓" } else { "" };
                let desc = if env_var.description.is_empty() {
                    "-".to_string()
                } else {
                    env_var.description.clone()
                };
                content.push(format!("| `{}` | {} | {} |", env_var.name, required, desc));
            }
            content.push(String::new());
        }

        if content.len() <= 3 {
            return None;
        }

        Some(Rule::project("project", content))
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
    fn test_project_rule_generation() {
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
            architecture: ArchitectureConvention {
                pattern_name: "Clean Architecture".into(),
                description: "Layered architecture with dependency inversion".into(),
                ..Default::default()
            },
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

        let rule = ProjectRuleGenerator::generate(&ctx);
        assert!(rule.is_some());

        let rule = rule.unwrap();
        assert_eq!(rule.name, "project");
        assert!(rule.always_inject);
        assert_eq!(rule.priority, 100);
        assert!(rule.content.iter().any(|c| c.contains("Clean Architecture")));
        // Verify commands are generated for Rust projects
        assert!(rule.content.iter().any(|c| c.contains("cargo build")));
        assert!(rule.content.iter().any(|c| c.contains("cargo test")));
        assert!(rule.content.iter().any(|c| c.contains("cargo clippy")));
    }

    #[test]
    fn test_project_commands_by_language() {
        // Test Python project
        let detection = ProjectDetection {
            languages: vec![LanguageInfo {
                language: "python".into(),
                file_count: 30,
                percentage: 0.9,
                primary_manifest: Some("pyproject.toml".into()),
            }],
            ..Default::default()
        };
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("python");

        let ctx = RuleGenerationContext {
            detection: &detection,
            conventions: &conventions,
            constraints: &constraints,
            tech_stack: &tech_stack,
            modules: &[],
            groups: &[],
            project_name: "python-project",
        };

        let rule = ProjectRuleGenerator::generate(&ctx);
        assert!(rule.is_some());
        let rule = rule.unwrap();
        assert!(rule.content.iter().any(|c| c.contains("pytest")));
        assert!(rule.content.iter().any(|c| c.contains("ruff")));
    }

    #[test]
    fn test_project_commands_typescript() {
        let detection = ProjectDetection {
            languages: vec![LanguageInfo {
                language: "typescript".into(),
                file_count: 50,
                percentage: 0.8,
                primary_manifest: Some("package.json".into()),
            }],
            ..Default::default()
        };
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("typescript");

        let ctx = RuleGenerationContext {
            detection: &detection,
            conventions: &conventions,
            constraints: &constraints,
            tech_stack: &tech_stack,
            modules: &[],
            groups: &[],
            project_name: "ts-project",
        };

        let rule = ProjectRuleGenerator::generate(&ctx);
        assert!(rule.is_some());
        let rule = rule.unwrap();
        assert!(rule.content.iter().any(|c| c.contains("npm")));
    }
}
