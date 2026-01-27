//! Reference Extractor Module
//!
//! Extracts file references for LLM context.
//! Uses file existence checks only - no content parsing.
//! LLM determines significance from context.

use std::path::Path;

use crate::types::Result;

/// A code reference pointing to a key file location
#[derive(Debug, Clone)]
pub struct CodeReference {
    pub path: String,
    pub line: Option<usize>,
    pub name: String,
    pub reference_type: ReferenceType,
}

impl CodeReference {
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
    ConfigFile,
    Module,
}

pub struct ReferenceExtractor;

impl ReferenceExtractor {
    /// Extract key file references from project
    /// File existence checks only - LLM determines significance
    pub async fn extract_key_references(project_root: &Path) -> Result<Vec<CodeReference>> {
        let patterns: &[(&str, ReferenceType)] = &[
            // Configuration files
            ("Cargo.toml", ReferenceType::ConfigFile),
            ("package.json", ReferenceType::ConfigFile),
            ("pyproject.toml", ReferenceType::ConfigFile),
            ("go.mod", ReferenceType::ConfigFile),
            ("build.gradle", ReferenceType::ConfigFile),
            ("build.gradle.kts", ReferenceType::ConfigFile),
            ("pom.xml", ReferenceType::ConfigFile),
            ("tsconfig.json", ReferenceType::ConfigFile),
            ("Makefile", ReferenceType::ConfigFile),
            // Entry points
            ("src/main.rs", ReferenceType::EntryPoint),
            ("src/lib.rs", ReferenceType::EntryPoint),
            ("src/index.ts", ReferenceType::EntryPoint),
            ("src/index.js", ReferenceType::EntryPoint),
            ("src/main.ts", ReferenceType::EntryPoint),
            ("src/main.tsx", ReferenceType::EntryPoint),
            ("src/App.tsx", ReferenceType::EntryPoint),
            ("src/main.py", ReferenceType::EntryPoint),
            ("main.go", ReferenceType::EntryPoint),
            ("cmd/main.go", ReferenceType::EntryPoint),
            ("app.py", ReferenceType::EntryPoint),
            ("app/main.py", ReferenceType::EntryPoint),
            ("index.ts", ReferenceType::EntryPoint),
            ("index.js", ReferenceType::EntryPoint),
            // Module directories
            ("src/api/", ReferenceType::Module),
            ("src/domain/", ReferenceType::Module),
            ("src/services/", ReferenceType::Module),
            ("src/handlers/", ReferenceType::Module),
            ("src/routes/", ReferenceType::Module),
            ("src/models/", ReferenceType::Module),
            ("src/utils/", ReferenceType::Module),
            ("internal/", ReferenceType::Module),
            ("pkg/", ReferenceType::Module),
            ("lib/", ReferenceType::Module),
        ];

        let refs = patterns
            .iter()
            .filter(|(path, _)| project_root.join(path).exists())
            .map(|(path, ref_type)| CodeReference {
                path: path.to_string(),
                line: None,
                name: path.to_string(),
                reference_type: *ref_type,
            })
            .collect();

        Ok(refs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_reference_format() {
        let with_line = CodeReference {
            path: "src/main.rs".to_string(),
            line: Some(42),
            name: "main".to_string(),
            reference_type: ReferenceType::EntryPoint,
        };
        assert_eq!(with_line.to_string_ref(), "@src/main.rs:42");

        let without_line = CodeReference {
            path: "src/lib.rs".to_string(),
            line: None,
            name: "lib".to_string(),
            reference_type: ReferenceType::Module,
        };
        assert_eq!(without_line.to_string_ref(), "@src/lib.rs");
    }
}
