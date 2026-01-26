//! Project Detection Phase
//!
//! Automatically detects project type by analyzing file structure and dependencies.
//! Supports: CLI, Library, Backend, Frontend, Monorepo, Agent, Hybrid

use std::collections::HashMap;
use std::path::Path;

use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::config::{AnalysisConfig, ProjectType};
use crate::types::Result;

/// Detected member type with language information
#[derive(Debug, Clone)]
struct MemberTypeInfo {
    project_type: ProjectType,
    language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDetection {
    pub primary_type: ProjectType,
    pub confidence: f32,
    pub signals: Vec<DetectionSignal>,
    pub languages: Vec<LanguageInfo>,
    pub is_monorepo: bool,
    pub workspace_config: Option<WorkspaceConfig>,
}

impl Default for ProjectDetection {
    fn default() -> Self {
        Self {
            primary_type: ProjectType::Auto,
            confidence: 0.0,
            signals: Vec::new(),
            languages: Vec::new(),
            is_monorepo: false,
            workspace_config: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionSignal {
    pub signal_type: SignalType,
    pub source: String,
    pub suggests: ProjectType,
    pub weight: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SignalType {
    ManifestFile,
    DirectoryStructure,
    EntryPoint,
    Dependency,
    FrameworkMarker,
    ToolConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageInfo {
    pub language: String,
    pub file_count: usize,
    pub percentage: f32,
    pub primary_manifest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub workspace_type: WorkspaceType,
    pub members: Vec<WorkspaceMember>,
    pub shared_packages: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkspaceType {
    CargoWorkspace,
    PnpmWorkspace,
    NpmWorkspace,
    YarnWorkspace,
    TurboRepo,
    NxWorkspace,
    LernaMonorepo,
    GradleMultiProject,
    MavenMultiModule,
    GoWorkspace,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMember {
    pub path: String,
    pub name: Option<String>,
    pub project_type: ProjectType,
    pub language: Option<String>,
}

pub struct ProjectDetector {
    project_root: std::path::PathBuf,
    exclude_patterns: Vec<glob::Pattern>,
    include_patterns: Vec<glob::Pattern>,
}

impl ProjectDetector {
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        Self::with_config(project_root, &AnalysisConfig::default())
    }

    pub fn with_config(project_root: impl AsRef<Path>, config: &AnalysisConfig) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            exclude_patterns: config
                .exclude
                .iter()
                .filter_map(|p| glob::Pattern::new(p).ok())
                .collect(),
            include_patterns: config
                .include
                .iter()
                .filter_map(|p| glob::Pattern::new(p).ok())
                .collect(),
        }
    }

    /// Check if a directory contains a package manifest file
    fn has_package_manifest(dir: &Path) -> bool {
        dir.join("package.json").exists()
            || dir.join("Cargo.toml").exists()
            || dir.join("go.mod").exists()
            || dir.join("pom.xml").exists()
            || dir.join("build.gradle").exists()
            || dir.join("build.gradle.kts").exists()
    }

    pub async fn detect(&self) -> Result<ProjectDetection> {
        let mut detection = ProjectDetection::default();

        let languages = self.detect_languages().await?;
        detection.languages = languages;

        let signals = self.collect_signals().await?;
        detection.signals = signals;

        let workspace = self.detect_workspace().await?;
        if let Some(ws) = workspace {
            detection.is_monorepo = true;
            detection.workspace_config = Some(ws);
        }

        let (primary_type, confidence) = self.compute_type(&detection);
        detection.primary_type = primary_type;
        detection.confidence = confidence;

        tracing::info!(
            project_type = %detection.primary_type,
            confidence = detection.confidence,
            is_monorepo = detection.is_monorepo,
            languages = ?detection.languages.iter().map(|l| &l.language).collect::<Vec<_>>(),
            "Project detection complete"
        );

        Ok(detection)
    }

    async fn detect_languages(&self) -> Result<Vec<LanguageInfo>> {
        let mut counts: HashMap<String, (usize, Option<String>)> = HashMap::new();

        let manifest_languages = [
            ("Cargo.toml", "rust"),
            ("package.json", "typescript"),
            ("pyproject.toml", "python"),
            ("setup.py", "python"),
            ("go.mod", "go"),
            ("build.gradle", "kotlin"),
            ("build.gradle.kts", "kotlin"),
            ("pom.xml", "java"),
            ("Gemfile", "ruby"),
            ("composer.json", "php"),
            ("Package.swift", "swift"),
        ];

        for (manifest, lang) in manifest_languages {
            if self.project_root.join(manifest).exists() {
                let entry = counts.entry(lang.to_string()).or_insert((0, None));
                entry.1 = Some(manifest.to_string());
            }
        }

        let extensions: HashMap<&str, &str> = [
            ("rs", "rust"),
            ("ts", "typescript"),
            ("tsx", "typescript"),
            ("js", "javascript"),
            ("jsx", "javascript"),
            ("py", "python"),
            ("go", "go"),
            ("kt", "kotlin"),
            ("java", "java"),
            ("rb", "ruby"),
            ("php", "php"),
            ("swift", "swift"),
            ("c", "c"),
            ("cpp", "cpp"),
            ("h", "c"),
            ("hpp", "cpp"),
        ]
        .into_iter()
        .collect();

        self.count_files_by_extension(&self.project_root, &extensions, &mut counts)
            .await?;

        let total: usize = counts.values().map(|(c, _)| c).sum();
        let total = total.max(1);

        let mut languages: Vec<LanguageInfo> = counts
            .into_iter()
            .filter(|(_, (count, _))| *count > 0)
            .map(|(lang, (count, manifest))| LanguageInfo {
                language: lang,
                file_count: count,
                percentage: (count as f32 / total as f32) * 100.0,
                primary_manifest: manifest,
            })
            .collect();

        languages.sort_by(|a, b| b.file_count.cmp(&a.file_count));
        Ok(languages)
    }

    /// Count files by extension using gitignore-aware walker
    ///
    /// Uses ignore crate's WalkBuilder for consistent gitignore handling with FileScanner.
    async fn count_files_by_extension(
        &self,
        _dir: &Path,
        extensions: &HashMap<&str, &str>,
        counts: &mut HashMap<String, (usize, Option<String>)>,
    ) -> Result<()> {
        let walker = WalkBuilder::new(&self.project_root)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .follow_links(false)
            .build();

        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path();

            // Skip directories
            if path.is_dir() {
                continue;
            }

            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();

            let relative_path = path
                .strip_prefix(&self.project_root)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            // Skip if matches exclude pattern
            if self
                .exclude_patterns
                .iter()
                .any(|p| p.matches(&relative_path) || p.matches(file_name))
            {
                continue;
            }

            // Skip hidden files unless explicitly included
            if file_name.starts_with('.')
                && !self
                    .include_patterns
                    .iter()
                    .any(|p| p.matches(&relative_path))
            {
                continue;
            }

            // Skip plugin output directories
            if relative_path.contains("-plugin/") || relative_path.ends_with("-plugin") {
                continue;
            }

            if let Some(ext) = path.extension().and_then(|e| e.to_str())
                && let Some(lang) = extensions.get(ext)
            {
                counts.entry((*lang).to_string()).or_insert((0, None)).0 += 1;
            }
        }
        Ok(())
    }

    async fn collect_signals(&self) -> Result<Vec<DetectionSignal>> {
        let mut signals = Vec::new();

        // Read manifest files once (avoid duplicate reads)
        let cargo_content = fs::read_to_string(self.project_root.join("Cargo.toml"))
            .await
            .ok();
        let package_json_content = fs::read_to_string(self.project_root.join("package.json"))
            .await
            .ok();

        // Rust signals (CLI + Backend)
        if let Some(ref content) = cargo_content {
            // CLI detection
            if content.contains("clap") || content.contains("structopt") || content.contains("argh")
            {
                signals.push(DetectionSignal {
                    signal_type: SignalType::Dependency,
                    source: "Cargo.toml".to_string(),
                    suggests: ProjectType::Cli,
                    weight: 0.8,
                });
            }

            // Backend framework detection
            let backend_deps = ["actix-web", "axum", "rocket", "warp", "tide"];
            for dep in backend_deps {
                if content.contains(dep) {
                    signals.push(DetectionSignal {
                        signal_type: SignalType::FrameworkMarker,
                        source: format!("{dep} dependency"),
                        suggests: ProjectType::Backend,
                        weight: 0.9,
                    });
                }
            }
        }

        // Entry point signals
        if self.project_root.join("src/main.rs").exists() {
            signals.push(DetectionSignal {
                signal_type: SignalType::EntryPoint,
                source: "src/main.rs".to_string(),
                suggests: ProjectType::Cli,
                weight: 0.3,
            });
        }

        // Library signals
        if self.project_root.join("src/lib.rs").exists()
            && !self.project_root.join("src/main.rs").exists()
        {
            signals.push(DetectionSignal {
                signal_type: SignalType::EntryPoint,
                source: "src/lib.rs (no main.rs)".to_string(),
                suggests: ProjectType::Library,
                weight: 0.9,
            });
        }

        // Backend directory markers
        let backend_markers = [
            ("src/routes", "routes directory"),
            ("src/api", "api directory"),
            ("src/handlers", "handlers directory"),
            ("src/controllers", "controllers directory"),
            ("src/services", "services directory"),
        ];
        for (path, desc) in backend_markers {
            if self.project_root.join(path).exists() {
                signals.push(DetectionSignal {
                    signal_type: SignalType::DirectoryStructure,
                    source: desc.to_string(),
                    suggests: ProjectType::Backend,
                    weight: 0.6,
                });
            }
        }

        // JVM backend detection (Kotlin/Java)
        for gradle_file in ["build.gradle.kts", "build.gradle"] {
            if let Ok(content) = fs::read_to_string(self.project_root.join(gradle_file)).await {
                let jvm_backend_deps = [
                    "spring-boot",
                    "spring-boot-starter",
                    "ktor",
                    "micronaut",
                    "quarkus",
                    "webflux",
                ];
                for dep in jvm_backend_deps {
                    if content.contains(dep) {
                        signals.push(DetectionSignal {
                            signal_type: SignalType::FrameworkMarker,
                            source: format!("{dep} dependency"),
                            suggests: ProjectType::Backend,
                            weight: 0.9,
                        });
                    }
                }
            }
        }

        // Hexagonal/Clean Architecture patterns
        let hexagonal_dirs = [
            ("adapter", 0.7),
            ("port", 0.7),
            ("domain", 0.5),
            ("src/main/kotlin", 0.4),
            ("src/main/java", 0.4),
        ];
        for (dir, weight) in hexagonal_dirs {
            if self.project_root.join(dir).exists()
                || self.project_root.join(format!("src/{dir}")).exists()
            {
                signals.push(DetectionSignal {
                    signal_type: SignalType::DirectoryStructure,
                    source: format!("{dir}/ directory (architecture pattern)"),
                    suggests: ProjectType::Backend,
                    weight,
                });
            }
        }

        // Python backend detection
        if let Ok(content) = fs::read_to_string(self.project_root.join("pyproject.toml")).await {
            let python_backend_deps = ["fastapi", "django", "flask", "starlette"];
            for dep in python_backend_deps {
                if content.contains(dep) {
                    signals.push(DetectionSignal {
                        signal_type: SignalType::FrameworkMarker,
                        source: format!("{dep} dependency"),
                        suggests: ProjectType::Backend,
                        weight: 0.9,
                    });
                }
            }
        }

        // Node.js signals (Backend + Frontend) - single read
        if let Some(ref content) = package_json_content {
            // Backend detection
            let node_backend_deps = ["express", "fastify", "koa", "hono", "nestjs"];
            for dep in node_backend_deps {
                if content.contains(&format!("\"{dep}\"")) {
                    signals.push(DetectionSignal {
                        signal_type: SignalType::FrameworkMarker,
                        source: format!("{dep} dependency"),
                        suggests: ProjectType::Backend,
                        weight: 0.9,
                    });
                }
            }

            // Frontend detection
            let frontend_deps = ["react", "vue", "angular", "svelte", "next", "nuxt"];
            for dep in frontend_deps {
                if content.contains(&format!("\"{dep}\"")) {
                    signals.push(DetectionSignal {
                        signal_type: SignalType::FrameworkMarker,
                        source: format!("{dep} dependency"),
                        suggests: ProjectType::Frontend,
                        weight: 0.9,
                    });
                }
            }
        }

        // Go signals
        if let Ok(content) = fs::read_to_string(self.project_root.join("go.mod")).await {
            let go_backend_deps = [
                "gin-gonic/gin",
                "labstack/echo",
                "gofiber/fiber",
                "go-chi/chi",
                "gorilla/mux",
                "gorm.io/gorm",
            ];
            for dep in go_backend_deps {
                if content.contains(dep) {
                    signals.push(DetectionSignal {
                        signal_type: SignalType::FrameworkMarker,
                        source: format!("{} dependency", dep.split('/').next_back().unwrap_or(dep)),
                        suggests: ProjectType::Backend,
                        weight: 0.9,
                    });
                }
            }
        }

        // Go CLI detection
        if self.project_root.join("go.mod").exists() && self.project_root.join("cmd").exists() {
            signals.push(DetectionSignal {
                signal_type: SignalType::DirectoryStructure,
                source: "cmd/ directory (Go CLI pattern)".to_string(),
                suggests: ProjectType::Cli,
                weight: 0.6,
            });
        }

        // Frontend directory markers
        let frontend_dirs = ["src/components", "src/pages", "src/views", "app/components"];
        for dir in frontend_dirs {
            if self.project_root.join(dir).exists() {
                signals.push(DetectionSignal {
                    signal_type: SignalType::DirectoryStructure,
                    source: dir.to_string(),
                    suggests: ProjectType::Frontend,
                    weight: 0.7,
                });
            }
        }

        // Agent signals
        let agent_markers = [
            "mcp.json",
            "tools.json",
            ".mcp/",
            "src/tools/",
            "src/agents/",
        ];
        for marker in agent_markers {
            if self.project_root.join(marker).exists() {
                signals.push(DetectionSignal {
                    signal_type: SignalType::ToolConfig,
                    source: marker.to_string(),
                    suggests: ProjectType::Agent,
                    weight: 0.8,
                });
            }
        }

        Ok(signals)
    }

    async fn detect_workspace(&self) -> Result<Option<WorkspaceConfig>> {
        // Cargo workspace
        if let Ok(content) = fs::read_to_string(self.project_root.join("Cargo.toml")).await
            && content.contains("[workspace]")
        {
            return Ok(Some(self.parse_cargo_workspace(&content).await?));
        }

        // pnpm workspace
        if self.project_root.join("pnpm-workspace.yaml").exists() {
            return Ok(Some(self.parse_pnpm_workspace().await?));
        }

        // npm/yarn workspace (package.json with workspaces)
        if let Ok(content) = fs::read_to_string(self.project_root.join("package.json")).await
            && content.contains("\"workspaces\"")
        {
            let ws_type = if self.project_root.join("yarn.lock").exists() {
                WorkspaceType::YarnWorkspace
            } else {
                WorkspaceType::NpmWorkspace
            };
            return Ok(Some(
                self.parse_npm_yarn_workspace(&content, ws_type).await?,
            ));
        }

        // Turbo repo (uses package.json workspaces underneath)
        if self.project_root.join("turbo.json").exists()
            && let Ok(content) = fs::read_to_string(self.project_root.join("package.json")).await
        {
            return Ok(Some(
                self.parse_npm_yarn_workspace(&content, WorkspaceType::TurboRepo)
                    .await?,
            ));
        }

        // Nx workspace
        if self.project_root.join("nx.json").exists() {
            return Ok(Some(self.parse_nx_workspace().await?));
        }

        // Lerna
        if self.project_root.join("lerna.json").exists() {
            return Ok(Some(self.parse_lerna_workspace().await?));
        }

        // Gradle multi-project
        if self.project_root.join("settings.gradle").exists()
            || self.project_root.join("settings.gradle.kts").exists()
        {
            return Ok(Some(self.parse_gradle_workspace().await?));
        }

        // Maven multi-module
        if let Ok(content) = fs::read_to_string(self.project_root.join("pom.xml")).await
            && content.contains("<modules>")
        {
            return Ok(Some(self.parse_maven_workspace(&content).await?));
        }

        // Go workspace
        if self.project_root.join("go.work").exists() {
            return Ok(Some(self.parse_go_workspace().await?));
        }

        Ok(None)
    }

    async fn parse_cargo_workspace(&self, content: &str) -> Result<WorkspaceConfig> {
        let mut members = Vec::new();
        let mut in_members = false;

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("members") && line.contains('[') {
                // Check for single-line format: members = ["a", "b"]
                if let (Some(arr_start), Some(arr_end)) = (line.find('['), line.find(']')) {
                    // Single-line format
                    let array_content = &line[arr_start + 1..arr_end];
                    for item in array_content.split(',') {
                        let member = item.trim().trim_matches(|c| c == '"' || c == '\'');
                        if !member.is_empty() && !member.starts_with('#') {
                            let info = self.infer_rust_member(member).await;
                            members.push(Self::create_member(member, info));
                        }
                    }
                    break;
                }
                // Multi-line format
                in_members = true;
                continue;
            }
            if in_members {
                if line.contains(']') {
                    break;
                }
                let member = line.trim_matches(|c| c == '"' || c == '\'' || c == ',');
                if !member.is_empty() && !member.starts_with('#') {
                    let info = self.infer_rust_member(member).await;
                    members.push(Self::create_member(member, info));
                }
            }
        }

        Ok(WorkspaceConfig {
            workspace_type: WorkspaceType::CargoWorkspace,
            members,
            shared_packages: Vec::new(),
        })
    }

    async fn parse_pnpm_workspace(&self) -> Result<WorkspaceConfig> {
        let content = fs::read_to_string(self.project_root.join("pnpm-workspace.yaml")).await?;
        let mut members = Vec::new();
        let mut in_packages = false;

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("packages:") {
                in_packages = true;
                continue;
            }
            if in_packages && line.starts_with('-') {
                let pattern = line
                    .trim_start_matches('-')
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'');
                if !pattern.is_empty() {
                    // Expand glob patterns like "packages/*"
                    let expanded = self.expand_glob_pattern(pattern).await;
                    for path in expanded {
                        let info = self.infer_js_member(&path).await;
                        members.push(Self::create_member(&path, info));
                    }
                }
            }
        }

        Ok(WorkspaceConfig {
            workspace_type: WorkspaceType::PnpmWorkspace,
            members,
            shared_packages: Vec::new(),
        })
    }

