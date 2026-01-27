//! Monorepo Structure Analyzer
//!
//! Analyzes monorepo structure to identify subprojects, shared packages,
//! and cross-project dependencies for appropriate output strategy.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::config::ProjectType;
use crate::types::Result;

use super::OutputStrategy;
use super::project_detection::{ProjectDetection, WorkspaceConfig, WorkspaceType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonorepoAnalysis {
    pub is_monorepo: bool,
    pub workspace_type: Option<WorkspaceType>,
    pub subprojects: Vec<SubprojectInfo>,
    pub shared_packages: Vec<SharedPackage>,
    pub cross_dependencies: Vec<CrossDependency>,
    pub output_strategy: OutputStrategy,
    pub rules_grouping: Vec<RulesGroup>,
}

impl Default for MonorepoAnalysis {
    fn default() -> Self {
        Self {
            is_monorepo: false,
            workspace_type: None,
            subprojects: Vec::new(),
            shared_packages: Vec::new(),
            cross_dependencies: Vec::new(),
            output_strategy: OutputStrategy::Unified,
            rules_grouping: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubprojectInfo {
    pub path: String,
    pub name: String,
    pub project_type: ProjectType,
    pub language: String,
    pub is_app: bool,
    pub dependencies: Vec<String>,
    pub entry_points: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedPackage {
    pub path: String,
    pub name: String,
    pub consumers: Vec<String>,
    pub is_internal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossDependency {
    pub source: String,
    pub target: String,
    pub dependency_type: CrossDepType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CrossDepType {
    Internal,
    Shared,
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulesGroup {
    pub name: String,
    pub paths: Vec<String>,
    pub project_types: Vec<ProjectType>,
    pub languages: Vec<String>,
}

pub struct MonorepoAnalyzer {
    project_root: PathBuf,
}

impl MonorepoAnalyzer {
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
        }
    }

    pub async fn analyze(&self, detection: &ProjectDetection) -> Result<MonorepoAnalysis> {
        if !detection.is_monorepo {
            return Ok(MonorepoAnalysis::default());
        }

        let workspace = detection
            .workspace_config
            .as_ref()
            .ok_or_else(|| crate::types::ClaudegenError::Config("No workspace config".into()))?;

        let subprojects = self.analyze_subprojects(workspace).await?;
        let shared_packages = self.find_shared_packages(&subprojects).await?;
        let cross_dependencies = self.find_cross_dependencies(&subprojects).await?;
        let output_strategy = self.determine_output_strategy(&subprojects);
        let rules_grouping = self.create_rules_grouping(&subprojects);

        let analysis = MonorepoAnalysis {
            is_monorepo: true,
            workspace_type: Some(workspace.workspace_type),
            subprojects,
            shared_packages,
            cross_dependencies,
            output_strategy,
            rules_grouping,
        };

        tracing::info!(
            workspace_type = ?analysis.workspace_type,
            subprojects = analysis.subprojects.len(),
            shared_packages = analysis.shared_packages.len(),
            output_strategy = ?analysis.output_strategy,
            "Monorepo analysis complete"
        );

        Ok(analysis)
    }

    async fn analyze_subprojects(
        &self,
        workspace: &WorkspaceConfig,
    ) -> Result<Vec<SubprojectInfo>> {
        let mut subprojects = Vec::new();

        for member in &workspace.members {
            let path = self.resolve_glob_path(&member.path).await;
            for resolved_path in path {
                if let Some(info) = self
                    .analyze_single_subproject(&resolved_path, workspace)
                    .await?
                {
                    subprojects.push(info);
                }
            }
        }

        Ok(subprojects)
    }

    async fn resolve_glob_path(&self, pattern: &str) -> Vec<String> {
        let clean_pattern = pattern.trim_end_matches("/*").trim_end_matches("/**");
        let full_path = self.project_root.join(clean_pattern);

        if full_path.exists() && full_path.is_dir() {
            if pattern.contains('*')
                && let Ok(mut entries) = fs::read_dir(&full_path).await
            {
                let mut paths = Vec::new();
                while let Ok(Some(entry)) = entries.next_entry().await {
                    if entry.path().is_dir() {
                        let relative = entry
                            .path()
                            .strip_prefix(&self.project_root)
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if !relative.is_empty() {
                            paths.push(relative);
                        }
                    }
                }
                return paths;
            }
            return vec![clean_pattern.to_string()];
        }

        Vec::new()
    }

    async fn analyze_single_subproject(
        &self,
        path: &str,
        workspace: &WorkspaceConfig,
    ) -> Result<Option<SubprojectInfo>> {
        let full_path = self.project_root.join(path);
        if !full_path.exists() || !full_path.is_dir() {
            return Ok(None);
        }

        let name = path.split('/').next_back().unwrap_or(path).to_string();
        let (project_type, language) = self.detect_subproject_type(&full_path, workspace).await;
        let is_app = self.is_application(&full_path, &project_type).await;
        // Local dependency detection removed - use proper manifest parsing or LLM analysis
        let dependencies = Vec::new();
        let entry_points = self.find_entry_points(&full_path, &language).await;

        Ok(Some(SubprojectInfo {
            path: path.to_string(),
            name,
            project_type,
            language,
            is_app,
            dependencies,
            entry_points,
        }))
    }

    async fn detect_subproject_type(
        &self,
        path: &Path,
        workspace: &WorkspaceConfig,
    ) -> (ProjectType, String) {
        // First check what manifest files exist in the subproject itself
        // This handles mixed-language monorepos (e.g., pnpm for frontend + gradle for backend)
        if path.join("Cargo.toml").exists() {
            return self.detect_rust_subproject_type(path).await;
        }
        if path.join("build.gradle.kts").exists() || path.join("build.gradle").exists() {
            return self.detect_jvm_subproject_type(path).await;
        }
        if path.join("pom.xml").exists() {
            return self.detect_jvm_subproject_type(path).await;
        }
        if path.join("go.mod").exists() {
            return (ProjectType::Library, "go".to_string());
        }
        if path.join("package.json").exists() {
            return self.detect_js_subproject_type(path).await;
        }
        if path.join("pyproject.toml").exists() || path.join("setup.py").exists() {
            return (ProjectType::Library, "python".to_string());
        }

        // Fall back to workspace-type-based detection
        match workspace.workspace_type {
            WorkspaceType::CargoWorkspace => self.detect_rust_subproject_type(path).await,
            WorkspaceType::PnpmWorkspace
            | WorkspaceType::NpmWorkspace
            | WorkspaceType::YarnWorkspace
            | WorkspaceType::TurboRepo
            | WorkspaceType::NxWorkspace
            | WorkspaceType::LernaMonorepo => self.detect_js_subproject_type(path).await,
            WorkspaceType::GradleMultiProject | WorkspaceType::MavenMultiModule => {
                self.detect_jvm_subproject_type(path).await
            }
            WorkspaceType::GoWorkspace => (ProjectType::Library, "go".to_string()),
            WorkspaceType::Unknown => (ProjectType::Library, "unknown".to_string()),
        }
    }

    async fn detect_rust_subproject_type(&self, path: &Path) -> (ProjectType, String) {
        if path.join("src/main.rs").exists() && !path.join("src/lib.rs").exists() {
            return (ProjectType::Cli, "rust".to_string());
        }
        (ProjectType::Library, "rust".to_string())
    }

    async fn detect_js_subproject_type(&self, path: &Path) -> (ProjectType, String) {
        let has_ts = path.join("tsconfig.json").exists();
        let lang = if has_ts { "typescript" } else { "javascript" }.to_string();
        (ProjectType::Library, lang)
    }

    async fn detect_jvm_subproject_type(&self, path: &Path) -> (ProjectType, String) {
        let lang = if path.join("src/main/kotlin").exists() {
            "kotlin"
        } else {
            "java"
        }
        .to_string();

        if path.join("src/main").exists() {
            let main_exists =
                path.join("src/main/kotlin").exists() || path.join("src/main/java").exists();
            if main_exists {
                return (ProjectType::Backend, lang);
            }
        }

        (ProjectType::Library, lang)
    }

    async fn is_application(&self, path: &Path, project_type: &ProjectType) -> bool {
        match project_type {
            ProjectType::Cli | ProjectType::Backend | ProjectType::Frontend => true,
            ProjectType::Library => {
                path.join("src/main.rs").exists()
                    || path.join("src/main.ts").exists()
                    || path.join("src/index.ts").exists()
            }
            _ => false,
        }
    }

    async fn find_entry_points(&self, path: &Path, language: &str) -> Vec<String> {
        let mut entries = Vec::new();

        let candidates: Vec<&str> = match language {
            "rust" => vec!["src/main.rs", "src/lib.rs"],
            "typescript" | "javascript" => vec![
                "src/index.ts",
                "src/main.ts",
                "src/index.js",
                "src/main.js",
                "index.ts",
                "index.js",
            ],
            "kotlin" | "java" => vec!["src/main/kotlin/Main.kt", "src/main/java/Main.java"],
            "go" => vec!["main.go", "cmd/main.go"],
            _ => vec![],
        };

        for candidate in candidates {
            if path.join(candidate).exists() {
                entries.push(candidate.to_string());
            }
        }

        entries
    }

    async fn find_shared_packages(
        &self,
        subprojects: &[SubprojectInfo],
    ) -> Result<Vec<SharedPackage>> {
        let mut shared = Vec::new();

        for sp in subprojects {
            if !sp.is_app && sp.project_type == ProjectType::Library {
                let consumers: Vec<String> = subprojects
                    .iter()
                    .filter(|other| other.dependencies.contains(&sp.name))
                    .map(|other| other.name.clone())
                    .collect();

                if !consumers.is_empty() {
                    shared.push(SharedPackage {
                        path: sp.path.clone(),
                        name: sp.name.clone(),
                        consumers,
                        is_internal: true,
                    });
                }
            }
        }

        Ok(shared)
    }

    async fn find_cross_dependencies(
        &self,
        subprojects: &[SubprojectInfo],
    ) -> Result<Vec<CrossDependency>> {
        let mut deps = Vec::new();

        for sp in subprojects {
            for dep in &sp.dependencies {
                let dep_type = if subprojects.iter().any(|s| &s.name == dep && s.is_app) {
                    CrossDepType::Internal
                } else if subprojects.iter().any(|s| &s.name == dep && !s.is_app) {
                    CrossDepType::Shared
                } else {
                    CrossDepType::External
                };

                deps.push(CrossDependency {
                    source: sp.name.clone(),
                    target: dep.clone(),
                    dependency_type: dep_type,
                });
            }
        }

        Ok(deps)
    }

    fn determine_output_strategy(&self, subprojects: &[SubprojectInfo]) -> OutputStrategy {
        if subprojects.is_empty() {
            return OutputStrategy::Unified;
        }

        let languages: std::collections::HashSet<_> =
            subprojects.iter().map(|s| s.language.as_str()).collect();
        let types: std::collections::HashSet<_> =
            subprojects.iter().map(|s| s.project_type).collect();

        if languages.len() > 1 {
            return OutputStrategy::SplitByLanguage;
        }

        if types.len() > 1 && subprojects.len() > 3 {
            return OutputStrategy::SplitByProject;
        }

        if subprojects.len() > 5 {
            return OutputStrategy::Hierarchical;
        }

        OutputStrategy::SplitByProject
    }

    fn create_rules_grouping(&self, subprojects: &[SubprojectInfo]) -> Vec<RulesGroup> {
        let mut groups: HashMap<(ProjectType, String), Vec<&SubprojectInfo>> = HashMap::new();

        for sp in subprojects {
            groups
                .entry((sp.project_type, sp.language.clone()))
                .or_default()
                .push(sp);
        }

        groups
            .into_iter()
            .map(|((proj_type, lang), sps)| {
                let name = format!("{}-{}", proj_type.as_str(), lang);
                let paths: Vec<String> = sps.iter().map(|sp| format!("{}/**", sp.path)).collect();

                RulesGroup {
                    name,
                    paths,
                    project_types: vec![proj_type],
                    languages: vec![lang],
                }
            })
            .collect()
    }
}

pub async fn run(
    project_root: impl AsRef<Path>,
    detection: &ProjectDetection,
) -> Result<MonorepoAnalysis> {
    let analyzer = MonorepoAnalyzer::new(project_root);
    analyzer.analyze(detection).await
}

pub async fn analyze(
    project_root: impl AsRef<Path>,
    detection: &ProjectDetection,
) -> Result<MonorepoAnalysis> {
    run(project_root, detection).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_strategy() {
        assert!(!OutputStrategy::Unified.is_split());
        assert!(OutputStrategy::SplitByProject.is_split());
        assert!(OutputStrategy::SplitByLanguage.is_split());
    }

    #[test]
    fn test_cross_dep_types() {
        assert_eq!(CrossDepType::Internal, CrossDepType::Internal);
        assert_ne!(CrossDepType::Internal, CrossDepType::External);
    }
}
