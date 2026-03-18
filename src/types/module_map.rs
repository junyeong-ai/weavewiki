pub use modmap::{
    ArchitectureLayer, Convention, DependencyEdge, DependencyGraph, DependencyType,
    DetectedLanguage, EvidenceLocation, FrameworkInfo, GeneratorInfo, IssueCategory,
    IssueSeverity, KnownIssue, LibraryInfo, Module, ModuleDependency, ModuleGroup, ModuleMap,
    ModuleMetrics, ProjectCommands, ProjectMetadata, ProjectType, TechStack, WorkspaceInfo,
    WorkspaceType, SCHEMA_VERSION,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedModule {
    pub module_id: String,
    pub paths: Vec<String>,
    pub key_files: Vec<String>,
    pub dependencies: Vec<String>,
    pub dependents: Vec<String>,
    pub responsibility: String,
    pub coverage_ratio: f64,
    pub value_score: f64,
    pub risk_score: f64,
    pub conventions: Vec<Convention>,
    pub known_issues: Vec<KnownIssue>,
    pub evidence: Vec<EvidenceLocation>,
    #[serde(default)]
    pub primary_language: Option<String>,
}

impl DetectedModule {
    pub fn new(module_id: impl Into<String>, responsibility: impl Into<String>) -> Self {
        Self {
            module_id: module_id.into(),
            paths: Vec::new(),
            key_files: Vec::new(),
            dependencies: Vec::new(),
            dependents: Vec::new(),
            responsibility: responsibility.into(),
            coverage_ratio: 0.0,
            value_score: 0.5,
            risk_score: 0.5,
            conventions: Vec::new(),
            known_issues: Vec::new(),
            evidence: Vec::new(),
            primary_language: None,
        }
    }

    pub fn paths(mut self, paths: Vec<String>) -> Self {
        self.paths = paths;
        self
    }

    pub fn key_files(mut self, key_files: Vec<String>) -> Self {
        self.key_files = key_files;
        self
    }

    pub fn dependencies(mut self, dependencies: Vec<String>) -> Self {
        self.dependencies = dependencies;
        self
    }

    pub fn dependents(mut self, dependents: Vec<String>) -> Self {
        self.dependents = dependents;
        self
    }

    pub fn metrics(mut self, coverage: f64, value: f64, risk: f64) -> Self {
        self.coverage_ratio = coverage;
        self.value_score = value;
        self.risk_score = risk;
        self
    }

    pub fn conventions(mut self, conventions: Vec<Convention>) -> Self {
        self.conventions = conventions;
        self
    }

    pub fn known_issues(mut self, issues: Vec<KnownIssue>) -> Self {
        self.known_issues = issues;
        self
    }

    pub fn evidence(mut self, evidence: Vec<EvidenceLocation>) -> Self {
        self.evidence = evidence;
        self
    }

    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.primary_language = Some(language.into());
        self
    }

    pub fn to_module(&self) -> Module {
        Module {
            id: self.module_id.clone(),
            name: self.module_id.clone(),
            paths: self.paths.clone(),
            key_files: self.key_files.clone(),
            dependencies: self
                .dependencies
                .iter()
                .map(ModuleDependency::new)
                .collect(),
            dependents: self.dependents.clone(),
            responsibility: self.responsibility.clone(),
            primary_language: self.primary_language.clone().unwrap_or_default(),
            metrics: ModuleMetrics::new(self.coverage_ratio, self.value_score, self.risk_score),
            conventions: self.conventions.clone(),
            known_issues: self.known_issues.clone(),
            evidence: self.evidence.clone(),
        }
    }

    pub fn file_in_module(&self, path: &str) -> bool {
        self.paths.iter().any(|p| path.starts_with(p))
    }
}

impl From<DetectedModule> for Module {
    fn from(dm: DetectedModule) -> Self {
        dm.to_module()
    }
}

/// High-level domain grouping of related module groups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Domain {
    pub name: String,
    pub id: String,
    pub responsibility: String,
    pub group_ids: Vec<String>,
    pub owner: String,
    pub interfaces: Vec<String>,
    pub boundary_rules: Vec<String>,
}

impl Domain {
    pub fn new(name: impl Into<String>, responsibility: impl Into<String>) -> Self {
        let name_str: String = name.into();
        let id = crate::utils::to_kebab_case(&name_str);
        Self {
            name: name_str,
            id,
            responsibility: responsibility.into(),
            group_ids: Vec::new(),
            owner: String::new(),
            interfaces: Vec::new(),
            boundary_rules: Vec::new(),
        }
    }
}

pub fn claudegen_generator() -> GeneratorInfo {
    GeneratorInfo::new("claudegen", env!("CARGO_PKG_VERSION"))
}

pub fn create_module_map(
    detected_modules: Vec<DetectedModule>,
    groups: Vec<ModuleGroup>,
    languages: Vec<DetectedLanguage>,
    workspace_type: WorkspaceType,
    total_files: usize,
    project_name: &str,
    tech_stack: TechStack,
) -> ModuleMap {
    let modules: Vec<Module> = detected_modules.into_iter().map(Into::into).collect();

    let project = ProjectMetadata::new(project_name, tech_stack)
        .with_workspace(WorkspaceInfo {
            workspace_type,
            root: Some(".".into()),
        })
        .with_languages(languages)
        .with_total_files(total_files);

    ModuleMap::new(claudegen_generator(), project, modules, groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detected_module_builder() {
        let dm = DetectedModule::new("pipeline", "Orchestrates generation pipeline")
            .paths(vec!["src/pipeline/".into()])
            .key_files(vec!["src/pipeline/adaptive.rs".into()])
            .dependencies(vec!["types".into()])
            .dependents(vec!["cli".into()])
            .metrics(0.85, 0.9, 0.3)
            .conventions(vec![Convention::new(
                "phase-execution",
                "Execute phases sequentially",
            )])
            .known_issues(vec![KnownIssue::new(
                "oscillation",
                "Quality loop oscillation",
                IssueSeverity::Medium,
                IssueCategory::Correctness,
            )
            .with_prevention("Use strategy rotation")])
            .evidence(vec![EvidenceLocation::new_range(
                "src/pipeline/adaptive.rs",
                64,
                80,
            )])
            .language("rust");

        let module: Module = dm.into();
        assert_eq!(module.id, "pipeline");
        assert_eq!(module.primary_language, "rust");
        assert!((module.metrics.value_score - 0.9).abs() < f64::EPSILON);
        assert_eq!(module.conventions.len(), 1);
        assert_eq!(module.known_issues.len(), 1);
    }

    #[test]
    fn test_create_module_map() {
        let modules = vec![DetectedModule::new("types", "Domain types")
            .paths(vec!["src/types/".into()])
            .metrics(1.0, 0.8, 0.1)];

        let tech_stack = TechStack::new("rust").with_version("1.92");

        let map = create_module_map(
            modules,
            vec![],
            vec![],
            WorkspaceType::SinglePackage,
            100,
            "claudegen",
            tech_stack,
        );

        assert_eq!(map.schema_version, SCHEMA_VERSION);
        assert!(map.find_module("types").is_some());
        assert_eq!(map.project.name, "claudegen");
    }
}
