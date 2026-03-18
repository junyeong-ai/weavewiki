//! Hook Generator
//!
//! Evidence-based generation of Claude Code lifecycle hooks.
//! Only generates hooks for tools/frameworks actually detected in the project.
//!
//! Hook categories:
//! - **Linter hooks** (PostToolUse): Auto-fix lint issues after file writes
//! - **Format hooks** (PostToolUse): Auto-format after file writes
//! - **Test hooks** (Stop): Run tests before session ends

use super::context::GenerationContext;
use crate::pipeline::context::FileRegistryExt;
use crate::types::hook::{Hook, HooksConfig};
use crate::types::module_map::TechStack;

/// Generates lifecycle hooks based on detected project tooling.
pub struct HookGenerator;

impl HookGenerator {
    /// Generate a `HooksConfig` from the generation context.
    ///
    /// Only produces hooks for tools with evidence of presence:
    /// - Language-specific linters/formatters detected via config files or manifests
    /// - Test frameworks detected via language and manifest presence
    pub fn generate(ctx: &GenerationContext<'_>) -> HooksConfig {
        let mut post_tool_use = Vec::new();
        let mut stop = Vec::new();

        // Collect format hooks (run before linters so lint sees formatted code)
        post_tool_use.extend(Self::format_hooks(ctx));

        // Collect linter hooks
        post_tool_use.extend(Self::linter_hooks(ctx));

        // Collect test hooks
        stop.extend(Self::test_hooks(ctx));

        HooksConfig {
            post_tool_use: if post_tool_use.is_empty() {
                None
            } else {
                Some(post_tool_use)
            },
            stop: if stop.is_empty() { None } else { Some(stop) },
            ..Default::default()
        }
    }

    /// Generate a `HooksConfig` from standalone detection data (no full context).
    pub fn generate_from_tech_stack(tech_stack: &TechStack) -> HooksConfig {
        let mut post_tool_use = Vec::new();
        let mut stop = Vec::new();

        post_tool_use.extend(Self::format_hooks_from_lang(&tech_stack.primary_language));
        post_tool_use.extend(Self::linter_hooks_from_lang(&tech_stack.primary_language));
        stop.extend(Self::test_hooks_from_lang(&tech_stack.primary_language));

        HooksConfig {
            post_tool_use: if post_tool_use.is_empty() {
                None
            } else {
                Some(post_tool_use)
            },
            stop: if stop.is_empty() { None } else { Some(stop) },
            ..Default::default()
        }
    }

    // =========================================================================
    // Format hooks (PostToolUse for Write/Edit)
    // =========================================================================

    fn format_hooks(ctx: &GenerationContext<'_>) -> Vec<Hook> {
        let mut hooks = Vec::new();
        let lang = ctx.tech_stack.primary_language.as_str();

        match lang {
            "rust" if ctx.file_registry.file_exists("Cargo.toml") => {
                hooks.push(Hook::new("Write", "cargo fmt -- --check"));
                hooks.push(Hook::new("Edit", "cargo fmt -- --check"));
            }
            "typescript" | "javascript"
                if ctx.file_registry.file_exists(".prettierrc")
                    || ctx.file_registry.file_exists(".prettierrc.json")
                    || ctx.file_registry.file_exists(".prettierrc.js")
                    || ctx.file_registry.file_exists("prettier.config.js")
                    || ctx.file_registry.file_exists("prettier.config.mjs") =>
            {
                hooks.push(Hook::new(
                    "Write",
                    "npx prettier --write $CLAUDE_FILE_PATH",
                ));
                hooks.push(Hook::new(
                    "Edit",
                    "npx prettier --write $CLAUDE_FILE_PATH",
                ));
            }
            "python"
                if ctx.file_registry.file_exists("pyproject.toml")
                    || ctx.file_registry.file_exists("setup.cfg") =>
            {
                // Check for black config indicators in pyproject.toml signal presence
                hooks.push(Hook::new("Write", "black --quiet $CLAUDE_FILE_PATH"));
                hooks.push(Hook::new("Edit", "black --quiet $CLAUDE_FILE_PATH"));
            }
            "go" if ctx.file_registry.file_exists("go.mod") => {
                hooks.push(Hook::new("Write", "gofmt -w $CLAUDE_FILE_PATH"));
                hooks.push(Hook::new("Edit", "gofmt -w $CLAUDE_FILE_PATH"));
            }
            _ => {}
        }

        hooks
    }

