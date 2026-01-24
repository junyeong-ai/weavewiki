//! Project Detection Phase
//!
//! Automatically detects project type by analyzing file structure and dependencies.
//! Supports: CLI, Library, Backend, Frontend, Monorepo, Agent, Hybrid

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::config::ProjectType;
use crate::types::Result;

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
}

impl ProjectDetector {
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
        }
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

    async fn count_files_by_extension(
        &self,
        dir: &Path,
        extensions: &HashMap<&str, &str>,
        counts: &mut HashMap<String, (usize, Option<String>)>,
    ) -> Result<()> {
        let skip_dirs = [
            "target",
            "node_modules",
            "dist",
            "build",
            ".git",
            "vendor",
            "__pycache__",
            ".venv",
        ];

        if let Ok(mut entries) = fs::read_dir(dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();

                if skip_dirs.contains(&file_name) {
                    continue;
                }

                if path.is_dir() {
                    Box::pin(self.count_files_by_extension(&path, extensions, counts)).await?;
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str())
                    && let Some(lang) = extensions.get(ext)
                {
                    counts.entry((*lang).to_string()).or_insert((0, None)).0 += 1;
                }
            }
        }
        Ok(())
    }

    async fn collect_signals(&self) -> Result<Vec<DetectionSignal>> {
        let mut signals = Vec::new();

        // CLI signals
        if self.project_root.join("Cargo.toml").exists() {
            if let Ok(content) = fs::read_to_string(self.project_root.join("Cargo.toml")).await
                && (content.contains("clap")
                    || content.contains("structopt")
                    || content.contains("argh"))
            {
                signals.push(DetectionSignal {
                    signal_type: SignalType::Dependency,
                    source: "Cargo.toml".to_string(),
                    suggests: ProjectType::Cli,
                    weight: 0.8,
                });
            }
            if self.project_root.join("src/main.rs").exists() {
                signals.push(DetectionSignal {
                    signal_type: SignalType::EntryPoint,
                    source: "src/main.rs".to_string(),
                    suggests: ProjectType::Cli,
                    weight: 0.3,
                });
            }
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

        // Backend signals
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

        // Backend framework detection - Rust
        if let Ok(content) = fs::read_to_string(self.project_root.join("Cargo.toml")).await {
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

        // Backend framework detection - JVM (Kotlin/Java)
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

        // Hexagonal/Clean Architecture patterns (strong backend signal)
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

        // Backend framework detection - Python
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

        // Backend framework detection - Node.js
        if let Ok(content) = fs::read_to_string(self.project_root.join("package.json")).await {
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
        }

        // Backend framework detection - Go
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

        // Frontend signals (React, Vue, etc.)
        if self.project_root.join("package.json").exists()
            && let Ok(content) = fs::read_to_string(self.project_root.join("package.json")).await
        {
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
            return Ok(Some(self.parse_pnpm_workspace().await.unwrap_or_else(
                |_| self.empty_workspace(WorkspaceType::PnpmWorkspace),
            )));
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
            return Ok(Some(self.empty_workspace(ws_type)));
        }

        // Turbo repo
        if self.project_root.join("turbo.json").exists() {
            return Ok(Some(self.empty_workspace(WorkspaceType::TurboRepo)));
        }

        // Nx workspace
        if self.project_root.join("nx.json").exists() {
            return Ok(Some(self.empty_workspace(WorkspaceType::NxWorkspace)));
        }

        // Lerna
        if self.project_root.join("lerna.json").exists() {
            return Ok(Some(self.empty_workspace(WorkspaceType::LernaMonorepo)));
        }

        // Gradle multi-project
        if self.project_root.join("settings.gradle").exists()
            || self.project_root.join("settings.gradle.kts").exists()
        {
            return Ok(Some(
                self.empty_workspace(WorkspaceType::GradleMultiProject),
            ));
        }

        // Maven multi-module
        if let Ok(content) = fs::read_to_string(self.project_root.join("pom.xml")).await
            && content.contains("<modules>")
        {
            return Ok(Some(self.empty_workspace(WorkspaceType::MavenMultiModule)));
        }

        // Go workspace
        if self.project_root.join("go.work").exists() {
            return Ok(Some(self.empty_workspace(WorkspaceType::GoWorkspace)));
        }

        Ok(None)
    }

    fn empty_workspace(&self, workspace_type: WorkspaceType) -> WorkspaceConfig {
        WorkspaceConfig {
            workspace_type,
            members: Vec::new(),
            shared_packages: Vec::new(),
        }
    }

    async fn parse_cargo_workspace(&self, content: &str) -> Result<WorkspaceConfig> {
        let mut members = Vec::new();
        let mut in_members = false;

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("members") && line.contains('[') {
                in_members = true;
                continue;
            }
            if in_members {
                if line.contains(']') {
                    break;
                }
                let member = line.trim_matches(|c| c == '"' || c == '\'' || c == ',');
                if !member.is_empty() && !member.starts_with('#') {
                    let member_type = self.infer_member_type(member).await;
                    members.push(WorkspaceMember {
                        path: member.to_string(),
                        name: member.split('/').next_back().map(String::from),
                        project_type: member_type,
                        language: Some("rust".to_string()),
                    });
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
                let package = line
                    .trim_start_matches('-')
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'');
                if !package.is_empty() {
                    let (member_type, lang) = self.infer_js_member_type(package).await;
                    members.push(WorkspaceMember {
                        path: package.to_string(),
                        name: package.split('/').next_back().map(String::from),
                        project_type: member_type,
                        language: Some(lang),
                    });
                }
            }
        }

        Ok(WorkspaceConfig {
            workspace_type: WorkspaceType::PnpmWorkspace,
            members,
            shared_packages: Vec::new(),
        })
    }

    async fn infer_member_type(&self, member_path: &str) -> ProjectType {
        let full_path = self.project_root.join(member_path);

        if full_path.join("src/main.rs").exists() {
            if let Ok(cargo) = fs::read_to_string(full_path.join("Cargo.toml")).await {
                if cargo.contains("clap") || cargo.contains("structopt") {
                    return ProjectType::Cli;
                }
                if cargo.contains("actix") || cargo.contains("axum") || cargo.contains("rocket") {
                    return ProjectType::Backend;
                }
            }
            return ProjectType::Cli;
        }

        if full_path.join("src/lib.rs").exists() {
            return ProjectType::Library;
        }

        ProjectType::Library
    }

    async fn infer_js_member_type(&self, member_path: &str) -> (ProjectType, String) {
        let clean_path = member_path.trim_end_matches("/*").trim_end_matches("/**");
        let full_path = self.project_root.join(clean_path);

        if let Ok(pkg) = fs::read_to_string(full_path.join("package.json")).await {
            let is_ts = pkg.contains("typescript") || full_path.join("tsconfig.json").exists();
            let lang = if is_ts { "typescript" } else { "javascript" }.to_string();

            if pkg.contains("react") || pkg.contains("vue") || pkg.contains("next") {
                return (ProjectType::Frontend, lang);
            }
            if pkg.contains("express") || pkg.contains("fastify") || pkg.contains("koa") {
                return (ProjectType::Backend, lang);
            }

            return (ProjectType::Library, lang);
        }

        (ProjectType::Library, "typescript".to_string())
    }

    fn compute_type(&self, detection: &ProjectDetection) -> (ProjectType, f32) {
        if detection.is_monorepo {
            return (ProjectType::Monorepo, 0.95);
        }

        let mut type_scores: HashMap<ProjectType, f32> = HashMap::new();

        for signal in &detection.signals {
            *type_scores.entry(signal.suggests).or_default() += signal.weight;
        }

        // Language hints
        if let Some(primary_lang) = detection.languages.first() {
            match primary_lang.language.as_str() {
                "rust" => {
                    if detection
                        .signals
                        .iter()
                        .any(|s| s.source.contains("lib.rs") && s.suggests == ProjectType::Library)
                    {
                        *type_scores.entry(ProjectType::Library).or_default() += 0.3;
                    }
                }
                "typescript" | "javascript" => {
                    if detection
                        .signals
                        .iter()
                        .any(|s| s.suggests == ProjectType::Frontend)
                    {
                        *type_scores.entry(ProjectType::Frontend).or_default() += 0.2;
                    }
                }
                _ => {}
            }
        }

        let (best_type, best_score) = type_scores
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((ProjectType::Library, 0.5));

        let confidence = (best_score / 2.0).min(1.0);

        // Hybrid detection: multiple strong signals
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

pub async fn run(project_root: impl AsRef<Path>) -> Result<ProjectDetection> {
    let detector = ProjectDetector::new(project_root);
    detector.detect().await
}

pub async fn detect(project_root: impl AsRef<Path>) -> Result<ProjectDetection> {
    run(project_root).await
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
}
