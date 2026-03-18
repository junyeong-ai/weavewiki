//! Dependency Graph
//!
//! Tracks relationships between files, modules, and artifacts.
//! Used to determine which artifacts need regeneration when files change.

use std::collections::{HashMap, HashSet};

use modmap::ProjectManifest;
use serde::{Deserialize, Serialize};

use super::ChangeSet;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArtifactRef {
    ProjectRule,
    TechRule(String),
    FrameworkRule(String),
    ModuleRule(String),
    GroupRule(String),
    DomainRule(String),
    CrossCuttingRule(String),
    ModuleAgent(String),
    DomainAgent(String),
    Skill(String),
    ClaudeMd,
}

impl ArtifactRef {
    pub fn output_path(&self) -> String {
        match self {
            Self::ProjectRule => "rules/project.md".into(),
            Self::TechRule(id) => format!("rules/tech/{}.md", id),
            Self::FrameworkRule(id) => format!("rules/frameworks/{}.md", id),
            Self::ModuleRule(id) => format!("rules/modules/{}.md", id),
            Self::GroupRule(id) => format!("rules/groups/{}.md", id),
            Self::DomainRule(id) => format!("rules/domains/{}.md", id),
            Self::CrossCuttingRule(id) => format!("rules/cross-cutting/{}.md", id),
            Self::ModuleAgent(id) => format!("agents/specialists/{}-specialist.md", id),
            Self::DomainAgent(id) => format!("agents/specialists/{}-expert.md", id),
            Self::Skill(id) => format!("skills/{}.md", id),
            Self::ClaudeMd => "CLAUDE.md".into(),
        }
    }
}

pub struct DependencyGraph {
    file_to_module: HashMap<String, String>,
    module_to_artifacts: HashMap<String, Vec<ArtifactRef>>,
    module_dependents: HashMap<String, Vec<String>>,
    group_to_artifacts: HashMap<String, Vec<ArtifactRef>>,
    domain_to_artifacts: HashMap<String, Vec<ArtifactRef>>,
    /// Maps group ID → set of module IDs that belong to this group
    group_members: HashMap<String, HashSet<String>>,
    /// Maps domain ID → set of group IDs that belong to this domain
    domain_groups: HashMap<String, HashSet<String>>,
}

impl DependencyGraph {
    pub fn build(manifest: &ProjectManifest) -> Self {
        let mut file_to_module = HashMap::new();
        let mut module_to_artifacts = HashMap::new();
        let mut module_dependents: HashMap<String, Vec<String>> = HashMap::new();
        let mut group_to_artifacts = HashMap::new();
        let mut domain_to_artifacts = HashMap::new();
        let mut group_members: HashMap<String, HashSet<String>> = HashMap::new();
        let mut domain_groups: HashMap<String, HashSet<String>> = HashMap::new();

        for module in &manifest.project.modules {
            for path_prefix in &module.paths {
                file_to_module.insert(path_prefix.clone(), module.id.clone());
            }

            let artifacts = vec![
                ArtifactRef::ModuleRule(module.id.clone()),
                ArtifactRef::ModuleAgent(module.id.clone()),
                ArtifactRef::Skill(module.id.clone()),
            ];
            module_to_artifacts.insert(module.id.clone(), artifacts);

            for dep in &module.dependencies {
                module_dependents
                    .entry(dep.module_id.clone())
                    .or_default()
                    .push(module.id.clone());
            }
        }

        for group in &manifest.project.groups {
            let artifacts = vec![ArtifactRef::GroupRule(group.id.clone())];
            group_to_artifacts.insert(group.id.clone(), artifacts);

            let members: HashSet<String> = group.module_ids.iter().cloned().collect();
            group_members.insert(group.id.clone(), members);
        }

        for domain in &manifest.project.domains {
            let artifacts = vec![
                ArtifactRef::DomainRule(domain.id.clone()),
                ArtifactRef::DomainAgent(domain.id.clone()),
            ];
            domain_to_artifacts.insert(domain.id.clone(), artifacts);

            let groups: HashSet<String> = domain.group_ids.iter().cloned().collect();
            domain_groups.insert(domain.id.clone(), groups);
        }

        Self {
            file_to_module,
            module_to_artifacts,
            module_dependents,
            group_to_artifacts,
            domain_to_artifacts,
            group_members,
            domain_groups,
        }
    }

