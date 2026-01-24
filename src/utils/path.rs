//! Path Security Utilities
//!
//! Centralized path validation to prevent path traversal attacks.

use std::path::{Component, Path, PathBuf};

/// Result of path resolution
#[derive(Debug)]
pub enum PathResolution {
    /// Path is safe and resolved
    Safe(PathBuf),
    /// Path contains traversal attempt (..)
    TraversalAttempt,
    /// Path is absolute (starts with / or has prefix)
    AbsolutePath,
    /// Path escapes root after canonicalization
    EscapesRoot,
}

impl PathResolution {
    /// Convert to Option, returning Some only for safe paths
    pub fn ok(self) -> Option<PathBuf> {
        match self {
            Self::Safe(path) => Some(path),
            _ => None,
        }
    }

    /// Check if resolution is safe
    pub fn is_safe(&self) -> bool {
        matches!(self, Self::Safe(_))
    }
}

/// Safely resolve a relative path within a root directory.
///
/// Prevents path traversal attacks by:
/// 1. Rejecting paths with `..` components
/// 2. Rejecting absolute paths
/// 3. Verifying canonicalized path stays within root
///
/// Returns `PathResolution` indicating the result.
pub fn safe_resolve(root: &Path, relative: &str) -> PathResolution {
    let path = Path::new(relative);

    for component in path.components() {
        match component {
            Component::ParentDir => {
                tracing::warn!(path = %relative, "Path traversal attempt detected");
                return PathResolution::TraversalAttempt;
            }
            Component::RootDir | Component::Prefix(_) => {
                tracing::warn!(path = %relative, "Absolute path rejected");
                return PathResolution::AbsolutePath;
            }
            _ => {}
        }
    }

    let full_path = root.join(relative);

    // Verify resolved path stays within root (handles symlinks)
    match full_path.canonicalize() {
        Ok(canonical) => {
            if let Ok(root_canonical) = root.canonicalize() {
                if canonical.starts_with(&root_canonical) {
                    return PathResolution::Safe(full_path);
                }
                tracing::warn!(
                    path = %relative,
                    resolved = %canonical.display(),
                    "Path escapes root after canonicalization"
                );
                return PathResolution::EscapesRoot;
            }
            // Root can't be canonicalized, allow the join result
            PathResolution::Safe(full_path)
        }
        // File doesn't exist yet, but path structure is safe
        Err(_) => PathResolution::Safe(full_path),
    }
}

/// Convenience function returning Option<PathBuf>
#[inline]
pub fn safe_join(root: &Path, relative: &str) -> Option<PathBuf> {
    safe_resolve(root, relative).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_safe_path() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("file.rs"), "").unwrap();

        let result = safe_resolve(temp.path(), "file.rs");
        assert!(result.is_safe());
    }

    #[test]
    fn test_traversal_rejected() {
        let temp = TempDir::new().unwrap();

        let result = safe_resolve(temp.path(), "../etc/passwd");
        assert!(matches!(result, PathResolution::TraversalAttempt));
    }

    #[test]
    fn test_absolute_rejected() {
        let temp = TempDir::new().unwrap();

        let result = safe_resolve(temp.path(), "/etc/passwd");
        assert!(matches!(result, PathResolution::AbsolutePath));
    }

    #[test]
    fn test_nested_path() {
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src/utils")).unwrap();
        std::fs::write(temp.path().join("src/utils/mod.rs"), "").unwrap();

        let result = safe_resolve(temp.path(), "src/utils/mod.rs");
        assert!(result.is_safe());
    }

    #[test]
    fn test_nonexistent_safe() {
        let temp = TempDir::new().unwrap();

        // Non-existent but safe path structure
        let result = safe_resolve(temp.path(), "future/file.rs");
        assert!(result.is_safe());
    }
}
