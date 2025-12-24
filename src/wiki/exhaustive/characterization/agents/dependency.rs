//! Dependency Agent - Turn 1
//!
//! Analyzes import/use statements and dependency patterns using LLM.
//! Includes circular dependency detection (T083).

use super::helpers::{calculate_confidence, parse_json_response};
use super::{DependencyInsight, InternalDependency};
use crate::types::error::WeaveError;
use crate::wiki::exhaustive::characterization::schemas::{AgentPrompts, AgentSchemas};
use crate::wiki::exhaustive::characterization::{
    AgentOutput, CharacterizationAgent, CharacterizationContext,
};
use std::collections::{HashMap, HashSet};

pub struct DependencyAgent;

impl DependencyAgent {
    /// Build import data string from context
    fn build_import_data(context: &CharacterizationContext) -> String {
        let mut data = String::from("File paths and their likely dependencies:\n");
        for file in &context.files {
            data.push_str(&format!(
                "- {} ({})\n",
                file.path,
                file.language.as_deref().unwrap_or("unknown")
            ));
        }
        data
    }

    /// Fallback analysis when LLM fails (language-agnostic)
    fn fallback_analysis(context: &CharacterizationContext) -> DependencyInsight {
        let mut internal_deps = vec![];

        // Package manager detection patterns (universal)
        let package_managers: &[(&str, &str)] = &[
            ("Cargo.toml", "Rust/Cargo"),
            ("Cargo.lock", "Rust/Cargo"),
            ("package.json", "Node.js/npm"),
            ("yarn.lock", "Node.js/Yarn"),
            ("pnpm-lock.yaml", "Node.js/pnpm"),
            ("go.mod", "Go modules"),
            ("go.sum", "Go modules"),
            ("requirements.txt", "Python/pip"),
            ("Pipfile", "Python/Pipenv"),
            ("pyproject.toml", "Python/poetry or pip"),
            ("setup.py", "Python/setuptools"),
            ("poetry.lock", "Python/poetry"),
            ("pom.xml", "Java/Maven"),
            ("build.gradle", "Java/Gradle"),
            ("build.gradle.kts", "Kotlin/Gradle"),
            ("Gemfile", "Ruby/Bundler"),
            ("Gemfile.lock", "Ruby/Bundler"),
            ("composer.json", "PHP/Composer"),
            ("composer.lock", "PHP/Composer"),
            (".csproj", ".NET/MSBuild"),
            (".fsproj", "F#/MSBuild"),
            ("packages.config", ".NET/NuGet"),
            ("Package.swift", "Swift/SPM"),
            ("mix.exs", "Elixir/Mix"),
            ("build.sbt", "Scala/SBT"),
        ];

        let mut indicators = vec![];
        for file in &context.files {
            let filename = file.path.split('/').next_back().unwrap_or(&file.path);
            for (pattern, description) in package_managers {
                if filename == *pattern || file.path.ends_with(pattern) {
                    let desc = description.to_string();
                    if !indicators.contains(&desc) {
                        indicators.push(desc);
                    }
                }
            }
        }

        // Infer module dependencies from directory structure
        let dirs: HashSet<String> = context
            .files
            .iter()
            .filter_map(|f| f.path.split('/').next().map(String::from))
            .collect();

        for dir in &dirs {
            if dir == "src" || dir == "lib" {
                continue;
            }
            internal_deps.push(InternalDependency {
                from: "main".to_string(),
                to: dir.clone(),
                dependency_type: Some("module".to_string()),
            });
        }

        // Detect circular dependencies (T083)
        let circular_deps = Self::detect_circular_dependencies(&internal_deps);

        DependencyInsight {
            internal_deps,
            external_deps: vec![],
            framework_indicators: indicators,
            circular_deps,
        }
    }