    async fn parse_npm_yarn_workspace(
        &self,
        content: &str,
        ws_type: WorkspaceType,
    ) -> Result<WorkspaceConfig> {
        let mut members = Vec::new();

        // Parse JSON workspaces - supports both array and object format
        // Array format: "workspaces": ["packages/*", "apps/*"]
        // Object format: "workspaces": { "packages": ["packages/*", "apps/*"], "nohoist": [...] }
        if let Some(start) = content.find("\"workspaces\"") {
            let after_key = &content[start + "\"workspaces\"".len()..];
            // Skip whitespace and colon
            let after_colon = after_key.trim_start().trim_start_matches(':').trim_start();

            let array_content = if after_colon.starts_with('[') {
                // Direct array format
                after_colon
                    .find(']')
                    .map(|arr_end| &after_colon[1..arr_end])
            } else if after_colon.starts_with('{') {
                // Object format - look for "packages" array inside
                if let Some(pkg_start) = after_colon.find("\"packages\"")
                    && let pkg_after = &after_colon[pkg_start + "\"packages\"".len()..]
                    && let trimmed = pkg_after.trim_start().trim_start_matches(':').trim_start()
                    && trimmed.starts_with('[')
                {
                    trimmed.find(']').map(|arr_end| &trimmed[1..arr_end])
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(arr_content) = array_content {
                for item in arr_content.split(',') {
                    let package = item
                        .trim()
                        .trim_matches(|c| c == '"' || c == '\'' || c == ' ');
                    if !package.is_empty() {
                        let expanded = self.expand_glob_pattern(package).await;
                        for path in expanded {
                            let info = self.infer_js_member(&path).await;
                            members.push(Self::create_member(&path, info));
                        }
                    }
                }
            }
        }

        Ok(WorkspaceConfig {
            workspace_type: ws_type,
            members,
            shared_packages: Vec::new(),
        })
    }

    async fn parse_gradle_workspace(&self) -> Result<WorkspaceConfig> {
        let mut members = Vec::new();

        // Try Kotlin DSL first, then Groovy DSL
        let settings_file = if self.project_root.join("settings.gradle.kts").exists() {
            self.project_root.join("settings.gradle.kts")
        } else {
            self.project_root.join("settings.gradle")
        };

        let content = fs::read_to_string(&settings_file).await?;

        // Parse include statements
        // Groovy: include ':app', ':core:data', ':feature:home'
        // Kotlin: include(":app", ":core:data")
        for line in content.lines() {
            let line = line.trim();
            if !line.starts_with("include") {
                continue;
            }

            // Extract project paths from include statement
            let projects = Self::extract_gradle_includes(line);
            for project in projects {
                // Convert Gradle project path to directory path
                // :app -> app, :core:data -> core/data
                let dir_path = project.trim_start_matches(':').replace(':', "/");
                if !dir_path.is_empty() {
                    let info = self.infer_jvm_member(&dir_path).await;
                    members.push(Self::create_member(&dir_path, info));
                }
            }
        }

        Ok(WorkspaceConfig {
            workspace_type: WorkspaceType::GradleMultiProject,
            members,
            shared_packages: Vec::new(),
        })
    }

    fn extract_gradle_includes(line: &str) -> Vec<String> {
        let mut projects = Vec::new();

        // Remove 'include' keyword and parentheses
        let content = line
            .trim_start_matches("include")
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .trim();

        // Split by comma and extract quoted strings
        for part in content.split(',') {
            let project = part
                .trim()
                .trim_matches(|c| c == '"' || c == '\'' || c == ' ');
            if !project.is_empty() {
                projects.push(project.to_string());
            }
        }

        projects
    }

    async fn parse_maven_workspace(&self, content: &str) -> Result<WorkspaceConfig> {
        const MODULE_TAG: &str = "<module>";
        const MODULE_END_TAG: &str = "</module>";
        let mut members = Vec::new();

        // Parse <modules><module>name</module></modules> section
        if let Some(start) = content.find("<modules>")
            && let Some(end) = content[start..].find("</modules>")
        {
            let modules_section = &content[start..start + end];

            // Extract each <module>...</module>
            let mut pos = 0;
            while let Some(mod_start) = modules_section[pos..].find(MODULE_TAG) {
                let actual_start = pos + mod_start + MODULE_TAG.len();
                if let Some(mod_end) = modules_section[actual_start..].find(MODULE_END_TAG) {
                    let module_name = modules_section[actual_start..actual_start + mod_end].trim();
                    if !module_name.is_empty() {
                        let info = self.infer_jvm_member(module_name).await;
                        members.push(Self::create_member(module_name, info));
                    }
                    pos = actual_start + mod_end + MODULE_END_TAG.len();
                } else {
                    break;
                }
            }
        }

        Ok(WorkspaceConfig {
            workspace_type: WorkspaceType::MavenMultiModule,
            members,
            shared_packages: Vec::new(),
        })
    }

    async fn parse_lerna_workspace(&self) -> Result<WorkspaceConfig> {
        let content = fs::read_to_string(self.project_root.join("lerna.json")).await?;
        let mut members = Vec::new();

        // Parse "packages": ["packages/*", ...]
        if let Some(start) = content.find("\"packages\"")
            && let after_key = &content[start..]
            && let Some(arr_start) = after_key.find('[')
            && let Some(arr_end) = after_key[arr_start..].find(']')
        {
            let arr_content = &after_key[arr_start + 1..arr_start + arr_end];
            for item in arr_content.split(',') {
                let pattern = item
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'' || c == ' ');
                if !pattern.is_empty() {
                    let expanded = self.expand_glob_pattern(pattern).await;
                    for path in expanded {
                        let info = self.infer_js_member(&path).await;
                        members.push(Self::create_member(&path, info));
                    }
                }
            }
        }

        Ok(WorkspaceConfig {
            workspace_type: WorkspaceType::LernaMonorepo,
            members,
            shared_packages: Vec::new(),
        })
    }

    async fn parse_go_workspace(&self) -> Result<WorkspaceConfig> {
        let content = fs::read_to_string(self.project_root.join("go.work")).await?;
        let mut members = Vec::new();
        let mut in_use_block = false;

        for line in content.lines() {
            let line = line.trim();

            // Single use statement: use ./path
            if line.starts_with("use ") && !line.contains('(') {
                let path = line
                    .trim_start_matches("use ")
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .trim_start_matches("./");
                if !path.is_empty() {
                    let info = self.infer_go_member(path).await;
                    members.push(Self::create_member(path, info));
                }
                continue;
            }

            // Block use statement: use ( ... )
            if line == "use (" {
                in_use_block = true;
                continue;
            }

            if line == ")" && in_use_block {
                in_use_block = false;
                continue;
            }

            if in_use_block {
                let path = line
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .trim_start_matches("./");
                if !path.is_empty() && !path.starts_with("//") {
                    let info = self.infer_go_member(path).await;
                    members.push(Self::create_member(path, info));
                }
            }
        }

        Ok(WorkspaceConfig {
            workspace_type: WorkspaceType::GoWorkspace,
            members,
            shared_packages: Vec::new(),
        })
    }

    async fn parse_nx_workspace(&self) -> Result<WorkspaceConfig> {
        let mut members = Vec::new();

        // Nx projects are typically in apps/ and libs/ directories
        // Also check workspace.json or project.json files
        let project_dirs = ["apps", "libs", "packages"];

        for dir in project_dirs {
            let dir_path = self.project_root.join(dir);
            if !dir_path.exists() {
                continue;
            }

            if let Ok(mut entries) = fs::read_dir(&dir_path).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if path.is_dir() {
                        // Check if it's an Nx project (has project.json or package.json)
                        if path.join("project.json").exists() || path.join("package.json").exists()
                        {
                            let relative_path = format!(
                                "{}/{}",
                                dir,
                                path.file_name().unwrap_or_default().to_string_lossy()
                            );
                            let info = self.infer_js_member(&relative_path).await;
                            members.push(Self::create_member(&relative_path, info));
                        }
                    }
                }
            }
        }

