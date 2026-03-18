//! Manifest-based Dependency Parsing
//!
//! Extracts dependency names from project manifest files across multiple ecosystems.
//! Uses deterministic parsing - no LLM inference needed for dependency extraction.

use std::path::Path;

use tokio::fs;

/// Trait for parsing dependencies from manifest files
pub trait ManifestParser: Send + Sync {
    /// Parser name for logging
    fn name(&self) -> &str;

    /// Manifest file names this parser can handle
    fn manifest_files(&self) -> &[&str];

    /// Parse dependencies from manifest content
    /// Returns list of dependency names (without versions)
    fn parse_dependencies(&self, content: &str) -> Vec<String>;
}

/// Parse dependencies for a subproject
///
/// Tries all parsers in order until one succeeds.
/// Returns empty vec if no manifest found or parsing fails.
pub async fn parse_subproject_dependencies(
    project_root: &Path,
    subproject_path: &str,
) -> Vec<String> {
    let parsers: Vec<Box<dyn ManifestParser>> = vec![
        Box::new(CargoManifestParser),
        Box::new(NodeManifestParser),
        Box::new(GradleManifestParser),
        Box::new(MavenManifestParser),
        Box::new(GoManifestParser),
    ];

    for parser in parsers {
        for manifest_file in parser.manifest_files() {
            let manifest_path = project_root.join(subproject_path).join(manifest_file);

            if let Ok(content) = fs::read_to_string(&manifest_path).await {
                let deps = parser.parse_dependencies(&content);
                if !deps.is_empty() {
                    tracing::debug!(
                        parser = parser.name(),
                        subproject = subproject_path,
                        manifest = manifest_file,
                        deps = deps.len(),
                        "Parsed dependencies"
                    );
                    return deps;
                }
            }
        }
    }

    Vec::new()
}

// =============================================================================
// RUST - Cargo.toml
// =============================================================================

pub struct CargoManifestParser;

impl ManifestParser for CargoManifestParser {
    fn name(&self) -> &str {
        "cargo"
    }

    fn manifest_files(&self) -> &[&str] {
        &["Cargo.toml"]
    }

    fn parse_dependencies(&self, content: &str) -> Vec<String> {
        let mut deps = Vec::new();
        let mut in_deps_section = false;
        let mut in_table = false;

        for line in content.lines() {
            let trimmed = line.trim();

            // Check for dependency sections
            if trimmed == "[dependencies]"
                || trimmed == "[dev-dependencies]"
                || trimmed == "[build-dependencies]"
            {
                in_deps_section = true;
                in_table = false;
                continue;
            }

            // End of dependencies section (new section starts)
            if trimmed.starts_with('[') && in_deps_section {
                in_deps_section = false;
                in_table = false;
                continue;
            }

            if !in_deps_section {
                continue;
            }

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Inline table (e.g., serde = { version = "1.0", features = ["derive"] })
            if trimmed.contains('{') && !in_table {
                if let Some(dep_name) = trimmed.split('=').next() {
                    let name = dep_name.trim().to_string();
                    if !name.is_empty() && is_valid_package_name(&name) {
                        deps.push(name);
                    }
                }
                continue;
            }

            // Simple dependency (e.g., tokio = "1.0")
            if let Some(dep_name) = trimmed.split('=').next() {
                let name = dep_name.trim();
                if !name.is_empty() && is_valid_package_name(name) {
                    deps.push(name.to_string());
                }
            }
        }

        deps
    }
}

// =============================================================================
// NODE - package.json
// =============================================================================

pub struct NodeManifestParser;

impl ManifestParser for NodeManifestParser {
    fn name(&self) -> &str {
        "node"
    }

    fn manifest_files(&self) -> &[&str] {
        &["package.json"]
    }

    fn parse_dependencies(&self, content: &str) -> Vec<String> {
        let mut deps = Vec::new();
        let mut in_deps = false;
        let mut brace_depth = 0;

        for line in content.lines() {
            let trimmed = line.trim();

            // Start of dependencies or devDependencies
            if trimmed.starts_with("\"dependencies\":")
                || trimmed.starts_with("\"devDependencies\":")
                || trimmed.starts_with("\"peerDependencies\":")
            {
                in_deps = true;
                brace_depth = 0;
                continue;
            }

            if !in_deps {
                continue;
            }

            // Track brace depth
            brace_depth += trimmed.matches('{').count() as i32;
            brace_depth -= trimmed.matches('}').count() as i32;

            // End of dependencies section
            if brace_depth < 0 {
                in_deps = false;
                continue;
            }

            // Extract package name (e.g., "react": "^18.0.0",)
            if let Some(pkg_start) = trimmed.find('"') {
                if let Some(pkg_end) = trimmed[pkg_start + 1..].find('"') {
                    let pkg_name = &trimmed[pkg_start + 1..pkg_start + 1 + pkg_end];
                    if !pkg_name.is_empty() && is_valid_package_name(pkg_name) {
                        deps.push(pkg_name.to_string());
                    }
                }
            }
        }

        deps
    }
}