    /// Detect circular dependencies from internal deps (T083)
    fn detect_circular_dependencies(deps: &[InternalDependency]) -> Vec<Vec<String>> {
        let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut all_nodes: HashSet<&str> = HashSet::new();

        for dep in deps {
            graph
                .entry(dep.from.as_str())
                .or_default()
                .push(dep.to.as_str());
            all_nodes.insert(dep.from.as_str());
            all_nodes.insert(dep.to.as_str());
        }

        let mut cycles: Vec<Vec<String>> = vec![];
        let mut visited: HashSet<&str> = HashSet::new();
        let mut rec_stack: HashSet<&str> = HashSet::new();
        let mut path: Vec<&str> = vec![];

        for node in &all_nodes {
            if !visited.contains(node) {
                Self::find_cycles_dfs(
                    node,
                    &graph,
                    &mut visited,
                    &mut rec_stack,
                    &mut path,
                    &mut cycles,
                );
            }
        }

        // Deduplicate cycles
        let mut unique_cycles: Vec<Vec<String>> = vec![];
        for cycle in cycles {
            let normalized = Self::normalize_cycle(&cycle);
            if !unique_cycles
                .iter()
                .any(|c| Self::normalize_cycle(c) == normalized)
            {
                unique_cycles.push(cycle);
            }
        }

        unique_cycles
    }

    fn find_cycles_dfs<'a>(
        node: &'a str,
        graph: &HashMap<&'a str, Vec<&'a str>>,
        visited: &mut HashSet<&'a str>,
        rec_stack: &mut HashSet<&'a str>,
        path: &mut Vec<&'a str>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        visited.insert(node);
        rec_stack.insert(node);
        path.push(node);

        if let Some(neighbors) = graph.get(node) {
            for &neighbor in neighbors {
                if !visited.contains(neighbor) {
                    Self::find_cycles_dfs(neighbor, graph, visited, rec_stack, path, cycles);
                } else if rec_stack.contains(neighbor)
                    && let Some(start_idx) = path.iter().position(|&n| n == neighbor)
                {
                    let cycle: Vec<String> =
                        path[start_idx..].iter().map(|s| s.to_string()).collect();
                    if cycle.len() >= 2 {
                        cycles.push(cycle);
                    }
                }
            }
        }

        path.pop();
        rec_stack.remove(node);
    }

    fn normalize_cycle(cycle: &[String]) -> Vec<String> {
        if cycle.is_empty() {
            return vec![];
        }
        let min_idx = cycle
            .iter()
            .enumerate()
            .min_by_key(|(_, s)| s.as_str())
            .map(|(i, _)| i)
            .unwrap_or(0);

        let mut normalized = cycle[min_idx..].to_vec();
        normalized.extend_from_slice(&cycle[..min_idx]);
        normalized
    }

    /// Post-process LLM response to detect/validate circular deps (T083)
    fn post_process_insight(mut insight: DependencyInsight) -> DependencyInsight {
        if insight.circular_deps.is_empty() {
            insight.circular_deps = Self::detect_circular_dependencies(&insight.internal_deps);
        }

        for cycle in &insight.circular_deps {
            tracing::warn!("Circular dependency detected: {}", cycle.join(" -> "));
        }

        insight
    }
}

#[async_trait::async_trait]
impl CharacterizationAgent for DependencyAgent {
    fn name(&self) -> &str {
        "dependency"
    }

    fn turn(&self) -> u8 {
        1
    }

    async fn run(&self, context: &CharacterizationContext) -> Result<AgentOutput, WeaveError> {
        tracing::debug!("DependencyAgent: Analyzing {} files", context.files.len());

        let import_data = Self::build_import_data(context);
        let user_prompt = AgentPrompts::dependency_prompt(&import_data);
        let schema = AgentSchemas::dependency_schema();
        let full_prompt = format!("{}\n\n{}", AgentPrompts::system_prompt(), user_prompt);

        let response = context
            .provider
            .generate(&full_prompt, &schema)
            .await
            .map_err(|e| WeaveError::LlmApi(format!("Dependency agent LLM call failed: {}", e)))?;

        // Parse with post-processing for circular dependency detection
        let insight: DependencyInsight =
            match parse_json_response(&response.content, "DependencyInsight") {
                Ok(i) => Self::post_process_insight(i),
                Err(e) => {
                    tracing::warn!("DependencyAgent: Fallback due to parse error: {}", e);
                    Self::fallback_analysis(context)
                }
            };

        let insight_json = serde_json::to_value(&insight)
            .map_err(|e| WeaveError::LlmApi(format!("Failed to serialize insight: {}", e)))?;

        tracing::debug!(
            "DependencyAgent: Found {} internal deps, {} external deps",
            insight.internal_deps.len(),
            insight.external_deps.len()
        );

        let is_empty = insight.internal_deps.is_empty() && insight.external_deps.is_empty();

        Ok(AgentOutput {
            agent_name: self.name().to_string(),
            turn: self.turn(),
            insight_json,
            confidence: calculate_confidence(is_empty),
        })
    }
}
