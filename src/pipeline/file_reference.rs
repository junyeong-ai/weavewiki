//! File Reference Parsing and Validation
//!
//! Parses @path:line references and validates against VerifiedFileRegistry.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::context::VerifiedFileRegistry;

// Re-export the pattern from utils for consistency
pub use crate::utils::patterns::FILE_REFERENCE_PATTERN;

/// Parsed file reference with full metadata
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileReference {
    /// Raw reference string (e.g., "@src/main.rs:42")
    pub raw: String,
    /// Extracted path (e.g., "src/main.rs")
    pub path: String,
    /// Start line number if specified
    pub line_start: Option<u32>,
    /// End line number if specified (for ranges)
    pub line_end: Option<u32>,
}

impl FileReference {
    /// Parse a file reference from raw text
    pub fn parse(raw: &str) -> Option<Self> {
        FILE_REFERENCE_PATTERN.captures(raw).and_then(|cap| {
            // Group 1 = quoted path, Group 2 = unquoted path
            let path = cap
                .get(1)
                .or_else(|| cap.get(2))
                .map(|m| m.as_str().to_string())?;

            // Skip non-file references
            if path.is_empty()
                || path.starts_with("http")
                || path.starts_with("https")
                || path.starts_with("CLAUDE")
                || path.contains('@')
                || path.chars().all(|c| c.is_ascii_digit())
            {
                return None;
            }

            // Line numbers are in groups 3 and 4 now
            let line_start = cap.get(3).and_then(|m| m.as_str().parse().ok());
            let line_end = cap.get(4).and_then(|m| m.as_str().parse().ok());

            Some(Self {
                raw: raw.to_string(),
                path,
                line_start,
                line_end,
            })
        })
    }

    /// Check if this reference includes line numbers
    pub fn has_line_info(&self) -> bool {
        self.line_start.is_some()
    }

    /// Check if this is a line range reference
    pub fn has_range(&self) -> bool {
        self.line_start.is_some() && self.line_end.is_some()
    }

    /// Get depth level: 0 = file only, 1 = file+line, 2 = file+line+range
    pub fn depth_level(&self) -> u8 {
        match (self.line_start, self.line_end) {
            (Some(_), Some(_)) => 2,
            (Some(_), None) => 1,
            _ => 0,
        }
    }
}

/// Extract all file references from content
pub fn extract_references(content: &str) -> Vec<FileReference> {
    FILE_REFERENCE_PATTERN
        .captures_iter(content)
        .filter_map(|cap| {
            let full_match = cap.get(0)?.as_str();
            FileReference::parse(full_match)
        })
        .collect()
}

/// Count file references in content
pub fn count_references(content: &str) -> usize {
    extract_references(content).len()
}

/// Count references with line numbers
pub fn count_references_with_lines(content: &str) -> usize {
    extract_references(content)
        .into_iter()
        .filter(|r| r.has_line_info())
        .count()
}

/// Count references with line ranges
pub fn count_references_with_ranges(content: &str) -> usize {
    extract_references(content)
        .into_iter()
        .filter(|r| r.has_range())
        .count()
}

/// Validation result for a file reference
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferenceValidation {
    Valid,
    FileNotFound,
    LineOutOfRange {
        referenced: u32,
        actual_max: usize,
    },
    RangeOutOfBounds {
        start: u32,
        end: u32,
        actual_max: usize,
    },
    InvalidRange {
        start: u32,
        end: u32,
    },
}

impl ReferenceValidation {
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }
}

/// Resolved file reference with validation status
#[derive(Debug, Clone)]
pub struct ResolvedReference {
    pub reference: FileReference,
    pub resolved_path: Option<PathBuf>,
    pub validation: ReferenceValidation,
}

impl ResolvedReference {
    pub fn is_valid(&self) -> bool {
        self.validation.is_valid()
    }
}

/// Path resolver with 6-step resolution strategy
pub struct PathResolver<'a> {
    project_root: &'a Path,
    registry: &'a VerifiedFileRegistry,
}

impl<'a> PathResolver<'a> {
    pub fn new(project_root: &'a Path, registry: &'a VerifiedFileRegistry) -> Self {
        Self {
            project_root,
            registry,
        }
    }