    fn format_hooks_from_lang(lang: &str) -> Vec<Hook> {
        match lang {
            "rust" => vec![
                Hook::new("Write", "cargo fmt -- --check"),
                Hook::new("Edit", "cargo fmt -- --check"),
            ],
            "go" => vec![
                Hook::new("Write", "gofmt -w $CLAUDE_FILE_PATH"),
                Hook::new("Edit", "gofmt -w $CLAUDE_FILE_PATH"),
            ],
            _ => Vec::new(),
        }
    }

    // =========================================================================
    // Linter hooks (PostToolUse for Write/Edit)
    // =========================================================================

    fn linter_hooks(ctx: &GenerationContext<'_>) -> Vec<Hook> {
        let mut hooks = Vec::new();
        let lang = ctx.tech_stack.primary_language.as_str();

        match lang {
            "rust" if ctx.file_registry.file_exists("Cargo.toml") => {
                hooks.push(Hook::new(
                    "Write",
                    "cargo clippy --fix --allow-dirty --allow-staged 2>/dev/null || true",
                ));
                hooks.push(Hook::new(
                    "Edit",
                    "cargo clippy --fix --allow-dirty --allow-staged 2>/dev/null || true",
                ));
            }
            "typescript" | "javascript"
                if ctx.file_registry.file_exists(".eslintrc")
                    || ctx.file_registry.file_exists(".eslintrc.js")
                    || ctx.file_registry.file_exists(".eslintrc.json")
                    || ctx.file_registry.file_exists(".eslintrc.yml")
                    || ctx.file_registry.file_exists("eslint.config.js")
                    || ctx.file_registry.file_exists("eslint.config.mjs") =>
            {
                hooks.push(Hook::new(
                    "Write",
                    "npx eslint --fix $CLAUDE_FILE_PATH 2>/dev/null || true",
                ));
                hooks.push(Hook::new(
                    "Edit",
                    "npx eslint --fix $CLAUDE_FILE_PATH 2>/dev/null || true",
                ));
            }
            "python"
                if ctx.file_registry.file_exists("pyproject.toml")
                    || ctx.file_registry.file_exists("ruff.toml")
                    || ctx.file_registry.file_exists(".ruff.toml") =>
            {
                hooks.push(Hook::new(
                    "Write",
                    "ruff check --fix $CLAUDE_FILE_PATH 2>/dev/null || true",
                ));
                hooks.push(Hook::new(
                    "Edit",
                    "ruff check --fix $CLAUDE_FILE_PATH 2>/dev/null || true",
                ));
            }
            "go" if ctx.file_registry.file_exists("go.mod") => {
                // golangci-lint is detected by config presence
                if ctx.file_registry.file_exists(".golangci.yml")
                    || ctx.file_registry.file_exists(".golangci.yaml")
                    || ctx.file_registry.file_exists(".golangci.toml")
                {
                    hooks.push(Hook::new(
                        "Write",
                        "golangci-lint run --fix ./... 2>/dev/null || true",
                    ));
                    hooks.push(Hook::new(
                        "Edit",
                        "golangci-lint run --fix ./... 2>/dev/null || true",
                    ));
                }
            }
            _ => {}
        }

        hooks
    }

    fn linter_hooks_from_lang(lang: &str) -> Vec<Hook> {
        match lang {
            "rust" => vec![
                Hook::new(
                    "Write",
                    "cargo clippy --fix --allow-dirty --allow-staged 2>/dev/null || true",
                ),
                Hook::new(
                    "Edit",
                    "cargo clippy --fix --allow-dirty --allow-staged 2>/dev/null || true",
                ),
            ],
            _ => Vec::new(),
        }
    }

    // =========================================================================
    // Test hooks (Stop event)
    // =========================================================================