    /// Determine which artifacts are affected by changes (unlimited depth)
    ///
    /// For configurable depth limit, use `affected_by_with_depth`.
    pub fn affected_by(&self, changes: &ChangeSet) -> Vec<ArtifactRef> {
        self.affected_by_with_depth(changes, usize::MAX)
    }

    /// Determine which artifacts are affected by changes with configurable depth limit
    ///
    /// # Arguments
    /// * `changes` - The set of file changes
    /// * `max_depth` - Maximum depth for transitive dependency propagation
    ///   - `0`: Only direct dependents
    ///   - `1`: Direct dependents + their direct dependents
    ///   - `2`: Two levels of transitive propagation (default config)
    ///   - `usize::MAX`: Unlimited propagation
    pub fn affected_by_with_depth(&self, changes: &ChangeSet, max_depth: usize) -> Vec<ArtifactRef> {
        let mut affected = HashSet::new();
        let mut directly_changed_modules = HashSet::new();

        for file in changes.all_changed_files() {
            if let Some(module_id) = self.find_module_for_file(file) {
                directly_changed_modules.insert(module_id);
            }
        }

        let all_affected_modules = self.resolve_transitive_with_depth(&directly_changed_modules, max_depth);

        for module_id in &all_affected_modules {
            if let Some(artifacts) = self.module_to_artifacts.get(module_id) {
                affected.extend(artifacts.iter().cloned());
            }
        }

        // Group invalidation: if any member module is affected, invalidate group rule
        for (group_id, members) in &self.group_members {
            if members.iter().any(|m| all_affected_modules.contains(m))
                && let Some(artifacts) = self.group_to_artifacts.get(group_id)
            {
                affected.extend(artifacts.iter().cloned());
            }
        }

        // Domain invalidation: if any member group is affected, invalidate domain artifacts
        let affected_groups: HashSet<_> = self
            .group_members
            .iter()
            .filter(|(_, members)| members.iter().any(|m| all_affected_modules.contains(m)))
            .map(|(group_id, _)| group_id.clone())
            .collect();

        for (domain_id, groups) in &self.domain_groups {
            if groups.iter().any(|g| affected_groups.contains(g))
                && let Some(artifacts) = self.domain_to_artifacts.get(domain_id)
            {
                affected.extend(artifacts.iter().cloned());
            }
        }

        if !changes.added.is_empty() || !changes.deleted.is_empty() {
            affected.insert(ArtifactRef::ProjectRule);
            affected.insert(ArtifactRef::ClaudeMd);
        }

        affected.into_iter().collect()
    }

    /// Resolve transitive dependencies with configurable depth limit
    ///
    /// # Arguments
    /// * `changed` - Set of directly changed modules
    /// * `max_depth` - Maximum propagation depth
    ///   - `0`: Return only the directly changed modules
    ///   - `1`: Return changed + their direct dependents
    ///   - `2+`: Continue propagation up to max_depth levels
    ///
    /// This prevents a single file change in a deeply connected module
    /// from triggering regeneration of the entire project.
    pub fn resolve_transitive_with_depth(
        &self,
        changed: &HashSet<String>,
        max_depth: usize,
    ) -> HashSet<String> {
        let mut visited = changed.clone();

        if max_depth == 0 {
            // No transitive propagation, return only directly changed
            return visited;
        }

        // Use BFS with depth tracking
        let mut current_level: Vec<String> = changed.iter().cloned().collect();
        let mut depth = 0;

        while depth < max_depth && !current_level.is_empty() {
            let mut next_level = Vec::new();

            for module_id in current_level {
                if let Some(dependents) = self.module_dependents.get(&module_id) {
                    for dependent in dependents {
                        if visited.insert(dependent.clone()) {
                            next_level.push(dependent.clone());
                        }
                    }
                }
            }

            current_level = next_level;
            depth += 1;
        }

        if depth >= max_depth && !current_level.is_empty() {
            tracing::debug!(
                "Propagation depth limit ({}) reached, {} modules not propagated further",
                max_depth,
                current_level.len()
            );
        }

        visited
    }

