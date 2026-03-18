//! Settings Generation
//!
//! Generates Claude Code `settings.json` content with permission rules
//! based on detected build/test commands and sensitive file patterns.
//!
//! # Detection-Based Generation
//!
//! Allow rules are derived from detected languages and build tools.
//! Deny rules are derived from sensitive file patterns that actually
//! exist in the project (evidence-based: no generation without evidence).

use std::path::Path;

use serde::Serialize;

use crate::pipeline::phases::project_detection::ProjectDetection;
use crate::types::module_map::TechStack;

/// Common sensitive file patterns to check for deny rules.
///
/// Each entry is (glob_pattern, display_name_for_rule).
/// Only patterns whose files actually exist in the project are included.
const SENSITIVE_PATTERNS: &[&str] = &[
    ".env",
    "credentials.json",
    "secrets.json",
    "secrets.yaml",
    "secrets.yml",
];

/// Sensitive file extensions to scan for.
/// These are checked via directory walk, not glob, because
/// we need to find any file matching `*.ext` at any depth.
const SENSITIVE_EXTENSIONS: &[&str] = &["pem", "key"];

pub struct SettingsGenerator;

#[derive(Debug, Clone, Serialize)]
pub struct GeneratedSettings {
    pub permissions: PermissionSettings,
}

#[derive(Debug, Clone, Serialize)]
pub struct PermissionSettings {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

impl SettingsGenerator {
    /// Generate settings based on project detection and tech stack.
    ///
    /// `project_root` is needed to verify that sensitive files actually
    /// exist before generating deny rules (evidence-based generation).
    pub fn generate(
        detection: &ProjectDetection,
        tech_stack: &TechStack,
        project_root: &Path,
    ) -> GeneratedSettings {
        let allow = Self::generate_allow_rules(detection, tech_stack);
        let deny = Self::generate_deny_rules(project_root);

        GeneratedSettings {
            permissions: PermissionSettings { allow, deny },
        }
    }

    /// Generate allow rules from detected languages and build tools.
    ///
    /// Maps detected languages to their standard build/test commands.
    /// Also respects explicit build_tools and test_frameworks from TechStack.
    fn generate_allow_rules(detection: &ProjectDetection, tech_stack: &TechStack) -> Vec<String> {
        let mut rules = Vec::new();

        // Collect detected language names (lowercase)
        let detected_languages: Vec<String> = detection
            .languages
            .iter()
            .map(|l| l.language.to_lowercase())
            .collect();

        // Also consider TechStack primary language
        let primary = tech_stack.primary_language.to_lowercase();

        // Rust
        if primary == "rust" || detected_languages.iter().any(|l| l == "rust") {
            rules.push("Bash(cargo test:*)".to_string());
            rules.push("Bash(cargo build:*)".to_string());
            rules.push("Bash(cargo clippy:*)".to_string());
        }

        // Node.js ecosystem (JavaScript / TypeScript)
        if primary == "javascript"
            || primary == "typescript"
            || primary == "node"
            || detected_languages
                .iter()
                .any(|l| l == "javascript" || l == "typescript")
        {
            rules.push("Bash(npm test:*)".to_string());
            rules.push("Bash(npm run build:*)".to_string());
        }

        // Go
        if primary == "go" || detected_languages.iter().any(|l| l == "go") {
            rules.push("Bash(go test:*)".to_string());
            rules.push("Bash(go build:*)".to_string());
        }

        // Python
        if primary == "python" || detected_languages.iter().any(|l| l == "python") {
            rules.push("Bash(pytest:*)".to_string());
            rules.push("Bash(python -m:*)".to_string());
        }

        // Java / Kotlin (Gradle or Maven)
        if primary == "java"
            || primary == "kotlin"
            || primary == "jvm"
            || detected_languages
                .iter()
                .any(|l| l == "java" || l == "kotlin")
        {
            // Check build tools for more specific commands
            if tech_stack.build_tools.iter().any(|t| {
                let t = t.to_lowercase();
                t.contains("gradle")
            }) {
                rules.push("Bash(./gradlew test:*)".to_string());
                rules.push("Bash(./gradlew build:*)".to_string());
            } else if tech_stack.build_tools.iter().any(|t| {
                let t = t.to_lowercase();
                t.contains("maven") || t.contains("mvn")
            }) {
                rules.push("Bash(mvn test:*)".to_string());
                rules.push("Bash(mvn package:*)".to_string());
            } else {
                // Default: include both
                rules.push("Bash(./gradlew test:*)".to_string());
                rules.push("Bash(./gradlew build:*)".to_string());
            }
        }

        // Ruby
        if primary == "ruby" || detected_languages.iter().any(|l| l == "ruby") {
            rules.push("Bash(bundle exec rake test:*)".to_string());
            rules.push("Bash(bundle exec rspec:*)".to_string());
        }

        // Add explicit build tools from TechStack that aren't already covered
        for tool in &tech_stack.build_tools {
            let tool_lower = tool.to_lowercase();
            let already_covered = rules.iter().any(|r| {
                let r_lower = r.to_lowercase();
                r_lower.contains(&tool_lower)
            });
            if !already_covered {
                rules.push(format!("Bash({tool}:*)"));
            }
        }

        // Add explicit test frameworks from TechStack that aren't already covered
        for framework in &tech_stack.test_frameworks {
            let fw_lower = framework.to_lowercase();
            let already_covered = rules.iter().any(|r| {
                let r_lower = r.to_lowercase();
                r_lower.contains(&fw_lower)
            });
            if !already_covered {
                rules.push(format!("Bash({framework}:*)"));
            }
        }

        rules
    }