    fn test_hooks(ctx: &GenerationContext<'_>) -> Vec<Hook> {
        let mut hooks = Vec::new();
        let lang = ctx.tech_stack.primary_language.as_str();

        match lang {
            "rust" if ctx.file_registry.file_exists("Cargo.toml") => {
                hooks.push(Hook::new(".*", "cargo test"));
            }
            "typescript" | "javascript" => {
                if ctx.file_registry.file_exists("jest.config.js")
                    || ctx.file_registry.file_exists("jest.config.ts")
                    || ctx.file_registry.file_exists("jest.config.mjs")
                {
                    hooks.push(Hook::new(".*", "npx jest --passWithNoTests"));
                } else if ctx.file_registry.file_exists("vitest.config.ts")
                    || ctx.file_registry.file_exists("vitest.config.js")
                {
                    hooks.push(Hook::new(".*", "npx vitest run"));
                } else if ctx.file_registry.file_exists("package.json") {
                    hooks.push(Hook::new(".*", "npm test"));
                }
            }
            "python" => {
                if ctx.file_registry.file_exists("pytest.ini")
                    || ctx.file_registry.file_exists("pyproject.toml")
                    || ctx.file_registry.file_exists("setup.cfg")
                {
                    hooks.push(Hook::new(".*", "pytest"));
                }
            }
            "go" if ctx.file_registry.file_exists("go.mod") => {
                hooks.push(Hook::new(".*", "go test ./..."));
            }
            "java" | "kotlin" => {
                if ctx.file_registry.file_exists("build.gradle")
                    || ctx.file_registry.file_exists("build.gradle.kts")
                {
                    hooks.push(Hook::new(".*", "gradle test"));
                } else if ctx.file_registry.file_exists("pom.xml") {
                    hooks.push(Hook::new(".*", "mvn test"));
                }
            }
            _ => {}
        }

        hooks
    }

    fn test_hooks_from_lang(lang: &str) -> Vec<Hook> {
        match lang {
            "rust" => vec![Hook::new(".*", "cargo test")],
            "go" => vec![Hook::new(".*", "go test ./...")],
            _ => Vec::new(),
        }
    }

    /// Returns true if the generated config has any hooks.
    pub fn has_hooks(config: &HooksConfig) -> bool {
        config.pre_tool_use.as_ref().is_some_and(|h| !h.is_empty())
            || config
                .post_tool_use
                .as_ref()
                .is_some_and(|h| !h.is_empty())
            || config.stop.as_ref().is_some_and(|h| !h.is_empty())
            || config
                .notification
                .as_ref()
                .is_some_and(|h| !h.is_empty())
            || config
                .session_start
                .as_ref()
                .is_some_and(|h| !h.is_empty())
            || config
                .session_end
                .as_ref()
                .is_some_and(|h| !h.is_empty())
            || config
                .subagent_stop
                .as_ref()
                .is_some_and(|h| !h.is_empty())
            || config
                .permission_request
                .as_ref()
                .is_some_and(|h| !h.is_empty())
            || config
                .user_prompt_submit
                .as_ref()
                .is_some_and(|h| !h.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::context::VerifiedFileRegistry;
    use crate::pipeline::phases::{
        constraint_extraction::ExtractedConstraints,
        convention_inference::InferredConventions,
        project_detection::ProjectDetection,
    };
    use crate::types::module_map::TechStack;

    fn registry_with(paths: &[&str]) -> VerifiedFileRegistry {
        let mut registry = VerifiedFileRegistry::empty();
        for path in paths {
            registry.register_test_file(path);
        }
        registry
    }

    fn test_context_with_registry<'a>(
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

    // =========================================================================
    // Empty project produces no hooks
    // =========================================================================

    #[test]
    fn test_empty_project_no_hooks() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("unknown");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context_with_registry(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );

        let config = HookGenerator::generate(&ctx);

        assert!(config.post_tool_use.is_none());
        assert!(config.stop.is_none());
        assert!(!HookGenerator::has_hooks(&config));
    }

    // =========================================================================
    // Rust project hooks
    // =========================================================================

    #[test]
    fn test_rust_project_hooks() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = registry_with(&["Cargo.toml", "src/main.rs"]);
        let ctx = test_context_with_registry(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );

        let config = HookGenerator::generate(&ctx);
        assert!(HookGenerator::has_hooks(&config));

        // PostToolUse: cargo fmt + clippy for Write and Edit
        let post = config.post_tool_use.expect("should have post_tool_use hooks");
        assert_eq!(post.len(), 4, "expected 4 post_tool_use hooks (fmt Write/Edit + clippy Write/Edit)");

        let commands: Vec<&str> = post.iter().flat_map(|h| h.hooks.iter().map(|c| c.command.as_str())).collect();
        assert!(commands.iter().any(|c| c.contains("cargo fmt")), "should have cargo fmt hook");
        assert!(commands.iter().any(|c| c.contains("cargo clippy")), "should have cargo clippy hook");

        // Stop: cargo test
        let stop = config.stop.expect("should have stop hooks");
        assert_eq!(stop.len(), 1);
        assert!(stop[0].hooks[0].command.contains("cargo test"));
    }