    fn find_module_for_file(&self, file: &str) -> Option<String> {
        let mut best_match: Option<(&str, &str)> = None;

        for (prefix, module_id) in &self.file_to_module {
            if file.starts_with(prefix) {
                match best_match {
                    None => best_match = Some((prefix, module_id)),
                    Some((current_prefix, _)) if prefix.len() > current_prefix.len() => {
                        best_match = Some((prefix, module_id));
                    }
                    _ => {}
                }
            }
        }

        best_match.map(|(_, id)| id.to_string())
    }

}

#[cfg(test)]
impl DependencyGraph {
    pub fn all_module_artifacts(&self) -> Vec<ArtifactRef> {
        self.module_to_artifacts
            .values()
            .flatten()
            .cloned()
            .collect()
    }

    pub fn all_group_artifacts(&self) -> Vec<ArtifactRef> {
        self.group_to_artifacts
            .values()
            .flatten()
            .cloned()
            .collect()
    }

    pub fn all_domain_artifacts(&self) -> Vec<ArtifactRef> {
        self.domain_to_artifacts
            .values()
            .flatten()
            .cloned()
            .collect()
    }

    pub fn all_skill_artifacts(&self) -> Vec<ArtifactRef> {
        self.module_to_artifacts
            .values()
            .flatten()
            .filter(|a| matches!(a, ArtifactRef::Skill(_)))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_graph() -> DependencyGraph {
        let mut file_to_module = HashMap::new();
        file_to_module.insert("src/auth".into(), "auth".into());
        file_to_module.insert("src/api".into(), "api".into());

        let mut module_to_artifacts = HashMap::new();
        module_to_artifacts.insert(
            "auth".into(),
            vec![
                ArtifactRef::ModuleRule("auth".into()),
                ArtifactRef::ModuleAgent("auth".into()),
                ArtifactRef::Skill("auth".into()),
            ],
        );
        module_to_artifacts.insert(
            "api".into(),
            vec![
                ArtifactRef::ModuleRule("api".into()),
                ArtifactRef::ModuleAgent("api".into()),
                ArtifactRef::Skill("api".into()),
            ],
        );

        let mut module_dependents = HashMap::new();
        module_dependents.insert("auth".into(), vec!["api".into()]);

        let mut group_to_artifacts = HashMap::new();
        group_to_artifacts.insert(
            "backend".into(),
            vec![ArtifactRef::GroupRule("backend".into())],
        );

        let mut domain_to_artifacts = HashMap::new();
        domain_to_artifacts.insert(
            "identity".into(),
            vec![
                ArtifactRef::DomainRule("identity".into()),
                ArtifactRef::DomainAgent("identity".into()),
            ],
        );

        let mut group_members = HashMap::new();
        group_members.insert("backend".into(), ["auth".into(), "api".into()].into_iter().collect());

        let mut domain_groups = HashMap::new();
        domain_groups.insert("identity".into(), ["backend".into()].into_iter().collect());

        DependencyGraph {
            file_to_module,
            module_to_artifacts,
            module_dependents,
            group_to_artifacts,
            domain_to_artifacts,
            group_members,
            domain_groups,
        }
    }

    #[test]
    fn test_find_module_for_file() {
        let graph = test_graph();

        assert_eq!(
            graph.find_module_for_file("src/auth/token.rs"),
            Some("auth".into())
        );
        assert_eq!(
            graph.find_module_for_file("src/api/routes.rs"),
            Some("api".into())
        );
        assert_eq!(graph.find_module_for_file("src/other/file.rs"), None);
    }

    #[test]
    fn test_affected_by_modification_with_transitive() {
        let graph = test_graph();

        let changes = ChangeSet {
            added: vec![],
            modified: vec!["src/auth/token.rs".into()],
            deleted: vec![],
        };

        let affected = graph.affected_by(&changes);

        // Direct module
        assert!(affected.contains(&ArtifactRef::ModuleRule("auth".into())));
        assert!(affected.contains(&ArtifactRef::ModuleAgent("auth".into())));
        assert!(affected.contains(&ArtifactRef::Skill("auth".into())));
        // Transitive dependent: api depends on auth
        assert!(affected.contains(&ArtifactRef::ModuleRule("api".into())));
        assert!(affected.contains(&ArtifactRef::ModuleAgent("api".into())));
        assert!(affected.contains(&ArtifactRef::Skill("api".into())));
    }

    #[test]
    fn test_affected_by_addition() {
        let graph = test_graph();

        let changes = ChangeSet {
            added: vec!["src/auth/new.rs".into()],
            modified: vec![],
            deleted: vec![],
        };

        let affected = graph.affected_by(&changes);

        assert!(affected.contains(&ArtifactRef::ProjectRule));
        assert!(affected.contains(&ArtifactRef::ClaudeMd));
        assert!(affected.contains(&ArtifactRef::ModuleRule("auth".into())));
    }

    #[test]
    fn test_artifact_ref_output_path() {
        assert_eq!(ArtifactRef::ProjectRule.output_path(), "rules/project.md");
        assert_eq!(
            ArtifactRef::ModuleRule("auth".into()).output_path(),
            "rules/modules/auth.md"
        );
        assert_eq!(
            ArtifactRef::ModuleAgent("auth".into()).output_path(),
            "agents/specialists/auth-specialist.md"
        );
        assert_eq!(
            ArtifactRef::Skill("auth".into()).output_path(),
            "skills/auth.md"
        );
        assert_eq!(ArtifactRef::ClaudeMd.output_path(), "CLAUDE.md");
    }

    #[test]
    fn test_all_module_artifacts() {
        let graph = test_graph();
        let artifacts = graph.all_module_artifacts();

        assert_eq!(artifacts.len(), 6);
    }

    #[test]
    fn test_all_group_artifacts() {
        let graph = test_graph();
        let artifacts = graph.all_group_artifacts();

        assert_eq!(artifacts.len(), 1);
    }

    #[test]
    fn test_all_domain_artifacts() {
        let graph = test_graph();
        let artifacts = graph.all_domain_artifacts();

        assert_eq!(artifacts.len(), 2);
    }

    #[test]
    fn test_all_skill_artifacts() {
        let graph = test_graph();
        let artifacts = graph.all_skill_artifacts();

        assert_eq!(artifacts.len(), 2);
        assert!(artifacts.contains(&ArtifactRef::Skill("auth".into())));
        assert!(artifacts.contains(&ArtifactRef::Skill("api".into())));
    }

    #[test]
    fn test_resolve_transitive_handles_cycles() {
        let mut module_dependents: HashMap<String, Vec<String>> = HashMap::new();
        // Circular: a → b → c → a
        module_dependents.insert("a".into(), vec!["b".into()]);
        module_dependents.insert("b".into(), vec!["c".into()]);
        module_dependents.insert("c".into(), vec!["a".into()]);

        let graph = DependencyGraph {
            file_to_module: HashMap::new(),
            module_to_artifacts: HashMap::new(),
            module_dependents,
            group_to_artifacts: HashMap::new(),
            domain_to_artifacts: HashMap::new(),
            group_members: HashMap::new(),
            domain_groups: HashMap::new(),
        };

        let changed: HashSet<String> = ["a".into()].into_iter().collect();
        let result = graph.resolve_transitive_with_depth(&changed, usize::MAX);

        assert_eq!(result.len(), 3);
        assert!(result.contains("a"));
        assert!(result.contains("b"));
        assert!(result.contains("c"));
    }

    fn deep_chain_graph() -> DependencyGraph {
        // Create a chain: a → b → c → d → e
        let mut module_dependents: HashMap<String, Vec<String>> = HashMap::new();
        module_dependents.insert("a".into(), vec!["b".into()]);
        module_dependents.insert("b".into(), vec!["c".into()]);
        module_dependents.insert("c".into(), vec!["d".into()]);
        module_dependents.insert("d".into(), vec!["e".into()]);

        let mut module_to_artifacts = HashMap::new();
        for id in ["a", "b", "c", "d", "e"] {
            module_to_artifacts.insert(
                id.to_string(),
                vec![ArtifactRef::ModuleRule(id.to_string())],
            );
        }

        let mut file_to_module = HashMap::new();
        file_to_module.insert("src/a".into(), "a".into());

        DependencyGraph {
            file_to_module,
            module_to_artifacts,
            module_dependents,
            group_to_artifacts: HashMap::new(),
            domain_to_artifacts: HashMap::new(),
            group_members: HashMap::new(),
            domain_groups: HashMap::new(),
        }
    }

    #[test]
    fn test_resolve_transitive_with_depth_zero() {
        let graph = deep_chain_graph();
        let changed: HashSet<String> = ["a".into()].into_iter().collect();

        // Depth 0: only the directly changed module
        let result = graph.resolve_transitive_with_depth(&changed, 0);
        assert_eq!(result.len(), 1);
        assert!(result.contains("a"));
        assert!(!result.contains("b"));
    }

    #[test]
    fn test_resolve_transitive_with_depth_one() {
        let graph = deep_chain_graph();
        let changed: HashSet<String> = ["a".into()].into_iter().collect();

        // Depth 1: changed + direct dependents
        let result = graph.resolve_transitive_with_depth(&changed, 1);
        assert_eq!(result.len(), 2);
        assert!(result.contains("a"));
        assert!(result.contains("b"));
        assert!(!result.contains("c"));
    }

    #[test]
    fn test_resolve_transitive_with_depth_two() {
        let graph = deep_chain_graph();
        let changed: HashSet<String> = ["a".into()].into_iter().collect();

        // Depth 2 (default): changed + 2 levels of dependents
        let result = graph.resolve_transitive_with_depth(&changed, 2);
        assert_eq!(result.len(), 3);
        assert!(result.contains("a"));
        assert!(result.contains("b"));
        assert!(result.contains("c"));
        assert!(!result.contains("d"));
    }

    #[test]
    fn test_resolve_transitive_with_unlimited_depth() {
        let graph = deep_chain_graph();
        let changed: HashSet<String> = ["a".into()].into_iter().collect();

        // Unlimited depth: all transitively connected
        let result = graph.resolve_transitive_with_depth(&changed, usize::MAX);
        assert_eq!(result.len(), 5);
        assert!(result.contains("a"));
        assert!(result.contains("b"));
        assert!(result.contains("c"));
        assert!(result.contains("d"));
        assert!(result.contains("e"));
    }

    #[test]
    fn test_affected_by_with_depth_limits_propagation() {
        let graph = deep_chain_graph();

        let changes = ChangeSet {
            added: vec![],
            modified: vec!["src/a/file.rs".into()],
            deleted: vec![],
        };

        // With depth 1, should only affect a and b
        let affected = graph.affected_by_with_depth(&changes, 1);
        assert!(affected.contains(&ArtifactRef::ModuleRule("a".into())));
        assert!(affected.contains(&ArtifactRef::ModuleRule("b".into())));
        assert!(!affected.contains(&ArtifactRef::ModuleRule("c".into())));
    }

    #[test]
    fn test_resolve_transitive_with_depth_handles_cycles() {
        let mut module_dependents: HashMap<String, Vec<String>> = HashMap::new();
        // Circular: a → b → c → a
        module_dependents.insert("a".into(), vec!["b".into()]);
        module_dependents.insert("b".into(), vec!["c".into()]);
        module_dependents.insert("c".into(), vec!["a".into()]);

        let graph = DependencyGraph {
            file_to_module: HashMap::new(),
            module_to_artifacts: HashMap::new(),
            module_dependents,
            group_to_artifacts: HashMap::new(),
            domain_to_artifacts: HashMap::new(),
            group_members: HashMap::new(),
            domain_groups: HashMap::new(),
        };

        let changed: HashSet<String> = ["a".into()].into_iter().collect();

        // Even with a cycle, depth limit should work
        let result = graph.resolve_transitive_with_depth(&changed, 1);
        assert_eq!(result.len(), 2);
        assert!(result.contains("a"));
        assert!(result.contains("b"));

        // With depth 2, we get all 3 (cycle completes)
        let result = graph.resolve_transitive_with_depth(&changed, 2);
        assert_eq!(result.len(), 3);
    }
}
