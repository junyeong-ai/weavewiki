//! File Scanner - Public API for codebase scanning
//!
//! This module provides a gitignore-aware file scanner for external consumers
//! who need to scan codebases with the same filtering logic as claudegen.
//!
//! # Public API
//!
//! `FileScanner` is exported as part of the public API (`claudegen::FileScanner`)
//! for use by external tools and integrations. Internal claudegen components
//! use `WalkBuilder` directly for specific requirements.
//!
//! # Example
//!
//! ```ignore
//! use claudegen::FileScanner;
//! use claudegen::config::AnalysisConfig;
//!
//! let scanner = FileScanner::new("./my-project", &AnalysisConfig::default())
//!     .source_only();
//!
//! for file in scanner.scan()? {
//!     println!("{}", file.path.display());
//! }
//! ```

use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

use crate::config::AnalysisConfig;
use crate::types::Result;

/// Common source code extensions - language-agnostic, comprehensive list
/// Includes mainstream and emerging languages for broad coverage
const SOURCE_EXTENSIONS: &[&str] = &[
    // Systems languages
    "rs", "c", "cpp", "cc", "cxx", "h", "hpp", "hxx", "zig", "nim", "v",
    // JVM languages
    "java", "kt", "kts", "scala", "sc", "groovy", "clj", "cljs", "cljc",
    // .NET languages
    "cs", "fs", "fsx", "vb", // Web/JavaScript ecosystem
    "ts", "tsx", "js", "jsx", "mjs", "cjs", "vue", "svelte", "astro", // Python ecosystem
    "py", "pyi", "pyx", "pxd", // Ruby ecosystem
    "rb", "rake", "gemspec", "erb", // Go
    "go",  // Swift/Apple ecosystem
    "swift", "m", "mm", // Functional languages
    "hs", "lhs", "ml", "mli", "ex", "exs", "erl", "hrl", "elm", "purs", "rkt",
    // Scripting languages
    "php", "lua", "pl", "pm", "r", "jl", // Shell scripts
    "sh", "bash", "zsh", "fish", "ps1", "psm1", // Mobile/Cross-platform
    "dart", "cr", // Config as code (often contains logic)
    "nix", "dhall", // Query/Data languages
    "sql", "graphql", "gql", // Markup with logic
    "mdx",
];

/// Pre-compiled pattern for efficient matching
#[derive(Debug, Clone)]
struct CompiledPattern {
    /// Compiled glob pattern (None if invalid)
    glob: Option<glob::Pattern>,
    /// Directory prefix for "dir/**" patterns
    dir_prefix: Option<String>,
}

impl CompiledPattern {
    fn compile(pattern: &str) -> Self {
        let glob = match glob::Pattern::new(pattern) {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!(pattern = %pattern, error = %e, "Invalid glob pattern, will use fallback matching");
                None
            }
        };

        let dir_prefix = pattern.strip_suffix("/**").map(|s| s.to_string());

        Self { glob, dir_prefix }
    }

    fn matches(&self, relative_path: &str) -> bool {
        // Try glob match first
        if let Some(ref glob) = self.glob
            && glob.matches(relative_path)
        {
            return true;
        }

        // Fallback: directory prefix matching for "dir/**" patterns
        if let Some(ref prefix) = self.dir_prefix
            && (relative_path.starts_with(prefix)
                || relative_path.starts_with(&format!("{}/", prefix)))
        {
            return true;
        }

        false
    }
}

/// Compiled pattern set for include/exclude matching
#[derive(Debug, Clone, Default)]
struct CompiledPatternSet {
    patterns: Vec<CompiledPattern>,
    /// Fast path: all files match (e.g., ["**/*"])
    match_all: bool,
}

impl CompiledPatternSet {
    fn compile(patterns: &[String]) -> Self {
        // Fast path detection
        if patterns.len() == 1 && patterns[0] == "**/*" {
            return Self {
                patterns: Vec::new(),
                match_all: true,
            };
        }

        let compiled: Vec<CompiledPattern> = patterns
            .iter()
            .map(|p| CompiledPattern::compile(p))
            .collect();

        Self {
            patterns: compiled,
            match_all: false,
        }
    }

    fn matches(&self, relative_path: &str) -> bool {
        if self.match_all {
            return true;
        }

        self.patterns.iter().any(|p| p.matches(relative_path))
    }

    fn is_empty(&self) -> bool {
        !self.match_all && self.patterns.is_empty()
    }
}

/// Gitignore-aware file scanner for codebase analysis.
///
/// This is the **public API** for scanning files with:
/// - `.gitignore` / `.git/info/exclude` / global gitignore support
/// - Configurable include/exclude glob patterns
/// - File size and sample count limits
/// - Source-only filtering by extension
///
/// # Usage
///
/// For external consumers who need claudegen's file scanning logic.
/// Internal components use `WalkBuilder` directly for specific requirements.
pub struct FileScanner {
    root: PathBuf,
    /// Pre-compiled include patterns
    include: CompiledPatternSet,
    /// Pre-compiled exclude patterns
    exclude: CompiledPatternSet,
    max_file_size: u64,
    /// Maximum number of files to sample (0 = unlimited)
    max_file_samples: usize,
    source_only: bool,
}