    // =========================================================================
    // Go project hooks
    // =========================================================================

    #[test]
    fn test_go_project_hooks() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("go");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = registry_with(&["go.mod", "main.go"]);
        let ctx = test_context_with_registry(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );

        let config = HookGenerator::generate(&ctx);

        // PostToolUse: gofmt (no golangci-lint without config)
        let post = config.post_tool_use.expect("should have post_tool_use hooks");
        let commands: Vec<&str> = post.iter().flat_map(|h| h.hooks.iter().map(|c| c.command.as_str())).collect();
        assert!(commands.iter().any(|c| c.contains("gofmt")), "should have gofmt hook");
        assert!(!commands.iter().any(|c| c.contains("golangci-lint")), "no golangci-lint without config");

        // Stop: go test
        let stop = config.stop.expect("should have stop hooks");
        assert!(stop[0].hooks[0].command.contains("go test"));
    }

    #[test]
    fn test_go_project_with_golangci_lint() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("go");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = registry_with(&["go.mod", ".golangci.yml"]);
        let ctx = test_context_with_registry(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );

        let config = HookGenerator::generate(&ctx);

        let post = config.post_tool_use.expect("should have post_tool_use hooks");
        let commands: Vec<&str> = post.iter().flat_map(|h| h.hooks.iter().map(|c| c.command.as_str())).collect();
        assert!(commands.iter().any(|c| c.contains("golangci-lint")), "should have golangci-lint hook with config");
    }

    // =========================================================================
    // Python project hooks
    // =========================================================================

    #[test]
    fn test_python_project_hooks() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("python");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = registry_with(&["pyproject.toml", "src/main.py"]);
        let ctx = test_context_with_registry(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );

        let config = HookGenerator::generate(&ctx);

        // PostToolUse: black + ruff
        let post = config.post_tool_use.expect("should have post_tool_use hooks");
        let commands: Vec<&str> = post.iter().flat_map(|h| h.hooks.iter().map(|c| c.command.as_str())).collect();
        assert!(commands.iter().any(|c| c.contains("black")), "should have black hook");
        assert!(commands.iter().any(|c| c.contains("ruff")), "should have ruff hook");

        // Stop: pytest
        let stop = config.stop.expect("should have stop hooks");
        assert!(stop[0].hooks[0].command.contains("pytest"));
    }

    // =========================================================================
    // TypeScript project hooks
    // =========================================================================

    #[test]
    fn test_typescript_project_with_eslint_and_prettier() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("typescript");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = registry_with(&[
            "package.json",
            ".eslintrc.json",
            ".prettierrc",
            "jest.config.ts",
        ]);
        let ctx = test_context_with_registry(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );

        let config = HookGenerator::generate(&ctx);

        let post = config.post_tool_use.expect("should have post_tool_use hooks");
        let commands: Vec<&str> = post.iter().flat_map(|h| h.hooks.iter().map(|c| c.command.as_str())).collect();
        assert!(commands.iter().any(|c| c.contains("prettier")), "should have prettier hook");
        assert!(commands.iter().any(|c| c.contains("eslint")), "should have eslint hook");

        let stop = config.stop.expect("should have stop hooks");
        assert!(stop[0].hooks[0].command.contains("jest"));
    }

    #[test]
    fn test_typescript_no_tools_without_config() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("typescript");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        // Only package.json, no eslint or prettier config
        let registry = registry_with(&["package.json", "src/index.ts"]);
        let ctx = test_context_with_registry(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );

        let config = HookGenerator::generate(&ctx);

        // No format/lint hooks without config files
        let post = config.post_tool_use;
        assert!(post.is_none(), "no format/lint hooks without config files");

        // But test hook should still be present (npm test fallback)
        let stop = config.stop.expect("should have test hook");
        assert!(stop[0].hooks[0].command.contains("npm test"));
    }

    // =========================================================================
    // Java/Kotlin project hooks
    // =========================================================================

    #[test]
    fn test_java_gradle_project_test_hook() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("java");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = registry_with(&["build.gradle", "src/main/java/App.java"]);
        let ctx = test_context_with_registry(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );

        let config = HookGenerator::generate(&ctx);

        let stop = config.stop.expect("should have stop hooks");
        assert!(stop[0].hooks[0].command.contains("gradle test"));
    }

    #[test]
    fn test_kotlin_maven_project_test_hook() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("kotlin");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = registry_with(&["pom.xml", "src/main/kotlin/App.kt"]);
        let ctx = test_context_with_registry(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );

        let config = HookGenerator::generate(&ctx);

        let stop = config.stop.expect("should have stop hooks");
        assert!(stop[0].hooks[0].command.contains("mvn test"));
    }

    // =========================================================================
    // Tech stack standalone generation
    // =========================================================================

    #[test]
    fn test_generate_from_tech_stack_rust() {
        let tech_stack = TechStack::new("rust");
        let config = HookGenerator::generate_from_tech_stack(&tech_stack);

        let post = config.post_tool_use.expect("should have post hooks");
        let commands: Vec<&str> = post.iter().flat_map(|h| h.hooks.iter().map(|c| c.command.as_str())).collect();
        assert!(commands.iter().any(|c| c.contains("cargo fmt")));
        assert!(commands.iter().any(|c| c.contains("cargo clippy")));

        let stop = config.stop.expect("should have stop hooks");
        assert!(stop[0].hooks[0].command.contains("cargo test"));
    }

    #[test]
    fn test_generate_from_tech_stack_unknown() {
        let tech_stack = TechStack::new("unknown");
        let config = HookGenerator::generate_from_tech_stack(&tech_stack);

        assert!(!HookGenerator::has_hooks(&config));
    }

    // =========================================================================
    // has_hooks utility
    // =========================================================================

    #[test]
    fn test_has_hooks_empty() {
        let config = HooksConfig::default();
        assert!(!HookGenerator::has_hooks(&config));
    }

    #[test]
    fn test_has_hooks_with_stop() {
        let config = HooksConfig {
            stop: Some(vec![Hook::new(".*", "echo done")]),
            ..Default::default()
        };
        assert!(HookGenerator::has_hooks(&config));
    }

    // =========================================================================
    // Hook structure validation
    // =========================================================================

    #[test]
    fn test_hook_command_type_is_command() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = registry_with(&["Cargo.toml"]);
        let ctx = test_context_with_registry(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );

        let config = HookGenerator::generate(&ctx);

        if let Some(post) = &config.post_tool_use {
            for hook in post {
                for cmd in &hook.hooks {
                    assert_eq!(cmd.command_type, "command");
                }
            }
        }
        if let Some(stop) = &config.stop {
            for hook in stop {
                for cmd in &hook.hooks {
                    assert_eq!(cmd.command_type, "command");
                }
            }
        }
    }

    #[test]
    fn test_matchers_are_valid() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = registry_with(&["Cargo.toml"]);
        let ctx = test_context_with_registry(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );

        let config = HookGenerator::generate(&ctx);

        if let Some(post) = &config.post_tool_use {
            for hook in post {
                assert!(
                    hook.matcher == "Write" || hook.matcher == "Edit",
                    "PostToolUse matcher should be Write or Edit, got: {}",
                    hook.matcher,
                );
            }
        }
        if let Some(stop) = &config.stop {
            for hook in stop {
                assert_eq!(hook.matcher, ".*", "Stop matcher should be wildcard");
            }
        }
    }
}