        Ok(WorkspaceConfig {
            workspace_type: WorkspaceType::NxWorkspace,
            members,
            shared_packages: Vec::new(),
        })
    }

    async fn expand_glob_pattern(&self, pattern: &str) -> Vec<String> {
        let mut results = Vec::new();

        // Check if this is a glob pattern or a literal path
        let is_glob = pattern.contains('*');

        if !is_glob {
            // Literal path - check if it exists and is a package
            let full_path = self.project_root.join(pattern);
            if full_path.is_dir() && Self::has_package_manifest(&full_path) {
                results.push(pattern.to_string());
            }
            return results;
        }

        // Handle simple glob patterns like "packages/*" or "apps/**"
        let base_path = pattern
            .trim_end_matches("/*")
            .trim_end_matches("/**")
            .trim_end_matches("*");

        let full_base = self.project_root.join(base_path);
        if !full_base.exists() {
            return results;
        }

        if let Ok(mut entries) = fs::read_dir(&full_base).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_dir() && Self::has_package_manifest(&path) {
                    let relative = format!(
                        "{}/{}",
                        base_path.trim_end_matches('/'),
                        path.file_name().unwrap_or_default().to_string_lossy()
                    );
                    results.push(relative);
                }
            }
        }

        results
    }

    /// Create a WorkspaceMember from path and type info
    fn create_member(path: &str, info: MemberTypeInfo) -> WorkspaceMember {
        WorkspaceMember {
            path: path.to_string(),
            name: path.split('/').next_back().map(String::from),
            project_type: info.project_type,
            language: Some(info.language),
        }
    }

    /// Unified JVM member inference (Gradle/Maven) - single pass
    async fn infer_jvm_member(&self, member_path: &str) -> MemberTypeInfo {
        let full_path = self.project_root.join(member_path);
        let mut project_type = ProjectType::Library;
        let mut language = "java".to_string();

        // Check source directories first (most reliable for language)
        if full_path.join("src/main/kotlin").exists() {
            language = "kotlin".to_string();
        } else if full_path.join("src/main/java").exists() {
            language = "java".to_string();
        }

        // Check build files for project type and language hints
        for gradle_file in ["build.gradle.kts", "build.gradle"] {
            if let Ok(content) = fs::read_to_string(full_path.join(gradle_file)).await {
                // Language detection from plugins
                if content.contains("kotlin(") || content.contains("org.jetbrains.kotlin") {
                    language = "kotlin".to_string();
                }

                // Project type detection
                if content.contains("com.android.application") {
                    project_type = ProjectType::Frontend;
                    break;
                }
                if content.contains("com.android.library") {
                    project_type = ProjectType::Library;
                    break;
                }
                if content.contains("spring-boot") || content.contains("org.springframework") {
                    project_type = ProjectType::Backend;
                    break;
                }
                if content.contains("ktor") {
                    project_type = ProjectType::Backend;
                    break;
                }
            }
        }

        // Check Maven pom.xml
        if let Ok(content) = fs::read_to_string(full_path.join("pom.xml")).await
            && (content.contains("spring-boot-starter-web")
                || content.contains("spring-webmvc")
                || content.contains("jakarta.ws.rs"))
        {
            project_type = ProjectType::Backend;
        }

        // Fallback: infer from directory name
        if project_type == ProjectType::Library {
            let name = member_path.split('/').next_back().unwrap_or(member_path);
            if name == "app" || name.contains("android") || name.contains("mobile") {
                project_type = ProjectType::Frontend;
            } else if name.contains("api") || name.contains("server") || name.contains("backend") {
                project_type = ProjectType::Backend;
            }
        }

        MemberTypeInfo {
            project_type,
            language,
        }
    }

    /// Infer Go member type based on directory structure
    async fn infer_go_member(&self, member_path: &str) -> MemberTypeInfo {
        let full_path = self.project_root.join(member_path);
        let language = "go".to_string();

        // Check for cmd directory (CLI pattern)
        if full_path.join("cmd").exists() {
            return MemberTypeInfo {
                project_type: ProjectType::Cli,
                language,
            };
        }

        // Check for main.go
        if full_path.join("main.go").exists() {
            if let Ok(content) = fs::read_to_string(full_path.join("main.go")).await {
                // HTTP server patterns
                if content.contains("http.ListenAndServe")
                    || content.contains("gin.")
                    || content.contains("echo.")
                    || content.contains("fiber.")
                {
                    return MemberTypeInfo {
                        project_type: ProjectType::Backend,
                        language,
                    };
                }
            }
            return MemberTypeInfo {
                project_type: ProjectType::Cli,
                language,
            };
        }

        // Internal or pkg directories are libraries
        MemberTypeInfo {
            project_type: ProjectType::Library,
            language,
        }
    }

    /// Infer Rust member type from Cargo.toml
    async fn infer_rust_member(&self, member_path: &str) -> MemberTypeInfo {
        let full_path = self.project_root.join(member_path);
        let language = "rust".to_string();

        if full_path.join("src/main.rs").exists() {
            if let Ok(cargo) = fs::read_to_string(full_path.join("Cargo.toml")).await {
                if cargo.contains("clap") || cargo.contains("structopt") {
                    return MemberTypeInfo {
                        project_type: ProjectType::Cli,
                        language,
                    };
                }
                if cargo.contains("actix") || cargo.contains("axum") || cargo.contains("rocket") {
                    return MemberTypeInfo {
                        project_type: ProjectType::Backend,
                        language,
                    };
                }
            }
            return MemberTypeInfo {
                project_type: ProjectType::Cli,
                language,
            };
        }

        if full_path.join("src/lib.rs").exists() {
            return MemberTypeInfo {
                project_type: ProjectType::Library,
                language,
            };
        }

        MemberTypeInfo {
            project_type: ProjectType::Library,
            language,
        }
    }

    /// Infer JS/TS member type from package.json
    async fn infer_js_member(&self, member_path: &str) -> MemberTypeInfo {
        let clean_path = member_path.trim_end_matches("/*").trim_end_matches("/**");
        let full_path = self.project_root.join(clean_path);

        if let Ok(pkg) = fs::read_to_string(full_path.join("package.json")).await {
            let is_ts = pkg.contains("typescript") || full_path.join("tsconfig.json").exists();
            let language = if is_ts { "typescript" } else { "javascript" }.to_string();

            if pkg.contains("react") || pkg.contains("vue") || pkg.contains("next") {
                return MemberTypeInfo {
                    project_type: ProjectType::Frontend,
                    language,
                };
            }
            if pkg.contains("express") || pkg.contains("fastify") || pkg.contains("koa") {
                return MemberTypeInfo {
                    project_type: ProjectType::Backend,
                    language,
                };
            }

            return MemberTypeInfo {
                project_type: ProjectType::Library,
                language,
            };
        }

        MemberTypeInfo {
            project_type: ProjectType::Library,
            language: "typescript".to_string(),
        }
    }

    fn compute_type(&self, detection: &ProjectDetection) -> (ProjectType, f32) {
        if detection.is_monorepo {
            return (ProjectType::Monorepo, 0.95);
        }

        let mut type_scores: HashMap<ProjectType, f32> = HashMap::new();

        for signal in &detection.signals {
            *type_scores.entry(signal.suggests).or_default() += signal.weight;
        }

        // REMOVED: Language-specific bonuses
        //
        // Previously, Rust projects with lib.rs got +0.3 toward Library, and
        // TypeScript/JavaScript got +0.2 toward Frontend. These were removed because:
        //
        // 1. ARBITRARY WEIGHTS: 0.3 vs 0.2 had no theoretical basis
        // 2. ASYMMETRIC: Python, Go, Java got no bonus (unfair treatment)
        // 3. MISLEADING: TypeScript can be backend (Node.js), Rust can be CLI
        // 4. REDUNDANT: File-based signals already capture these patterns
        //
        // Signals from actual file analysis (Cargo.toml, package.json, etc.) are
        // sufficient and more accurate. Language-based assumptions are unnecessary.
        //
        // If a language-based hint is needed, LLM can apply domain knowledge
        // during analysis, where it has full context about the project.

        // FALLBACK: If no signals detected, defaults to Library with 0.5 confidence.
        // This may be incorrect for CLI tools or other project types without clear markers.
        // LLM should verify project type from actual code purpose, not rely on this default.
        let (best_type, best_score) = type_scores
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((ProjectType::Library, 0.5));

        let confidence = (best_score / 2.0).min(1.0);

        // HYBRID DETECTION - ARBITRARY THRESHOLD
        //
        // Uses weight >= 0.7 to identify "strong signals" for hybrid detection.
        // This threshold is arbitrary:
        // - Project with 0.8 backend + 0.65 frontend will NOT be detected as hybrid
        // - Project with 0.71 backend + 0.71 frontend WILL be detected as hybrid
        // - The 0.7 cutoff has no theoretical basis
        //
        // LLM should validate multi-purpose projects independently.
        let strong_signals: Vec<_> = detection
            .signals
            .iter()
            .filter(|s| s.weight >= 0.7)
            .collect();
        let unique_types: std::collections::HashSet<_> =
            strong_signals.iter().map(|s| s.suggests).collect();

        if unique_types.len() > 1 {
            return (ProjectType::Hybrid, confidence * 0.8);
        }

        (best_type, confidence)
    }
}

