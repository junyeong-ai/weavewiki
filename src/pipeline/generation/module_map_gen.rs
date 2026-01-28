use crate::types::module_map::{DetectedModule, ModuleGroup, ModuleMap};
use crate::types::Result;

pub struct ModuleMapGenerator;

impl ModuleMapGenerator {
    pub fn generate(modules: &[DetectedModule], groups: &[ModuleGroup]) -> Result<ModuleMap> {
        Ok(ModuleMap::new(modules.to_vec(), groups.to_vec()))
    }

    pub fn to_json(modules: &[DetectedModule], groups: &[ModuleGroup]) -> Result<String> {
        let map = Self::generate(modules, groups)?;
        map.to_json().map_err(|e| {
            crate::types::ClaudegenError::Config(format!(
                "Failed to serialize module map: {e}"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_module_map() {
        let modules = vec![DetectedModule {
            module_id: "test".to_string(),
            paths: vec!["src/test/".to_string()],
            key_files: vec![],
            dependencies: vec![],
            responsibility: "Test module".to_string(),
            coverage_ratio: 1.0,
            value_score: 0.5,
            risk_score: 0.1,
            conventions: vec![],
            known_issues: vec![],
            evidence: vec![],
        }];

        let json = ModuleMapGenerator::to_json(&modules, &[]).unwrap();
        assert!(json.contains("\"module_map_version\""));
        assert!(json.contains("\"test\""));
    }
}