// =============================================================================
// GRADLE - build.gradle / build.gradle.kts
// =============================================================================

pub struct GradleManifestParser;

impl ManifestParser for GradleManifestParser {
    fn name(&self) -> &str {
        "gradle"
    }

    fn manifest_files(&self) -> &[&str] {
        &["build.gradle", "build.gradle.kts"]
    }

    fn parse_dependencies(&self, content: &str) -> Vec<String> {
        let mut deps = Vec::new();
        let mut in_deps_block = false;

        for line in content.lines() {
            let trimmed = line.trim();

            // Start of dependencies block
            if trimmed.starts_with("dependencies") && trimmed.contains('{') {
                in_deps_block = true;
                continue;
            }

            // End of dependencies block
            if in_deps_block && trimmed.starts_with('}') {
                in_deps_block = false;
                continue;
            }

            if !in_deps_block {
                continue;
            }

            // Parse dependency declarations
            // implementation("group:artifact:version")
            // implementation 'group:artifact:version'
            // testImplementation(...)
            if let Some(dep) = extract_gradle_dependency(trimmed) {
                deps.push(dep);
            }
        }

        deps
    }
}

fn extract_gradle_dependency(line: &str) -> Option<String> {
    // Find the quoted string or parentheses content
    let start = line.find(|c| c == '"' || c == '\'')?;
    let quote_char = line.chars().nth(start)?;
    let end = line[start + 1..].find(quote_char)?;
    let dep_string = &line[start + 1..start + 1 + end];

    // Parse "group:artifact:version" format
    let parts: Vec<&str> = dep_string.split(':').collect();
    if parts.len() >= 2 {
        // Return "group:artifact"
        Some(format!("{}:{}", parts[0], parts[1]))
    } else {
        None
    }
}

// =============================================================================
// MAVEN - pom.xml
// =============================================================================

pub struct MavenManifestParser;

impl ManifestParser for MavenManifestParser {
    fn name(&self) -> &str {
        "maven"
    }

    fn manifest_files(&self) -> &[&str] {
        &["pom.xml"]
    }

    fn parse_dependencies(&self, content: &str) -> Vec<String> {
        let mut deps = Vec::new();
        let mut in_deps = false;
        let mut current_group = None;
        let mut current_artifact = None;

        for line in content.lines() {
            let trimmed = line.trim();

            // Start of dependencies section
            if trimmed.starts_with("<dependencies>") {
                in_deps = true;
                continue;
            }

            // End of dependencies section
            if trimmed.starts_with("</dependencies>") {
                in_deps = false;
                continue;
            }

            if !in_deps {
                continue;
            }

            // Extract groupId
            if trimmed.starts_with("<groupId>") {
                if let Some(group) = extract_xml_content(trimmed, "groupId") {
                    current_group = Some(group);
                }
            }

            // Extract artifactId
            if trimmed.starts_with("<artifactId>") {
                if let Some(artifact) = extract_xml_content(trimmed, "artifactId") {
                    current_artifact = Some(artifact);
                }
            }

            // When we have both, combine and reset
            if let (Some(group), Some(artifact)) = (&current_group, &current_artifact) {
                deps.push(format!("{}:{}", group, artifact));
                current_group = None;
                current_artifact = None;
            }
        }

        deps
    }
}

fn extract_xml_content(line: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{}>", tag);
    let end_tag = format!("</{}>", tag);

    let start = line.find(&start_tag)? + start_tag.len();
    let end = line.find(&end_tag)?;

    if start < end {
        Some(line[start..end].trim().to_string())
    } else {
        None
    }
}

// =============================================================================
// GO - go.mod
// =============================================================================

pub struct GoManifestParser;

impl ManifestParser for GoManifestParser {
    fn name(&self) -> &str {
        "go"
    }

    fn manifest_files(&self) -> &[&str] {
        &["go.mod"]
    }