    /// Generate deny rules for sensitive files that actually exist.
    ///
    /// Only includes rules for files/patterns confirmed present in the project.
    /// This follows the evidence-based generation principle.
    fn generate_deny_rules(project_root: &Path) -> Vec<String> {
        let mut rules = Vec::new();

        // Check exact sensitive file names
        for pattern in SENSITIVE_PATTERNS {
            if project_root.join(pattern).exists() {
                rules.push(format!("Read({pattern})"));
            }
        }

        // Check for .env.* variants (e.g., .env.local, .env.production)
        if let Ok(entries) = std::fs::read_dir(project_root) {
            for entry in entries.filter_map(|e| e.ok()) {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(".env.") && entry.path().is_file() {
                    rules.push(format!("Read({name})"));
                }
            }
        }

        // Check for sensitive extensions anywhere in the project
        let mut found_pem = false;
        let mut found_key = false;
        for ext in SENSITIVE_EXTENSIONS {
            let found = Self::has_files_with_extension(project_root, ext);
            match *ext {
                "pem" if found => found_pem = true,
                "key" if found => found_key = true,
                _ => {}
            }
        }
        if found_pem {
            rules.push("Read(*.pem)".to_string());
        }
        if found_key {
            rules.push("Read(*.key)".to_string());
        }

        rules
    }

    /// Check if any files with the given extension exist under project_root.
    ///
    /// Uses a shallow scan (top-level + one level deep) to avoid expensive
    /// full-tree walks. Most sensitive files (certs, keys) live near the root.
    fn has_files_with_extension(project_root: &Path, ext: &str) -> bool {
        let Ok(entries) = std::fs::read_dir(project_root) else {
            return false;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file()
                && path.extension().is_some_and(|e| e == ext)
            {
                return true;
            }
            // Check one level deep
            if path.is_dir()
                && let Ok(sub_entries) = std::fs::read_dir(&path)
            {
                for sub_entry in sub_entries.filter_map(|e| e.ok()) {
                    let sub_path = sub_entry.path();
                    if sub_path.is_file()
                        && sub_path.extension().is_some_and(|e| e == ext)
                    {
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectType;
    use crate::pipeline::phases::project_detection::{LanguageInfo, ProjectDetection};

    fn make_detection(languages: Vec<(&str, usize, f32)>) -> ProjectDetection {
        ProjectDetection {
            primary_type: ProjectType::Auto,
            confidence: 0.5,
            signals: Vec::new(),
            languages: languages
                .into_iter()
                .map(|(lang, count, pct)| LanguageInfo {
                    language: lang.to_string(),
                    file_count: count,
                    percentage: pct,
                    primary_manifest: None,
                })
                .collect(),
            is_monorepo: false,
            workspace_config: None,
        }
    }

    #[test]
    fn test_rust_allow_rules() {
        let detection = make_detection(vec![("rust", 50, 100.0)]);
        let tech_stack = TechStack::new("rust");
        let dir = tempfile::tempdir().unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings.permissions.allow.contains(&"Bash(cargo test:*)".to_string()));
        assert!(settings.permissions.allow.contains(&"Bash(cargo build:*)".to_string()));
        assert!(settings.permissions.allow.contains(&"Bash(cargo clippy:*)".to_string()));
    }

    #[test]
    fn test_node_allow_rules() {
        let detection = make_detection(vec![("typescript", 100, 80.0), ("javascript", 20, 20.0)]);
        let tech_stack = TechStack::new("typescript");
        let dir = tempfile::tempdir().unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings.permissions.allow.contains(&"Bash(npm test:*)".to_string()));
        assert!(settings.permissions.allow.contains(&"Bash(npm run build:*)".to_string()));
    }

    #[test]
    fn test_go_allow_rules() {
        let detection = make_detection(vec![("go", 30, 100.0)]);
        let tech_stack = TechStack::new("go");
        let dir = tempfile::tempdir().unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings.permissions.allow.contains(&"Bash(go test:*)".to_string()));
        assert!(settings.permissions.allow.contains(&"Bash(go build:*)".to_string()));
    }

    #[test]
    fn test_python_allow_rules() {
        let detection = make_detection(vec![("python", 40, 100.0)]);
        let tech_stack = TechStack::new("python");
        let dir = tempfile::tempdir().unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings.permissions.allow.contains(&"Bash(pytest:*)".to_string()));
        assert!(settings.permissions.allow.contains(&"Bash(python -m:*)".to_string()));
    }

    #[test]
    fn test_java_gradle_allow_rules() {
        let detection = make_detection(vec![("java", 60, 100.0)]);
        let tech_stack = TechStack::new("java").with_build_tool("gradle");
        let dir = tempfile::tempdir().unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings.permissions.allow.contains(&"Bash(./gradlew test:*)".to_string()));
        assert!(settings.permissions.allow.contains(&"Bash(./gradlew build:*)".to_string()));
    }

