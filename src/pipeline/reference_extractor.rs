//! Reference Extractor Module
//!
//! Extracts file:line references for progressive disclosure.
//! Provides specific entry points, key functions, and important patterns.

use std::path::Path;

use tokio::fs;

use crate::config::ProjectType;
use crate::types::Result;

#[derive(Debug, Clone)]
pub struct FileReference {
    pub path: String,
    pub line: Option<usize>,
    pub name: String,
    pub reference_type: ReferenceType,
}

impl FileReference {
    pub fn to_string_ref(&self) -> String {
        if let Some(line) = self.line {
            format!("@{}:{}", self.path, line)
        } else {
            format!("@{}", self.path)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceType {
    EntryPoint,
    KeyFunction,
    TypeDefinition,
    ConfigFile,
    TestFile,
    Module,
}

pub struct ReferenceExtractor;

impl ReferenceExtractor {
    pub async fn extract_key_references(
        project_root: &Path,
        project_type: ProjectType,
    ) -> Result<Vec<FileReference>> {
        let mut references = Vec::new();

        references.extend(Self::extract_entry_points(project_root, project_type).await?);
        references.extend(Self::extract_key_files(project_root, project_type).await?);

        Ok(references)
    }

    async fn extract_entry_points(
        project_root: &Path,
        project_type: ProjectType,
    ) -> Result<Vec<FileReference>> {
        let mut refs = Vec::new();

        match project_type {
            ProjectType::Cli => {
                if let Ok(line) = Self::find_main_function(project_root, "src/main.rs").await {
                    refs.push(FileReference {
                        path: "src/main.rs".to_string(),
                        line: Some(line),
                        name: "main".to_string(),
                        reference_type: ReferenceType::EntryPoint,
                    });
                }

                if let Ok(lines) = Self::find_command_handlers(project_root).await {
                    for (path, line, name) in lines {
                        refs.push(FileReference {
                            path,
                            line: Some(line),
                            name,
                            reference_type: ReferenceType::KeyFunction,
                        });
                    }
                }
            }
            ProjectType::Backend => {
                if let Ok(lines) = Self::find_api_routes(project_root).await {
                    for (path, line, name) in lines {
                        refs.push(FileReference {
                            path,
                            line: Some(line),
                            name,
                            reference_type: ReferenceType::EntryPoint,
                        });
                    }
                }
            }
            ProjectType::Library => {
                if let Ok(line) = Self::find_lib_entry(project_root).await {
                    refs.push(FileReference {
                        path: "src/lib.rs".to_string(),
                        line: Some(line),
                        name: "public API".to_string(),
                        reference_type: ReferenceType::EntryPoint,
                    });
                }
            }
            _ => {
                if let Ok(entries) = Self::find_generic_entries(project_root).await {
                    refs.extend(entries);
                }
            }
        }

        Ok(refs)
    }

    async fn extract_key_files(
        project_root: &Path,
        project_type: ProjectType,
    ) -> Result<Vec<FileReference>> {
        let mut refs = Vec::new();

        let key_patterns: Vec<(&str, ReferenceType)> = match project_type {
            ProjectType::Cli => vec![
                ("Cargo.toml", ReferenceType::ConfigFile),
                ("src/cli/mod.rs", ReferenceType::Module),
                ("src/config/mod.rs", ReferenceType::Module),
            ],
            ProjectType::Backend => vec![
                ("src/api/mod.rs", ReferenceType::Module),
                ("src/domain/mod.rs", ReferenceType::Module),
                ("src/routes.rs", ReferenceType::EntryPoint),
            ],
            ProjectType::Frontend => vec![
                ("package.json", ReferenceType::ConfigFile),
                ("src/App.tsx", ReferenceType::EntryPoint),
                ("src/main.tsx", ReferenceType::EntryPoint),
            ],
            ProjectType::Library => vec![
                ("Cargo.toml", ReferenceType::ConfigFile),
                ("src/lib.rs", ReferenceType::EntryPoint),
            ],
            _ => vec![],
        };

        for (path, ref_type) in key_patterns {
            if project_root.join(path).exists() {
                refs.push(FileReference {
                    path: path.to_string(),
                    line: None,
                    name: path.to_string(),
                    reference_type: ref_type,
                });
            }
        }

        Ok(refs)
    }

    async fn find_main_function(project_root: &Path, file_path: &str) -> Result<usize> {
        let full_path = project_root.join(file_path);
        if !full_path.exists() {
            return Err(crate::types::ClaudegenError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "File not found",
            )));
        }

        let content = fs::read_to_string(&full_path).await?;
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            // Skip comments
            if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
                continue;
            }
            if trimmed.contains("fn main(") || trimmed.contains("async fn main(") {
                return Ok(i + 1);
            }
        }

