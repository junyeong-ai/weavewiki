//! Dependency Graph Analysis
//!
//! Tracks module and file dependencies for architectural analysis.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DependencyGraph {
    pub edges: Vec<DependencyEdge>,
    pub modules: HashMap<String, ModuleDependencies>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyEdge {
    pub source: String,
    pub target: String,
    pub dep_type: DependencyType,
    pub weight: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DependencyType {
    Import,
    Inheritance,
    Implementation,
    Composition,
    Call,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModuleDependencies {
    pub module: String,
    pub imports: HashSet<String>,
    pub dependents: HashSet<String>,
    pub external_deps: HashSet<String>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_edge(&mut self, source: String, target: String, dep_type: DependencyType) {
        if let Some(existing) = self
            .edges
            .iter_mut()
            .find(|e| e.source == source && e.target == target)
        {
            existing.weight += 1;
        } else {
            self.edges.push(DependencyEdge {
                source: source.clone(),
                target: target.clone(),
                dep_type,
                weight: 1,
            });
        }

        self.modules
            .entry(source.clone())
            .or_default()
            .imports
            .insert(target.clone());
        self.modules
            .entry(target)
            .or_default()
            .dependents
            .insert(source);
    }

    pub fn get_dependencies(&self, module: &str) -> Vec<&str> {
        self.modules
            .get(module)
            .map(|m| m.imports.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }

    pub fn get_dependents(&self, module: &str) -> Vec<&str> {
        self.modules
            .get(module)
            .map(|m| m.dependents.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }

    pub fn topological_order(&self) -> Vec<String> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();

        for (module, deps) in &self.modules {
            in_degree.entry(module.as_str()).or_insert(0);
            for dep in &deps.imports {
                graph.entry(module.as_str()).or_default().push(dep.as_str());
                *in_degree.entry(dep.as_str()).or_insert(0) += 1;
            }
        }

        let mut queue: Vec<&str> = in_degree
            .iter()
            .filter(|&(_, &d)| d == 0)
            .map(|(&m, _)| m)
            .collect();

        let mut result = Vec::new();

        while let Some(node) = queue.pop() {
            result.push(node.to_string());
            if let Some(deps) = graph.get(node) {
                for &dep in deps {
                    if let Some(degree) = in_degree.get_mut(dep) {
                        *degree = degree.saturating_sub(1);
                        if *degree == 0 {
                            queue.push(dep);
                        }
                    }
                }
            }
        }

        result
    }

    pub fn find_cycles(&self) -> Vec<Vec<String>> {
        let mut cycles = Vec::new();
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut path = Vec::new();

        for module in self.modules.keys() {
            if !visited.contains(module.as_str()) {
                self.dfs_cycle(module, &mut visited, &mut rec_stack, &mut path, &mut cycles);
            }
        }

        cycles
    }

    fn dfs_cycle(
        &self,
        node: &str,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
        path: &mut Vec<String>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        visited.insert(node.to_string());
        rec_stack.insert(node.to_string());
        path.push(node.to_string());

        if let Some(deps) = self.modules.get(node) {
            for dep in &deps.imports {
                if !visited.contains(dep.as_str()) {
                    self.dfs_cycle(dep, visited, rec_stack, path, cycles);
                } else if rec_stack.contains(dep) {
                    let cycle_start = path.iter().position(|n| n == dep).unwrap_or(0);
                    cycles.push(path[cycle_start..].to_vec());
                }
            }
        }

        path.pop();
        rec_stack.remove(node);
    }
}