    #[test]
    fn test_java_maven_allow_rules() {
        let detection = make_detection(vec![("java", 60, 100.0)]);
        let tech_stack = TechStack::new("java").with_build_tool("maven");
        let dir = tempfile::tempdir().unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings.permissions.allow.contains(&"Bash(mvn test:*)".to_string()));
        assert!(settings.permissions.allow.contains(&"Bash(mvn package:*)".to_string()));
    }

    #[test]
    fn test_ruby_allow_rules() {
        let detection = make_detection(vec![("ruby", 25, 100.0)]);
        let tech_stack = TechStack::new("ruby");
        let dir = tempfile::tempdir().unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings
            .permissions
            .allow
            .contains(&"Bash(bundle exec rake test:*)".to_string()));
        assert!(settings
            .permissions
            .allow
            .contains(&"Bash(bundle exec rspec:*)".to_string()));
    }

    #[test]
    fn test_multi_language_allow_rules() {
        let detection = make_detection(vec![("rust", 30, 60.0), ("python", 20, 40.0)]);
        let tech_stack = TechStack::new("rust");
        let dir = tempfile::tempdir().unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        // Should have both Rust and Python rules
        assert!(settings.permissions.allow.contains(&"Bash(cargo test:*)".to_string()));
        assert!(settings.permissions.allow.contains(&"Bash(pytest:*)".to_string()));
    }

    #[test]
    fn test_deny_rules_env_file() {
        let detection = make_detection(vec![("rust", 10, 100.0)]);
        let tech_stack = TechStack::new("rust");
        let dir = tempfile::tempdir().unwrap();

        // Create .env file
        std::fs::write(dir.path().join(".env"), "SECRET=value").unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings.permissions.deny.contains(&"Read(.env)".to_string()));
    }

    #[test]
    fn test_deny_rules_env_variants() {
        let detection = make_detection(vec![("rust", 10, 100.0)]);
        let tech_stack = TechStack::new("rust");
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(dir.path().join(".env.local"), "SECRET=value").unwrap();
        std::fs::write(dir.path().join(".env.production"), "PROD=value").unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings.permissions.deny.contains(&"Read(.env.local)".to_string()));
        assert!(settings
            .permissions
            .deny
            .contains(&"Read(.env.production)".to_string()));
    }

    #[test]
    fn test_deny_rules_credentials_json() {
        let detection = make_detection(vec![("typescript", 50, 100.0)]);
        let tech_stack = TechStack::new("typescript");
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(dir.path().join("credentials.json"), "{}").unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings
            .permissions
            .deny
            .contains(&"Read(credentials.json)".to_string()));
    }

    #[test]
    fn test_deny_rules_pem_files() {
        let detection = make_detection(vec![("go", 20, 100.0)]);
        let tech_stack = TechStack::new("go");
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(dir.path().join("server.pem"), "-----BEGIN CERT-----").unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings.permissions.deny.contains(&"Read(*.pem)".to_string()));
    }

    #[test]
    fn test_deny_rules_key_files() {
        let detection = make_detection(vec![("python", 15, 100.0)]);
        let tech_stack = TechStack::new("python");
        let dir = tempfile::tempdir().unwrap();

        // Create a key file one level deep
        let certs_dir = dir.path().join("certs");
        std::fs::create_dir(&certs_dir).unwrap();
        std::fs::write(certs_dir.join("private.key"), "key data").unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings.permissions.deny.contains(&"Read(*.key)".to_string()));
    }

    #[test]
    fn test_no_deny_rules_when_no_sensitive_files() {
        let detection = make_detection(vec![("rust", 50, 100.0)]);
        let tech_stack = TechStack::new("rust");
        let dir = tempfile::tempdir().unwrap();

        // Create only non-sensitive files
        std::fs::write(dir.path().join("README.md"), "# Project").unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings.permissions.deny.is_empty());
    }

    #[test]
    fn test_no_allow_rules_for_unknown_language() {
        let detection = make_detection(vec![("brainfuck", 5, 100.0)]);
        let tech_stack = TechStack::new("brainfuck");
        let dir = tempfile::tempdir().unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings.permissions.allow.is_empty());
    }

    #[test]
    fn test_explicit_build_tools_added() {
        let detection = make_detection(vec![("rust", 50, 100.0)]);
        let tech_stack = TechStack::new("rust").with_build_tool("make");
        let dir = tempfile::tempdir().unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings.permissions.allow.contains(&"Bash(cargo test:*)".to_string()));
        assert!(settings.permissions.allow.contains(&"Bash(make:*)".to_string()));
    }

    #[test]
    fn test_explicit_test_framework_added() {
        let detection = make_detection(vec![("python", 30, 100.0)]);
        let tech_stack = TechStack::new("python").with_test_framework("tox");
        let dir = tempfile::tempdir().unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings.permissions.allow.contains(&"Bash(pytest:*)".to_string()));
        assert!(settings.permissions.allow.contains(&"Bash(tox:*)".to_string()));
    }

    #[test]
    fn test_no_duplicate_explicit_tools() {
        let detection = make_detection(vec![("rust", 50, 100.0)]);
        // "cargo" is already covered by Rust language detection
        let tech_stack = TechStack::new("rust").with_build_tool("cargo");
        let dir = tempfile::tempdir().unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        // Should not have a duplicate "Bash(cargo:*)" since cargo is already covered
        let cargo_count = settings
            .permissions
            .allow
            .iter()
            .filter(|r| r.contains("cargo"))
            .count();
        // cargo test, cargo build, cargo clippy = 3 (no extra "Bash(cargo:*)")
        assert_eq!(cargo_count, 3);
    }

    #[test]
    fn test_serialization() {
        let detection = make_detection(vec![("rust", 50, 100.0)]);
        let tech_stack = TechStack::new("rust");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "SECRET=x").unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());
        let json = serde_json::to_string_pretty(&settings).unwrap();

        assert!(json.contains("\"permissions\""));
        assert!(json.contains("\"allow\""));
        assert!(json.contains("\"deny\""));
        assert!(json.contains("cargo test"));
        assert!(json.contains(".env"));
    }

    #[test]
    fn test_detection_from_primary_language_only() {
        // No detected languages in detection, but primary_language in tech_stack
        let detection = make_detection(vec![]);
        let tech_stack = TechStack::new("go");
        let dir = tempfile::tempdir().unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings.permissions.allow.contains(&"Bash(go test:*)".to_string()));
        assert!(settings.permissions.allow.contains(&"Bash(go build:*)".to_string()));
    }

    #[test]
    fn test_secrets_yaml() {
        let detection = make_detection(vec![]);
        let tech_stack = TechStack::new("python");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("secrets.yaml"), "key: val").unwrap();

        let settings = SettingsGenerator::generate(&detection, &tech_stack, dir.path());

        assert!(settings
            .permissions
            .deny
            .contains(&"Read(secrets.yaml)".to_string()));
    }
}