impl FileScanner {
    /// Create a scanner with configuration
    pub fn new<P: AsRef<Path>>(root: P, config: &AnalysisConfig) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            include: CompiledPatternSet::compile(&config.include),
            exclude: CompiledPatternSet::compile(&config.exclude),
            max_file_size: config.max_file_size as u64,
            max_file_samples: config.max_file_samples,
            source_only: false,
        }
    }

    pub fn with_include(mut self, patterns: &[String]) -> Self {
        self.include = CompiledPatternSet::compile(patterns);
        self
    }

    pub fn with_exclude(mut self, patterns: &[String]) -> Self {
        self.exclude = CompiledPatternSet::compile(patterns);
        self
    }

    pub fn with_max_file_size(mut self, size: u64) -> Self {
        self.max_file_size = size;
        self
    }

    /// Enable source file extension filtering
    pub fn source_only(mut self) -> Self {
        self.source_only = true;
        self
    }

    fn build_walker(&self) -> ignore::Walk {
        WalkBuilder::new(&self.root)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .follow_links(false) // Security: prevent symlink traversal attacks
            .build()
    }

    fn should_process(&self, path: &Path) -> bool {
        path.is_file()
            && self.should_include(path)
            && !self.should_exclude(path)
            && self.check_source_extension(path)
    }

    /// Count files without collecting them (more efficient for scale detection)
    ///
    /// Respects `max_file_samples` limit for consistency with `scan()`.
    pub fn count(&self) -> usize {
        let iter = self.build_walker().filter_map(|e| e.ok()).filter(|entry| {
            let path = entry.path();
            self.should_process(path) && self.check_size(path)
        });

        if self.max_file_samples > 0 {
            iter.take(self.max_file_samples).count()
        } else {
            iter.count()
        }
    }

    /// Get relative paths as strings
    pub fn paths(&self) -> Result<Vec<String>> {
        let files = self.scan()?;
        Ok(files
            .into_iter()
            .filter_map(|f| {
                f.path
                    .strip_prefix(&self.root)
                    .ok()
                    .map(|p| p.to_string_lossy().to_string())
            })
            .collect())
    }

    pub fn scan(&self) -> Result<Vec<ScannedFile>> {
        let mut files = Vec::new();

        for entry in self.build_walker().filter_map(|e| e.ok()) {
            // Check sample limit (0 = unlimited)
            if self.max_file_samples > 0 && files.len() >= self.max_file_samples {
                tracing::info!(
                    limit = self.max_file_samples,
                    "Reached max_file_samples limit, stopping scan"
                );
                break;
            }

            let path = entry.path();

            if !self.should_process(path) {
                continue;
            }

            match path.metadata() {
                Ok(metadata) if metadata.len() <= self.max_file_size => {
                    files.push(ScannedFile {
                        path: path.to_path_buf(),
                        size: metadata.len(),
                        extension: path.extension().and_then(|e| e.to_str()).map(String::from),
                    });
                }
                Ok(_) => {} // File too large, skip silently
                Err(e) => {
                    tracing::debug!(path = %path.display(), error = %e, "Failed to read file metadata");
                }
            }
        }

        Ok(files)
    }

    fn get_relative_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string()
    }

    fn should_include(&self, path: &Path) -> bool {
        let relative = self.get_relative_path(path);
        self.include.matches(&relative)
    }

    fn should_exclude(&self, path: &Path) -> bool {
        if self.exclude.is_empty() {
            return false;
        }
        let relative = self.get_relative_path(path);
        self.exclude.matches(&relative)
    }

    fn check_size(&self, path: &Path) -> bool {
        path.metadata()
            .map(|m| m.len() <= self.max_file_size)
            .unwrap_or(false)
    }

    fn check_source_extension(&self, path: &Path) -> bool {
        if !self.source_only {
            return true;
        }
        path.extension()
            .and_then(|e| e.to_str())
            .map(|ext| SOURCE_EXTENSIONS.contains(&ext))
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub size: u64,
    pub extension: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compiled_pattern_glob() {
        let pattern = CompiledPattern::compile("*.rs");
        assert!(pattern.glob.is_some());
        assert!(pattern.matches("main.rs"));
        assert!(!pattern.matches("main.ts"));
    }

    #[test]
    fn test_compiled_pattern_directory() {
        let pattern = CompiledPattern::compile("node_modules/**");
        assert!(pattern.dir_prefix.is_some());
        assert!(pattern.matches("node_modules/package/index.js"));
        assert!(!pattern.matches("src/index.js"));
    }

    #[test]
    fn test_compiled_pattern_invalid() {
        // Invalid glob pattern should not panic, just log warning
        let pattern = CompiledPattern::compile("[invalid");
        assert!(pattern.glob.is_none());
        // Should not match anything via glob
        assert!(!pattern.matches("test.rs"));
    }

    #[test]
    fn test_pattern_set_match_all() {
        let set = CompiledPatternSet::compile(&["**/*".to_string()]);
        assert!(set.match_all);
        assert!(set.matches("any/path/file.rs"));
    }

    #[test]
    fn test_pattern_set_specific() {
        let set = CompiledPatternSet::compile(&["src/**/*.rs".to_string()]);
        assert!(!set.match_all);
        assert!(set.matches("src/lib.rs"));
        assert!(set.matches("src/utils/helpers.rs"));
    }
}