pub async fn detect(
    project_root: impl AsRef<Path>,
    config: &AnalysisConfig,
) -> Result<ProjectDetection> {
    let detector = ProjectDetector::with_config(project_root, config);
    detector.detect().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_types() {
        let signal = DetectionSignal {
            signal_type: SignalType::ManifestFile,
            source: "Cargo.toml".to_string(),
            suggests: ProjectType::Cli,
            weight: 0.8,
        };
        assert_eq!(signal.signal_type, SignalType::ManifestFile);
    }

    #[test]
    fn test_workspace_types() {
        assert_eq!(WorkspaceType::CargoWorkspace, WorkspaceType::CargoWorkspace);
        assert_ne!(WorkspaceType::CargoWorkspace, WorkspaceType::PnpmWorkspace);
    }

    #[test]
    fn test_extract_gradle_includes_kotlin_dsl() {
        // Single include - Kotlin DSL
        let line = r#"include("services:core-lib")"#;
        let projects = ProjectDetector::extract_gradle_includes(line);
        assert_eq!(projects, vec!["services:core-lib"]);

        // Multiple includes - Kotlin DSL
        let line = r#"include(":app", ":core:data", ":feature:home")"#;
        let projects = ProjectDetector::extract_gradle_includes(line);
        assert_eq!(projects, vec![":app", ":core:data", ":feature:home"]);
    }

    #[test]
    fn test_extract_gradle_includes_groovy_dsl() {
        // Groovy DSL with single quotes
        let line = "include ':app', ':core:data', ':feature:home'";
        let projects = ProjectDetector::extract_gradle_includes(line);
        assert_eq!(projects, vec![":app", ":core:data", ":feature:home"]);

        // Groovy DSL with double quotes
        let line = r#"include ":app", ":core""#;
        let projects = ProjectDetector::extract_gradle_includes(line);
        assert_eq!(projects, vec![":app", ":core"]);
    }

    #[test]
    fn test_gradle_path_conversion() {
        // Verify that Gradle project paths are properly converted to directory paths
        // :app -> app
        // :core:data -> core/data
        // services:core-lib -> services/core-lib
        let project = "services:core-lib";
        let dir_path = project.trim_start_matches(':').replace(':', "/");
        assert_eq!(dir_path, "services/core-lib");

        let project = ":core:data";
        let dir_path = project.trim_start_matches(':').replace(':', "/");
        assert_eq!(dir_path, "core/data");
    }

    #[tokio::test]
    async fn test_gradle_workspace_detection() {
        use std::fs;
        use tempfile::tempdir;

        let temp = tempdir().unwrap();
        let root = temp.path();

        // Create settings.gradle.kts
        fs::write(
            root.join("settings.gradle.kts"),
            r#"rootProject.name = "test"
include("app")
include("core:domain")
include("feature:home")
"#,
        )
        .unwrap();

        // Create subproject directories with build files
        for path in ["app", "core/domain", "feature/home"] {
            let dir = root.join(path);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("build.gradle.kts"), "").unwrap();
            fs::create_dir_all(dir.join("src/main/kotlin")).unwrap();
        }

        let detector = ProjectDetector::new(root);
        let detection = detector.detect().await.unwrap();

        assert!(detection.is_monorepo);
        assert!(detection.workspace_config.is_some());

        let ws = detection.workspace_config.unwrap();
        assert_eq!(ws.workspace_type, WorkspaceType::GradleMultiProject);
        assert_eq!(ws.members.len(), 3);

        let paths: Vec<_> = ws.members.iter().map(|m| m.path.as_str()).collect();
        assert!(paths.contains(&"app"));
        assert!(paths.contains(&"core/domain"));
        assert!(paths.contains(&"feature/home"));
    }

    #[tokio::test]
    async fn test_maven_workspace_detection() {
        use std::fs;
        use tempfile::tempdir;

        let temp = tempdir().unwrap();
        let root = temp.path();

        // Create pom.xml with modules
        fs::write(
            root.join("pom.xml"),
            r#"<?xml version="1.0"?>
<project>
    <modules>
        <module>core</module>
        <module>api</module>
        <module>web</module>
    </modules>
</project>
"#,
        )
        .unwrap();

        // Create module directories with pom.xml
        for module in ["core", "api", "web"] {
            let dir = root.join(module);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("pom.xml"), "<project></project>").unwrap();
            fs::create_dir_all(dir.join("src/main/java")).unwrap();
        }

        let detector = ProjectDetector::new(root);
        let detection = detector.detect().await.unwrap();

        assert!(detection.is_monorepo);
        assert!(detection.workspace_config.is_some());

        let ws = detection.workspace_config.unwrap();
        assert_eq!(ws.workspace_type, WorkspaceType::MavenMultiModule);
        assert_eq!(ws.members.len(), 3);
    }

    #[tokio::test]
    async fn test_go_workspace_detection() {
        use std::fs;
        use tempfile::tempdir;

        let temp = tempdir().unwrap();
        let root = temp.path();

        // Create go.work
        fs::write(
            root.join("go.work"),
            r#"go 1.21

use (
    ./cmd/server
    ./pkg/core
    ./internal/utils
)
"#,
        )
        .unwrap();

        // Create module directories with go.mod
        for path in ["cmd/server", "pkg/core", "internal/utils"] {
            let dir = root.join(path);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("go.mod"), "module example.com/test").unwrap();
        }

        let detector = ProjectDetector::new(root);
        let detection = detector.detect().await.unwrap();

        assert!(detection.is_monorepo);
        assert!(detection.workspace_config.is_some());

        let ws = detection.workspace_config.unwrap();
        assert_eq!(ws.workspace_type, WorkspaceType::GoWorkspace);
        assert_eq!(ws.members.len(), 3);
    }

    #[tokio::test]
    async fn test_cargo_workspace_single_line_format() {
        use std::fs;
        use tempfile::tempdir;

        let temp = tempdir().unwrap();
        let root = temp.path();

        // Create Cargo.toml with single-line members format
        fs::write(
            root.join("Cargo.toml"),
            r#"[workspace]
members = ["crates/core", "crates/cli", "crates/utils"]
"#,
        )
        .unwrap();

        // Create member directories with Cargo.toml
        for path in ["crates/core", "crates/cli", "crates/utils"] {
            let dir = root.join(path);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("Cargo.toml"),
                format!("[package]\nname = \"{}\"\n", path.replace('/', "-")),
            )
            .unwrap();
            fs::create_dir_all(dir.join("src")).unwrap();
        }

        let detector = ProjectDetector::new(root);
        let detection = detector.detect().await.unwrap();

        assert!(detection.is_monorepo);
        assert!(detection.workspace_config.is_some());

        let ws = detection.workspace_config.unwrap();
        assert_eq!(ws.workspace_type, WorkspaceType::CargoWorkspace);
        assert_eq!(ws.members.len(), 3);

        let paths: Vec<_> = ws.members.iter().map(|m| m.path.as_str()).collect();
        assert!(paths.contains(&"crates/core"));
        assert!(paths.contains(&"crates/cli"));
        assert!(paths.contains(&"crates/utils"));
    }

    #[tokio::test]
    async fn test_npm_workspace_object_format() {
        use std::fs;
        use tempfile::tempdir;

        let temp = tempdir().unwrap();
        let root = temp.path();

        // Create package.json with object workspaces format
        fs::write(
            root.join("package.json"),
            r#"{
  "name": "monorepo",
  "private": true,
  "workspaces": {
    "packages": ["packages/core", "packages/utils"],
    "nohoist": ["**/react-native"]
  }
}"#,
        )
        .unwrap();

        // Create member directories with package.json
        for path in ["packages/core", "packages/utils"] {
            let dir = root.join(path);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("package.json"),
                format!(r#"{{"name": "{}"}}"#, path.replace('/', "-")),
            )
            .unwrap();
        }

        let detector = ProjectDetector::new(root);
        let detection = detector.detect().await.unwrap();

        assert!(detection.is_monorepo);
        assert!(detection.workspace_config.is_some());

        let ws = detection.workspace_config.unwrap();
        assert!(
            ws.workspace_type == WorkspaceType::NpmWorkspace
                || ws.workspace_type == WorkspaceType::YarnWorkspace
        );
        assert_eq!(ws.members.len(), 2);

        let paths: Vec<_> = ws.members.iter().map(|m| m.path.as_str()).collect();
        assert!(paths.contains(&"packages/core"));
        assert!(paths.contains(&"packages/utils"));
    }
}