    /// Resolve a path using 6-step strategy
    pub fn resolve(&self, path: &str) -> Option<PathBuf> {
        // 1. Direct match
        let direct = self.project_root.join(path);
        if direct.exists() {
            return Some(direct);
        }
        if self.registry.contains(path) {
            return Some(direct);
        }

        // 2. Add src/ prefix
        let with_src = self.project_root.join("src").join(path);
        if with_src.exists() {
            return Some(with_src);
        }

        // 3. Remove src/ prefix
        if let Some(without_src) = path.strip_prefix("src/") {
            let candidate = self.project_root.join(without_src);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        // 4. Common prefixes
        for prefix in &["lib/", "crates/", "packages/", "apps/"] {
            let with_prefix = self.project_root.join(prefix).join(path);
            if with_prefix.exists() {
                return Some(with_prefix);
            }
        }

        // 5. Basename-only unique match
        if let Some(basename) = Path::new(path).file_name().and_then(|n| n.to_str()) {
            let matches: Vec<_> = self
                .registry
                .all_files()
                .filter(|f: &&String| f.ends_with(basename))
                .collect();

            if matches.len() == 1 {
                return Some(self.project_root.join(matches[0]));
            }
        }

        // 6. Case-insensitive match
        let path_lower = path.to_lowercase();
        for file in self.registry.all_files() {
            if file.to_lowercase() == path_lower {
                return Some(self.project_root.join(file));
            }
        }

        None
    }

    /// Resolve and validate a reference
    pub fn resolve_and_validate(&self, reference: &FileReference) -> ResolvedReference {
        let resolved_path = self.resolve(&reference.path);

        let validation = match &resolved_path {
            None => ReferenceValidation::FileNotFound,
            Some(path) => self.validate_line_numbers(reference, path),
        };

        ResolvedReference {
            reference: reference.clone(),
            resolved_path,
            validation,
        }
    }

    fn validate_line_numbers(&self, reference: &FileReference, path: &Path) -> ReferenceValidation {
        let Some(start) = reference.line_start else {
            return ReferenceValidation::Valid;
        };

        // Validate range ordering
        if let Some(end) = reference.line_end
            && start > end
        {
            return ReferenceValidation::InvalidRange { start, end };
        }

        // Read file and count lines
        let Ok(content) = std::fs::read_to_string(path) else {
            return ReferenceValidation::Valid; // Can't read = assume valid
        };
        let line_count = content.lines().count();

        if start as usize > line_count {
            return ReferenceValidation::LineOutOfRange {
                referenced: start,
                actual_max: line_count,
            };
        }

        if let Some(end) = reference.line_end
            && end as usize > line_count
        {
            return ReferenceValidation::RangeOutOfBounds {
                start,
                end,
                actual_max: line_count,
            };
        }

        ReferenceValidation::Valid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_file_only() {
        let r = FileReference::parse("@src/main.rs").unwrap();
        assert_eq!(r.path, "src/main.rs");
        assert!(r.line_start.is_none());
        assert_eq!(r.depth_level(), 0);
    }

    #[test]
    fn test_parse_file_line() {
        let r = FileReference::parse("@src/main.rs:42").unwrap();
        assert_eq!(r.path, "src/main.rs");
        assert_eq!(r.line_start, Some(42));
        assert!(r.line_end.is_none());
        assert_eq!(r.depth_level(), 1);
    }

    #[test]
    fn test_parse_file_range() {
        let r = FileReference::parse("@src/main.rs:42-50").unwrap();
        assert_eq!(r.path, "src/main.rs");
        assert_eq!(r.line_start, Some(42));
        assert_eq!(r.line_end, Some(50));
        assert_eq!(r.depth_level(), 2);
    }

    #[test]
    fn test_skip_invalid_references() {
        assert!(FileReference::parse("email@example.com").is_none());
        assert!(FileReference::parse("@http://example.com").is_none());
        assert!(FileReference::parse("@CLAUDE.md").is_none());
    }

    #[test]
    fn test_extract_references() {
        let content = "See @src/main.rs:42 and @src/lib.rs for details";
        let refs = extract_references(content);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].path, "src/main.rs");
        assert_eq!(refs[1].path, "src/lib.rs");
    }

    #[test]
    fn test_count_functions() {
        let content = "See @src/main.rs:42-50 and @src/lib.rs:10 and @README.md";
        assert_eq!(count_references(content), 3);
        assert_eq!(count_references_with_lines(content), 2);
        assert_eq!(count_references_with_ranges(content), 1);
    }
}
