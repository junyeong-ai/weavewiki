use crate::types::Result;
use crate::types::module_map::{
    DetectedLanguage, DetectedModule, ModuleGroup, ModuleMap, TechStack, WorkspaceType,
    create_module_map,
};

pub struct ModuleMapGenerator;

impl ModuleMapGenerator {
    pub fn generate(
        modules: &[DetectedModule],
        groups: &[ModuleGroup],
        languages: &[DetectedLanguage],
        workspace_type: WorkspaceType,
        total_files: usize,
        project_name: &str,
        tech_stack: TechStack,
    ) -> Result<ModuleMap> {
        Ok(create_module_map(
            modules.to_vec(),
            groups.to_vec(),
            languages.to_vec(),
            workspace_type,
            total_files,
            project_name,
            tech_stack,
        ))
    }

    pub fn generate_simple(
        modules: &[DetectedModule],
        groups: &[ModuleGroup],
        project_name: &str,
    ) -> Result<ModuleMap> {
        Self::generate(
            modules,
            groups,
            &[],
            WorkspaceType::SinglePackage,
            0,
            project_name,
            TechStack::default(),
        )
    }

    pub fn to_json(
        modules: &[DetectedModule],
        groups: &[ModuleGroup],
        languages: &[DetectedLanguage],
        workspace_type: WorkspaceType,
        total_files: usize,
        project_name: &str,
        tech_stack: TechStack,
    ) -> Result<String> {
        let map = Self::generate(
            modules,
            groups,
            languages,
            workspace_type,
            total_files,
            project_name,
            tech_stack,
        )?;
        map.to_json().map_err(|e| {
            crate::types::ClaudegenError::Config(format!("Failed to serialize module map: {e}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_module_map() {
        let modules = vec![DetectedModule::new("test", "Test module")
            .paths(vec!["src/test/".to_string()])
            .metrics(1.0, 0.5, 0.1)];

        let map = ModuleMapGenerator::generate_simple(&modules, &[], "test-project").unwrap();
        let json = map.to_json().unwrap();
        assert!(json.contains("\"schema_version\""));
        assert!(json.contains("\"test\""));
    }
}