    fn parse_dependencies(&self, content: &str) -> Vec<String> {
        let mut deps = Vec::new();
        let mut in_require = false;

        for line in content.lines() {
            let trimmed = line.trim();

            // Start of require block
            if trimmed.starts_with("require") {
                in_require = true;
                // Single-line require: require github.com/foo/bar v1.0.0
                if !trimmed.contains('(') {
                    if let Some(dep) = extract_go_module(trimmed) {
                        deps.push(dep);
                    }
                    in_require = false;
                }
                continue;
            }

            // End of require block
            if in_require && trimmed.starts_with(')') {
                in_require = false;
                continue;
            }

            if !in_require {
                continue;
            }

            // Extract module path (e.g., "github.com/foo/bar v1.0.0")
            if let Some(dep) = extract_go_module(trimmed) {
                deps.push(dep);
            }
        }

        deps
    }
}

fn extract_go_module(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("//") {
        return None;
    }

    // Remove "require" keyword if present
    let cleaned = trimmed.strip_prefix("require").unwrap_or(trimmed).trim();

    // Split by whitespace and take first part (module path)
    let parts: Vec<&str> = cleaned.split_whitespace().collect();
    if let Some(module) = parts.first() {
        if module.contains('/') && !module.starts_with("//") {
            return Some(module.to_string());
        }
    }

    None
}

// =============================================================================
// UTILITIES
// =============================================================================

fn is_valid_package_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('[')
        && !name.starts_with('#')
        && name != "workspace"
        && !name.ends_with(".workspace")
        && name.chars().any(|c| c.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cargo_parser() {
        let manifest = r#"
[package]
name = "test"

[dependencies]
tokio = "1.0"
serde = { version = "1.0", features = ["derive"] }
anyhow = "1.0"

[dev-dependencies]
mockito = "0.31"
"#;

        let parser = CargoManifestParser;
        let deps = parser.parse_dependencies(manifest);

        assert!(deps.contains(&"tokio".to_string()));
        assert!(deps.contains(&"serde".to_string()));
        assert!(deps.contains(&"anyhow".to_string()));
        assert!(deps.contains(&"mockito".to_string()));
    }

    #[test]
    fn test_node_parser() {
        let manifest = r#"{
  "name": "test",
  "dependencies": {
    "react": "^18.0.0",
    "express": "~4.18.0"
  },
  "devDependencies": {
    "jest": "^29.0.0"
  }
}"#;

        let parser = NodeManifestParser;
        let deps = parser.parse_dependencies(manifest);

        assert!(deps.contains(&"react".to_string()));
        assert!(deps.contains(&"express".to_string()));
        assert!(deps.contains(&"jest".to_string()));
    }

    #[test]
    fn test_gradle_parser() {
        let manifest = r#"
dependencies {
    implementation("com.google.guava:guava:31.0-jre")
    testImplementation 'org.junit.jupiter:junit-jupiter:5.8.2'
}
"#;

        let parser = GradleManifestParser;
        let deps = parser.parse_dependencies(manifest);

        assert!(deps.contains(&"com.google.guava:guava".to_string()));
        assert!(deps.contains(&"org.junit.jupiter:junit-jupiter".to_string()));
    }

    #[test]
    fn test_maven_parser() {
        let manifest = r#"
<dependencies>
    <dependency>
        <groupId>org.springframework.boot</groupId>
        <artifactId>spring-boot-starter-web</artifactId>
        <version>2.7.0</version>
    </dependency>
</dependencies>
"#;

        let parser = MavenManifestParser;
        let deps = parser.parse_dependencies(manifest);

        assert!(deps.contains(&"org.springframework.boot:spring-boot-starter-web".to_string()));
    }

    #[test]
    fn test_go_parser() {
        let manifest = r#"
module example.com/myapp

go 1.19

require (
    github.com/gin-gonic/gin v1.8.1
    github.com/go-sql-driver/mysql v1.6.0
)
"#;

        let parser = GoManifestParser;
        let deps = parser.parse_dependencies(manifest);

        assert!(deps.contains(&"github.com/gin-gonic/gin".to_string()));
        assert!(deps.contains(&"github.com/go-sql-driver/mysql".to_string()));
    }

    #[tokio::test]
    async fn test_parse_subproject_empty() {
        let temp_dir = std::env::temp_dir();
        let deps = parse_subproject_dependencies(&temp_dir, "nonexistent").await;
        assert!(deps.is_empty());
    }
}
