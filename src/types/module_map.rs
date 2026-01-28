use serde::{Deserialize, Serialize};

use super::node::EvidenceLocation;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedModule {
    pub module_id: String,
    pub paths: Vec<String>,
    pub key_files: Vec<String>,
    pub dependencies: Vec<String>,
    pub responsibility: String,
    pub coverage_ratio: f32,
    pub value_score: f32,
    pub risk_score: f32,
    pub conventions: Vec<String>,
    pub known_issues: Vec<String>,
    pub evidence: Vec<EvidenceLocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleGroup {
    pub group_id: String,
    pub name: String,
    pub module_ids: Vec<String>,
    pub responsibility: String,
    pub external_dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleMap {
    pub module_map_version: String,
    pub modules: Vec<DetectedModule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<ModuleGroup>,
}

impl ModuleMap {
    pub fn new(modules: Vec<DetectedModule>, groups: Vec<ModuleGroup>) -> Self {
        Self {
            module_map_version: "1.0.0".to_string(),
            modules,
            groups,
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn find_module(&self, module_id: &str) -> Option<&DetectedModule> {
        self.modules.iter().find(|m| m.module_id == module_id)
    }

    pub fn module_ids(&self) -> Vec<&str> {
        self.modules.iter().map(|m| m.module_id.as_str()).collect()
    }

    pub fn find_group(&self, group_id: &str) -> Option<&ModuleGroup> {
        self.groups.iter().find(|g| g.group_id == group_id)
    }

    pub fn group_for_module(&self, module_id: &str) -> Option<&ModuleGroup> {
        self.groups
            .iter()
            .find(|g| g.module_ids.iter().any(|id| id == module_id))
    }

    pub fn is_grouped(&self) -> bool {
        !self.groups.is_empty()
    }
}

impl DetectedModule {
    pub fn file_in_module(&self, path: &str) -> bool {
        self.paths.iter().any(|p| path.starts_with(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_map_serialization() {
        let map = ModuleMap::new(vec![DetectedModule {
            module_id: "pipeline".to_string(),
            paths: vec!["src/pipeline/".to_string()],
            key_files: vec!["src/pipeline/adaptive.rs".to_string()],
            dependencies: vec!["types".to_string(), "config".to_string()],
            responsibility: "Orchestrates generation pipeline".to_string(),
            coverage_ratio: 0.85,
            value_score: 0.9,
            risk_score: 0.3,
            conventions: vec!["Phase-based execution".to_string()],
            known_issues: vec![],
            evidence: vec![EvidenceLocation {
                file: "src/pipeline/adaptive.rs".to_string(),
                start_line: 64,
                end_line: 80,
                start_column: None,
                end_column: None,
            }],
        }], vec![]);

        let json = map.to_json().unwrap();
        assert!(json.contains("pipeline"));
        assert!(json.contains("1.0.0"));
    }

    #[test]
    fn test_find_module() {
        let map = ModuleMap::new(vec![DetectedModule {
            module_id: "types".to_string(),
            paths: vec!["src/types/".to_string()],
            key_files: vec![],
            dependencies: vec![],
            responsibility: "Domain types".to_string(),
            coverage_ratio: 1.0,
            value_score: 0.8,
            risk_score: 0.1,
            conventions: vec![],
            known_issues: vec![],
            evidence: vec![],
        }], vec![]);

        assert!(map.find_module("types").is_some());
        assert!(map.find_module("nonexistent").is_none());
    }

    #[test]
    fn test_file_in_module() {
        let module = DetectedModule {
            module_id: "pipeline".to_string(),
            paths: vec!["src/pipeline/".to_string()],
            key_files: vec![],
            dependencies: vec![],
            responsibility: "".to_string(),
            coverage_ratio: 0.0,
            value_score: 0.0,
            risk_score: 0.0,
            conventions: vec![],
            known_issues: vec![],
            evidence: vec![],
        };

        assert!(module.file_in_module("src/pipeline/adaptive.rs"));
        assert!(!module.file_in_module("src/types/mod.rs"));
    }
}
