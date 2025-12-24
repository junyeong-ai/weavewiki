use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

use crate::types::Result;

/// Default maximum file size for analysis (1MB)
const DEFAULT_MAX_FILE_SIZE: usize = 1_048_576;

/// Common source code extensions
const SOURCE_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "rb", "c", "cpp", "h", "hpp", "cs",
    "swift", "scala", "php", "lua", "sh", "bash", "zsh",
];

/// Default directories to skip
const DEFAULT_SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    "build",
    "dist",
    "__pycache__",
    "vendor",
    ".venv",
];

pub struct FileScanner {
    root: PathBuf,
    include: Vec<String>,
    exclude: Vec<String>,
    max_file_size: u64,
    source_only: bool,
}

impl FileScanner {
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            include: vec!["**/*".to_string()],
            exclude: vec![],
            max_file_size: DEFAULT_MAX_FILE_SIZE as u64,
            source_only: false,
        }
    }

    /// Create a scanner for source files with default skip patterns
    pub fn source_files<P: AsRef<Path>>(root: P) -> Self {
        let exclude = DEFAULT_SKIP_DIRS
            .iter()
            .map(|d| format!("{}/**", d))
            .collect();
        Self {
            root: root.as_ref().to_path_buf(),
            include: vec!["**/*".to_string()],
            exclude,
            max_file_size: DEFAULT_MAX_FILE_SIZE as u64,
            source_only: true,
        }
    }

    pub fn with_include(mut self, patterns: Vec<String>) -> Self {
        self.include = patterns;
        self
    }

    pub fn with_exclude(mut self, patterns: Vec<String>) -> Self {
        self.exclude = patterns;
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

    /// Count files without collecting them (more efficient for scale detection)
    pub fn count(&self) -> usize {
        let walker = WalkBuilder::new(&self.root)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .follow_links(false) // Security: prevent symlink traversal attacks
            .build();

        walker
            .filter_map(|e| e.ok())
            .filter(|entry| {
                let path = entry.path();
                path.is_file()
                    && !self.should_exclude(path)
                    && self.check_size(path)
                    && self.check_source_extension(path)
            })
            .count()
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

        let walker = WalkBuilder::new(&self.root)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .follow_links(false) // Security: prevent symlink traversal attacks
            .build();

        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            if self.should_exclude(path) {
                continue;
            }

            if !self.check_source_extension(path) {
                continue;
            }

            if let Ok(metadata) = path.metadata() {
                if metadata.len() > self.max_file_size {
                    continue;
                }

                files.push(ScannedFile {
                    path: path.to_path_buf(),
                    size: metadata.len(),
                    extension: path.extension().and_then(|e| e.to_str()).map(String::from),
                });
            }
        }

        Ok(files)
    }

    fn should_exclude(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();

        for pattern in &self.exclude {
            if glob::Pattern::new(pattern)
                .map(|p| p.matches(&path_str))
                .unwrap_or(false)
            {
                return true;
            }
        }

        false
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