        Err(crate::types::ClaudegenError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "main function not found",
        )))
    }

    async fn find_command_handlers(project_root: &Path) -> Result<Vec<(String, usize, String)>> {
        let mut handlers = Vec::new();

        let commands_dir = project_root.join("src/cli/commands");
        if commands_dir.exists()
            && let Ok(mut entries) = fs::read_dir(&commands_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "rs") {
                        let rel_path = path.strip_prefix(project_root).unwrap_or(&path);
                        let file_name = path.file_stem().unwrap_or_default().to_string_lossy();

                        if file_name != "mod"
                            && let Ok(content) = fs::read_to_string(&path).await {
                                for (i, line) in content.lines().enumerate() {
                                    let trimmed = line.trim();
                                    // Skip comments
                                    if trimmed.starts_with("//") || trimmed.starts_with("/*") {
                                        continue;
                                    }
                                    if trimmed.contains("pub fn run(")
                                        || trimmed.contains("pub async fn run(")
                                    {
                                        handlers.push((
                                            rel_path.to_string_lossy().to_string(),
                                            i + 1,
                                            format!("{} command", file_name),
                                        ));
                                        break;
                                    }
                                }
                            }
                    }
                }
            }

        Ok(handlers)
    }

    async fn find_api_routes(project_root: &Path) -> Result<Vec<(String, usize, String)>> {
        let mut routes = Vec::new();

        let candidates = ["src/api/routes.rs", "src/routes.rs", "src/api/mod.rs"];

        for candidate in candidates {
            let path = project_root.join(candidate);
            if path.exists()
                && let Ok(content) = fs::read_to_string(&path).await {
                    for (i, line) in content.lines().enumerate() {
                        let trimmed = line.trim();
                        // Skip comments
                        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
                            continue;
                        }
                        if trimmed.contains("#[get(")
                            || trimmed.contains("#[post(")
                            || trimmed.contains("#[put(")
                            || trimmed.contains("#[delete(")
                            || trimmed.contains(".route(")
                        {
                            let name = Self::extract_route_name(trimmed);
                            routes.push((candidate.to_string(), i + 1, name));
                        }
                    }
                }
        }

        Ok(routes)
    }

    fn extract_route_name(line: &str) -> String {
        // Find the first quoted string safely using char indices
        let chars: Vec<char> = line.chars().collect();
        let mut in_quote = false;
        let mut start_idx = 0;

        for (i, &ch) in chars.iter().enumerate() {
            if ch == '"' {
                if !in_quote {
                    in_quote = true;
                    start_idx = i + 1;
                } else {
                    // Found the closing quote - extract the content
                    return chars[start_idx..i].iter().collect();
                }
            }
        }

        "route".to_string()
    }

    async fn find_lib_entry(project_root: &Path) -> Result<usize> {
        let lib_path = project_root.join("src/lib.rs");
        if !lib_path.exists() {
            return Err(crate::types::ClaudegenError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "lib.rs not found",
            )));
        }

        let content = fs::read_to_string(&lib_path).await?;
        for (i, line) in content.lines().enumerate() {
            if line.starts_with("pub mod") || line.starts_with("pub use") {
                return Ok(i + 1);
            }
        }

        Ok(1)
    }

    async fn find_generic_entries(project_root: &Path) -> Result<Vec<FileReference>> {
        let mut refs = Vec::new();

        let candidates = [
            ("src/main.rs", "main"),
            ("src/lib.rs", "lib"),
            ("main.go", "main"),
            ("index.ts", "index"),
            ("index.js", "index"),
        ];

        for (path, name) in candidates {
            if project_root.join(path).exists() {
                refs.push(FileReference {
                    path: path.to_string(),
                    line: Some(1),
                    name: name.to_string(),
                    reference_type: ReferenceType::EntryPoint,
                });
            }
        }

        Ok(refs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_reference_format() {
        let with_line = FileReference {
            path: "src/main.rs".to_string(),
            line: Some(42),
            name: "main".to_string(),
            reference_type: ReferenceType::EntryPoint,
        };
        assert_eq!(with_line.to_string_ref(), "@src/main.rs:42");

        let without_line = FileReference {
            path: "src/lib.rs".to_string(),
            line: None,
            name: "lib".to_string(),
            reference_type: ReferenceType::Module,
        };
        assert_eq!(without_line.to_string_ref(), "@src/lib.rs");
    }

    #[test]
    fn test_extract_route_name() {
        assert_eq!(
            ReferenceExtractor::extract_route_name(r#"#[get("/api/users")]"#),
            "/api/users"
        );
        assert_eq!(
            ReferenceExtractor::extract_route_name(r#".route("/health", get(health_check))"#),
            "/health"
        );
    }
}
