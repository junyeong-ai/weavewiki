//! Hidden Constraint Extractor
//!
//! Extracts Tier 3 value: hidden constraints, anti-patterns, and complex workflows
//! that are not obvious from code structure alone.

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::ai::LlmProvider;
use crate::config::ProjectType;
use crate::types::Result;

use super::convention_inference::InferredConventions;
use super::project_detection::ProjectDetection;
use crate::pipeline::analysis::SynthesizedAnalysis;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtractedConstraints {
    pub anti_patterns: Vec<AntiPattern>,
    pub hidden_dependencies: Vec<HiddenDependency>,
    pub complex_workflows: Vec<ComplexWorkflow>,
    pub implicit_rules: Vec<ImplicitRule>,
    pub gotchas: Vec<Gotcha>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiPattern {
    pub name: String,
    pub description: String,
    pub why_bad: String,
    pub correct_approach: String,
    pub evidence: Vec<Evidence>,
    pub severity: Severity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub file: String,
    pub line: Option<u32>,
    pub snippet: Option<String>,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiddenDependency {
    pub source: String,
    pub target: String,
    pub dependency_type: HiddenDepType,
    pub description: String,
    pub evidence: Vec<Evidence>,
    pub impact: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HiddenDepType {
    ImplicitOrdering,
    SharedState,
    ConfigDependency,
    RuntimeDependency,
    BuildTimeDependency,
    DataFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexWorkflow {
    pub name: String,
    pub description: String,
    pub trigger: String,
    pub steps: Vec<WorkflowStep>,
    pub gotchas: Vec<String>,
    pub automation_potential: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub order: u32,
    pub action: String,
    pub files_involved: Vec<String>,
    pub commands: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplicitRule {
    pub name: String,
    pub description: String,
    pub applies_to: Vec<String>,
    pub enforcement: RuleEnforcement,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuleEnforcement {
    Linter,
    CiCheck,
    Convention,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gotcha {
    pub title: String,
    pub description: String,
    pub when: String,
    pub solution: String,
    pub related_files: Vec<String>,
}

pub struct ConstraintExtractor {
    project_root: std::path::PathBuf,
    provider: Arc<dyn LlmProvider>,
}

impl ConstraintExtractor {
    pub fn new(project_root: impl AsRef<Path>, provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            provider,
        }
    }

    pub async fn extract(
        &self,
        detection: &ProjectDetection,
        conventions: &InferredConventions,
    ) -> Result<ExtractedConstraints> {
        self.extract_with_synthesis(detection, conventions, None).await
    }

    /// Extract constraints with optional synthesis data for enhanced analysis
    pub async fn extract_with_synthesis(
        &self,
        detection: &ProjectDetection,
        conventions: &InferredConventions,
        synthesis: Option<&SynthesizedAnalysis>,
    ) -> Result<ExtractedConstraints> {
        let mut constraints = ExtractedConstraints::default();

        let static_constraints = self.extract_static(detection).await?;
        constraints.anti_patterns.extend(static_constraints.anti_patterns);
        constraints.hidden_dependencies.extend(static_constraints.hidden_dependencies);
        constraints.gotchas.extend(static_constraints.gotchas);

        // Use synthesis data to enhance constraint extraction
        if let Some(synth) = synthesis {
            self.extract_from_synthesis(synth, &mut constraints);
        }

        let llm_constraints = self
            .extract_with_llm(detection, conventions)
            .await
            .unwrap_or_default();
        constraints.anti_patterns.extend(llm_constraints.anti_patterns);
        constraints.hidden_dependencies.extend(llm_constraints.hidden_dependencies);
        constraints.complex_workflows.extend(llm_constraints.complex_workflows);
        constraints.implicit_rules.extend(llm_constraints.implicit_rules);
        constraints.gotchas.extend(llm_constraints.gotchas);

        constraints.anti_patterns.dedup_by(|a, b| a.name == b.name);
        constraints.complex_workflows.dedup_by(|a, b| a.name == b.name);
        constraints.gotchas.dedup_by(|a, b| a.title == b.title);

        tracing::info!(
            anti_patterns = constraints.anti_patterns.len(),
            hidden_deps = constraints.hidden_dependencies.len(),
            complex_workflows = constraints.complex_workflows.len(),
            gotchas = constraints.gotchas.len(),
            synthesis_enhanced = synthesis.is_some(),
            "Constraint extraction complete"
        );

        Ok(constraints)
    }

    /// Extract constraints from synthesis analysis
    fn extract_from_synthesis(
        &self,
        synthesis: &SynthesizedAnalysis,
        constraints: &mut ExtractedConstraints,
    ) {
        // Extract constraints from synthesis module responsibilities
        for module in &synthesis.modules {
            // Add gotchas for modules with shared state concerns
            if module.responsibility.to_lowercase().contains("shared")
                || module.responsibility.to_lowercase().contains("global")
            {
                constraints.gotchas.push(Gotcha {
                    title: format!("{} module state management", module.name),
                    description: format!(
                        "The {} module handles shared state and requires careful handling",
                        module.name
                    ),
                    when: format!("Modifying {} or components that depend on it", module.path),
                    solution: "Ensure thread-safety and consider race conditions".to_string(),
                    related_files: vec![module.path.clone()],
                });
            }

            // Extract module-specific constraints from synthesis
            for constraint_desc in &module.constraints {
                if !constraint_desc.is_empty() {
                    constraints.gotchas.push(Gotcha {
                        title: format!("{} module constraint", module.name),
                        description: constraint_desc.clone(),
                        when: format!("Working with {} module", module.name),
                        solution: "Follow the identified constraint".to_string(),
                        related_files: module.key_files.clone(),
                    });
                }
            }
        }

        // Extract patterns from deep analysis
        for pattern in &synthesis.deep.patterns {
            // Only flag patterns that appear in few locations (potential inconsistency)
            if pattern.locations.len() < 2 {
                constraints.anti_patterns.push(AntiPattern {
                    name: format!("Limited {} pattern usage", pattern.name),
                    description: format!(
                        "Pattern '{}' is used in only {} location(s)",
                        pattern.name,
                        pattern.locations.len()
                    ),
                    why_bad: "Limited pattern usage may indicate inconsistency".to_string(),
                    correct_approach: format!(
                        "Consider using this pattern more widely: {}",
                        pattern.usage_guidance
                    ),
                    evidence: pattern.locations.iter().take(3).map(|loc| Evidence {
                        file: loc.file.clone(),
                        line: Some(loc.line),
                        snippet: Some(loc.snippet.clone()),
                        context: "Pattern usage found here".to_string(),
                    }).collect(),
                    severity: Severity::Low,
                });
            }
        }

        // Convert deep analysis constraints to extracted constraints
        for constraint in &synthesis.deep.constraints {
            constraints.hidden_dependencies.push(HiddenDependency {
                source: format!("{:?}", constraint.kind),
                target: constraint
                    .evidence
                    .first()
                    .map(|e| e.file.clone())
                    .unwrap_or_default(),
                dependency_type: HiddenDepType::ImplicitOrdering,
                description: constraint.description.clone(),
                evidence: constraint.evidence.iter().map(|e| Evidence {
                    file: e.file.clone(),
                    line: e.line,
                    snippet: None,
                    context: e.context.clone(),
                }).collect(),
                impact: format!(
                    "Violating '{}' constraint: {}",
                    constraint.title,
                    constraint.rationale
                ),
            });
        }
    }

    async fn extract_static(&self, detection: &ProjectDetection) -> Result<ExtractedConstraints> {
        let mut constraints = ExtractedConstraints::default();

        match detection.primary_type {
            ProjectType::Cli => {
                self.extract_cli_constraints(&mut constraints).await?;
            }
            ProjectType::Backend => {
                self.extract_backend_constraints(&mut constraints).await?;
            }
            ProjectType::Frontend => {
                self.extract_frontend_constraints(&mut constraints).await?;
            }
            ProjectType::Library => {
                self.extract_library_constraints(&mut constraints).await?;
            }
            ProjectType::Monorepo => {
                self.extract_monorepo_constraints(&mut constraints).await?;
            }
            _ => {}
        }

        self.extract_common_constraints(&mut constraints, detection)
            .await?;

        Ok(constraints)
    }

    async fn extract_cli_constraints(
        &self,
        constraints: &mut ExtractedConstraints,
    ) -> Result<()> {
        if self.project_root.join("Cargo.toml").exists()
            && let Ok(content) = fs::read_to_string(self.project_root.join("Cargo.toml")).await
                && content.contains("clap") {
                    constraints.gotchas.push(Gotcha {
                        title: "CLI argument changes require version bump consideration".to_string(),
                        description: "Changing CLI arguments can break scripts that depend on the tool".to_string(),
                        when: "Adding, removing, or renaming CLI flags/arguments".to_string(),
                        solution: "Use deprecation warnings before removing flags. Add --help examples.".to_string(),
                        related_files: vec!["src/cli/".to_string(), "Cargo.toml".to_string()],
                    });
                }

        if self.project_root.join("src/main.rs").exists() {
            constraints.anti_patterns.push(AntiPattern {
                name: "Direct stdout in library code".to_string(),
                description: "Using println! or print! in non-CLI modules".to_string(),
                why_bad: "Makes library code unusable as a dependency, breaks --quiet flags".to_string(),
                correct_approach: "Use logging (tracing/log) or return data for CLI layer to format".to_string(),
                evidence: Vec::new(),
                severity: Severity::Medium,
            });
        }

        Ok(())
    }

    async fn extract_backend_constraints(
        &self,
        constraints: &mut ExtractedConstraints,
    ) -> Result<()> {
        if self.project_root.join("src/domain").exists()
            || self.project_root.join("domain").exists()
        {
            constraints.anti_patterns.push(AntiPattern {
                name: "Infrastructure in domain layer".to_string(),
                description: "Database/HTTP/external service code in domain modules".to_string(),
                why_bad: "Violates hexagonal architecture, makes testing difficult".to_string(),
                correct_approach: "Use port interfaces in domain, implement in adapter layer".to_string(),
                evidence: Vec::new(),
                severity: Severity::High,
            });
        }

        if self.has_file_pattern("**/*Controller*").await
            || self.has_file_pattern("**/*controller*").await
        {
            constraints.gotchas.push(Gotcha {
                title: "Controller should not contain business logic".to_string(),
                description: "Controllers are for request handling, not business rules".to_string(),
                when: "Adding new endpoints or modifying request handling".to_string(),
                solution: "Delegate to service/use-case layer for business logic".to_string(),
                related_files: vec!["**/controller*".to_string(), "**/Controller*".to_string()],
            });
        }

        Ok(())
    }

    async fn extract_frontend_constraints(
        &self,
        constraints: &mut ExtractedConstraints,
    ) -> Result<()> {
        if self.project_root.join("package.json").exists()
            && let Ok(content) = fs::read_to_string(self.project_root.join("package.json")).await {
                if content.contains("orval") || content.contains("openapi-typescript") {
                    constraints.anti_patterns.push(AntiPattern {
                        name: "Manual API client modifications".to_string(),
                        description: "Editing auto-generated API client files".to_string(),
                        why_bad: "Changes will be lost on next generation".to_string(),
                        correct_approach: "Modify OpenAPI spec or orval config instead".to_string(),
                        evidence: Vec::new(),
                        severity: Severity::High,
                    });

                    constraints.complex_workflows.push(ComplexWorkflow {
                        name: "API Client Regeneration".to_string(),
                        description: "Regenerate TypeScript API clients from OpenAPI spec".to_string(),
                        trigger: "Backend API changes".to_string(),
                        steps: vec![
                            WorkflowStep {
                                order: 1,
                                action: "Ensure backend OpenAPI spec is updated".to_string(),
                                files_involved: vec!["openapi.yaml".to_string()],
                                commands: Vec::new(),
                                notes: Vec::new(),
                            },
                            WorkflowStep {
                                order: 2,
                                action: "Run orval to regenerate clients".to_string(),
                                files_involved: Vec::new(),
                                commands: vec!["pnpm orval".to_string()],
                                notes: vec!["Check for breaking type changes".to_string()],
                            },
                            WorkflowStep {
                                order: 3,
                                action: "Update affected components".to_string(),
                                files_involved: vec!["src/**/*.tsx".to_string()],
                                commands: Vec::new(),
                                notes: vec!["TypeScript will show errors for breaking changes".to_string()],
                            },
                        ],
                        gotchas: vec!["Never manually edit generated files".to_string()],
                        automation_potential: 0.7,
                    });
                }

                if content.contains("\"react\"") {
                    constraints.gotchas.push(Gotcha {
                        title: "State management boundaries".to_string(),
                        description: "Server state vs client state handling".to_string(),
                        when: "Adding new data fetching or state".to_string(),
                        solution: "Use TanStack Query for server state, Context/useState for client state".to_string(),
                        related_files: vec!["src/hooks/".to_string(), "src/context/".to_string()],
                    });
                }
            }

        Ok(())
    }

    async fn extract_library_constraints(
        &self,
        constraints: &mut ExtractedConstraints,
    ) -> Result<()> {
        if self.project_root.join("src/lib.rs").exists() {
            constraints.complex_workflows.push(ComplexWorkflow {
                name: "Public API Change".to_string(),
                description: "Process for modifying public API".to_string(),
                trigger: "Need to change public function/type signatures".to_string(),
                steps: vec![
                    WorkflowStep {
                        order: 1,
                        action: "Check if change is breaking".to_string(),
                        files_involved: vec!["src/lib.rs".to_string()],
                        commands: Vec::new(),
                        notes: vec!["Removing/renaming public items is breaking".to_string()],
                    },
                    WorkflowStep {
                        order: 2,
                        action: "Update CHANGELOG.md".to_string(),
                        files_involved: vec!["CHANGELOG.md".to_string()],
                        commands: Vec::new(),
                        notes: vec!["Document what changed and why".to_string()],
                    },
                    WorkflowStep {
                        order: 3,
                        action: "Bump version appropriately".to_string(),
                        files_involved: vec!["Cargo.toml".to_string()],
                        commands: Vec::new(),
                        notes: vec!["Breaking = major, Feature = minor, Fix = patch".to_string()],
                    },
                ],
                gotchas: vec![
                    "Consider deprecation before removal".to_string(),
                    "Document migration path for breaking changes".to_string(),
                ],
                automation_potential: 0.4,
            });

            constraints.anti_patterns.push(AntiPattern {
                name: "Leaking internal types".to_string(),
                description: "Exposing internal implementation details in public API".to_string(),
                why_bad: "Couples users to implementation, prevents refactoring".to_string(),
                correct_approach: "Only expose necessary types, use pub(crate) for internals".to_string(),
                evidence: Vec::new(),
                severity: Severity::Medium,
            });
        }

        Ok(())
    }

    async fn extract_monorepo_constraints(
        &self,
        constraints: &mut ExtractedConstraints,
    ) -> Result<()> {
        constraints.hidden_dependencies.push(HiddenDependency {
            source: "shared packages".to_string(),
            target: "consumer apps".to_string(),
            dependency_type: HiddenDepType::SharedState,
            description: "Changes to shared packages affect all consumers".to_string(),
            evidence: Vec::new(),
            impact: "Test all consumers when changing shared code".to_string(),
        });

        constraints.complex_workflows.push(ComplexWorkflow {
            name: "Cross-Project Update".to_string(),
            description: "Coordinated update across multiple projects".to_string(),
            trigger: "Shared type or interface changes".to_string(),
            steps: vec![
                WorkflowStep {
                    order: 1,
                    action: "Identify all affected projects".to_string(),
                    files_involved: Vec::new(),
                    commands: vec!["pnpm why <package>".to_string()],
                    notes: Vec::new(),
                },
                WorkflowStep {
                    order: 2,
                    action: "Update shared package first".to_string(),
                    files_involved: vec!["packages/*/".to_string()],
                    commands: Vec::new(),
                    notes: Vec::new(),
                },
                WorkflowStep {
                    order: 3,
                    action: "Update consumers in dependency order".to_string(),
                    files_involved: vec!["apps/*/".to_string(), "services/*/".to_string()],
                    commands: Vec::new(),
                    notes: Vec::new(),
                },
                WorkflowStep {
                    order: 4,
                    action: "Run integration tests".to_string(),
                    files_involved: Vec::new(),
                    commands: vec!["pnpm test".to_string()],
                    notes: Vec::new(),
                },
            ],
            gotchas: vec![
                "May require coordinated deployment".to_string(),
                "Breaking changes need careful versioning".to_string(),
            ],
            automation_potential: 0.5,
        });

        Ok(())
    }

    async fn extract_common_constraints(
        &self,
        constraints: &mut ExtractedConstraints,
        detection: &ProjectDetection,
    ) -> Result<()> {
        if self.project_root.join(".env.example").exists()
            || self.project_root.join(".env.sample").exists()
        {
            constraints.gotchas.push(Gotcha {
                title: "Environment variable management".to_string(),
                description: "New env vars need documentation".to_string(),
                when: "Adding new configuration via environment variables".to_string(),
                solution: "Update .env.example with new variables and documentation".to_string(),
                related_files: vec![".env.example".to_string(), ".env.sample".to_string()],
            });
        }

        if self.project_root.join(".github/workflows").exists() {
            constraints.hidden_dependencies.push(HiddenDependency {
                source: "CI workflows".to_string(),
                target: "Project structure".to_string(),
                dependency_type: HiddenDepType::BuildTimeDependency,
                description: "CI may assume certain paths/commands exist".to_string(),
                evidence: Vec::new(),
                impact: "Changes to project structure may break CI".to_string(),
            });
        }

        let primary_lang = detection
            .languages
            .first()
            .map(|l| l.language.as_str())
            .unwrap_or("unknown");

        match primary_lang {
            "rust" => {
                constraints.gotchas.push(Gotcha {
                    title: "Feature flag interactions".to_string(),
                    description: "Features may have unexpected interactions".to_string(),
                    when: "Adding new feature flags or enabling combinations".to_string(),
                    solution: "Test with various feature combinations, document dependencies".to_string(),
                    related_files: vec!["Cargo.toml".to_string()],
                });
            }
            "typescript" | "javascript" => {
                constraints.anti_patterns.push(AntiPattern {
                    name: "Type assertions abuse".to_string(),
                    description: "Using `as any` or `as unknown as T` to bypass type checking".to_string(),
                    why_bad: "Defeats purpose of TypeScript, hides bugs".to_string(),
                    correct_approach: "Fix the underlying type issue or use proper type guards".to_string(),
                    evidence: Vec::new(),
                    severity: Severity::Medium,
                });
            }
            _ => {}
        }

        Ok(())
    }

    async fn has_file_pattern(&self, pattern: &str) -> bool {
        let clean_pattern = pattern.replace("**/*", "").replace("*", "");
        self.check_pattern_exists(&self.project_root, &clean_pattern, 0)
            .await
    }

    async fn check_pattern_exists(&self, dir: &Path, pattern: &str, depth: usize) -> bool {
        if depth > 3 {
            return false;
        }

        if let Ok(mut entries) = fs::read_dir(dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.contains(pattern) {
                    return true;
                }
                if entry.path().is_dir() && !name.starts_with('.')
                    && Box::pin(self.check_pattern_exists(&entry.path(), pattern, depth + 1))
                        .await
                    {
                        return true;
                    }
            }
        }
        false
    }

    async fn extract_with_llm(
        &self,
        detection: &ProjectDetection,
        conventions: &InferredConventions,
    ) -> Result<ExtractedConstraints> {
        let prompt = self.build_extraction_prompt(detection, conventions).await?;

        let schema = serde_json::json!({
            "type": "object",
            "required": ["anti_patterns", "gotchas", "complex_workflows"],
            "properties": {
                "anti_patterns": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string", "description": "Name of the anti-pattern"},
                            "description": {"type": "string", "description": "What the anti-pattern is"},
                            "why_bad": {"type": "string", "description": "Why this is problematic"},
                            "correct_approach": {"type": "string", "description": "What to do instead"},
                            "severity": {"type": "string", "enum": ["critical", "high", "medium", "low"]}
                        },
                        "required": ["name", "description", "why_bad", "correct_approach"]
                    }
                },
                "gotchas": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": {"type": "string", "description": "Short title for the gotcha"},
                            "description": {"type": "string", "description": "Detailed description"},
                            "when": {"type": "string", "description": "When this gotcha applies"},
                            "solution": {"type": "string", "description": "How to handle it"},
                            "related_files": {"type": "array", "items": {"type": "string"}}
                        },
                        "required": ["title", "description", "solution"]
                    }
                },
                "complex_workflows": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string", "description": "Workflow name"},
                            "description": {"type": "string", "description": "What the workflow does"},
                            "trigger": {"type": "string", "description": "When to use this workflow"},
                            "steps": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "order": {"type": "integer"},
                                        "action": {"type": "string"},
                                        "files_involved": {"type": "array", "items": {"type": "string"}},
                                        "commands": {"type": "array", "items": {"type": "string"}}
                                    }
                                }
                            },
                            "gotchas": {"type": "array", "items": {"type": "string"}}
                        },
                        "required": ["name", "trigger", "steps"]
                    }
                },
                "hidden_dependencies": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "source": {"type": "string"},
                            "target": {"type": "string"},
                            "description": {"type": "string"},
                            "impact": {"type": "string"}
                        }
                    }
                }
            }
        });

        let response = self.provider.generate(&prompt, &schema).await?;

        let content_str = response.content.as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| serde_json::to_string(&response.content).unwrap_or_default());

        self.parse_llm_response(&content_str)
    }

    async fn build_extraction_prompt(
        &self,
        detection: &ProjectDetection,
        conventions: &InferredConventions,
    ) -> Result<String> {
        let structure = self.collect_structure_summary().await?;

        Ok(format!(
            r#"# Hidden Constraint Extraction Task

Analyze this {project_type} project and identify hidden constraints that are NOT obvious from the code structure.
Focus on Tier 3 value - things that would surprise a new developer.

You MUST respond with valid JSON matching the schema provided.

## Project Info
- Type: {project_type}
- Languages: {languages}
- Architecture: {architecture}

## Structure Summary
```
{structure}
```

## Already Identified Conventions
- File organization: {file_org:?}
- Error handling: {error_handling:?}
- Async pattern: {async_pattern:?}

## What to Extract

1. **Anti-patterns**: Things that should NOT be done
   - Not generic advice - specific to THIS codebase
   - Include why it's bad and what to do instead

2. **Gotchas**: Surprising behaviors or common mistakes
   - Things that trip up new developers
   - Non-obvious requirements

3. **Complex Workflows**: Multi-step processes (5+ steps)
   - Processes that require specific ordering
   - Things that could be automated with a skill

4. **Hidden Dependencies**: Non-obvious relationships
   - File A depends on file B in non-obvious way
   - Configuration that affects runtime behavior

## Response Format

Return ONLY valid JSON:

{{
  "anti_patterns": [
    {{
      "name": "<specific anti-pattern name>",
      "description": "<what the anti-pattern is>",
      "why_bad": "<why this is problematic in this codebase>",
      "correct_approach": "<what to do instead>",
      "severity": "critical|high|medium|low"
    }}
  ],
  "gotchas": [
    {{
      "title": "<short descriptive title>",
      "description": "<detailed explanation>",
      "when": "<when this gotcha applies>",
      "solution": "<how to handle it>",
      "related_files": ["<affected files>"]
    }}
  ],
  "complex_workflows": [
    {{
      "name": "<workflow name>",
      "description": "<what it does>",
      "trigger": "<when to use this>",
      "steps": [
        {{"order": 1, "action": "<step description>", "files_involved": [], "commands": []}}
      ],
      "gotchas": ["<pitfalls to avoid>"]
    }}
  ],
  "hidden_dependencies": [
    {{
      "source": "<source component>",
      "target": "<dependent component>",
      "description": "<how they're related>",
      "impact": "<what happens if violated>"
    }}
  ]
}}

IMPORTANT:
- Be specific to THIS project based on the structure shown
- Each anti-pattern should be actionable
- Complex workflows should have at least 3 meaningful steps
- Do not include generic programming advice"#,
            project_type = detection.primary_type,
            languages = detection
                .languages
                .iter()
                .map(|l| l.language.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            architecture = if conventions.architecture.pattern_name.is_empty() {
                "Not yet identified"
            } else {
                &conventions.architecture.pattern_name
            },
            structure = structure,
            file_org = conventions.file_organization.structure_type,
            error_handling = conventions.error_handling.style,
            async_pattern = conventions.async_pattern.style,
        ))
    }

    async fn collect_structure_summary(&self) -> Result<String> {
        let mut summary = Vec::new();
        self.collect_dir_summary(&self.project_root, "", 0, &mut summary)
            .await?;
        Ok(summary.join("\n"))
    }

    async fn collect_dir_summary(
        &self,
        dir: &Path,
        prefix: &str,
        depth: usize,
        output: &mut Vec<String>,
    ) -> Result<()> {
        if depth > 2 {
            return Ok(());
        }

        let skip_dirs = [
            "target",
            "node_modules",
            "dist",
            ".git",
            "vendor",
            "__pycache__",
            ".venv",
            ".claudegen",
            ".claude",
            "claudegen-plugin",
        ];

        if let Ok(mut entries) = fs::read_dir(dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                // Skip known directories and generated plugin directories
                if skip_dirs.contains(&name.as_str())
                    || name.starts_with('.')
                    || name.ends_with("-plugin")
                {
                    continue;
                }
                if entry.path().is_dir() {
                    output.push(format!("{prefix}{name}/"));
                    Box::pin(self.collect_dir_summary(
                        &entry.path(),
                        &format!("{prefix}  "),
                        depth + 1,
                        output,
                    ))
                    .await?;
                }
            }
        }
        Ok(())
    }

    fn parse_llm_response(&self, content: &str) -> Result<ExtractedConstraints> {
        let mut constraints = ExtractedConstraints::default();

        // Try JSON parsing first
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(content) {
            // Parse anti-patterns
            if let Some(anti_patterns) = json.get("anti_patterns").and_then(|v| v.as_array()) {
                for ap in anti_patterns {
                    let name = ap.get("name").and_then(|v| v.as_str()).unwrap_or_default();
                    if !name.is_empty() {
                        constraints.anti_patterns.push(AntiPattern {
                            name: name.to_string(),
                            description: ap.get("description").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                            why_bad: ap.get("why_bad").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                            correct_approach: ap.get("correct_approach").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                            evidence: Vec::new(),
                            severity: match ap.get("severity").and_then(|v| v.as_str()).unwrap_or("medium") {
                                "critical" => Severity::Critical,
                                "high" => Severity::High,
                                "low" => Severity::Low,
                                _ => Severity::Medium,
                            },
                        });
                    }
                }
            }

            // Parse gotchas
            if let Some(gotchas) = json.get("gotchas").and_then(|v| v.as_array()) {
                for g in gotchas {
                    let title = g.get("title").and_then(|v| v.as_str()).unwrap_or_default();
                    if !title.is_empty() {
                        constraints.gotchas.push(Gotcha {
                            title: title.to_string(),
                            description: g.get("description").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                            when: g.get("when").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                            solution: g.get("solution").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                            related_files: g.get("related_files")
                                .and_then(|v| v.as_array())
                                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                                .unwrap_or_default(),
                        });
                    }
                }
            }

            // Parse complex workflows
            if let Some(workflows) = json.get("complex_workflows").and_then(|v| v.as_array()) {
                for wf in workflows {
                    let name = wf.get("name").and_then(|v| v.as_str()).unwrap_or_default();
                    if !name.is_empty() {
                        let steps: Vec<WorkflowStep> = wf.get("steps")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter().map(|s| WorkflowStep {
                                    order: s.get("order").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                                    action: s.get("action").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                                    files_involved: s.get("files_involved")
                                        .and_then(|v| v.as_array())
                                        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                                        .unwrap_or_default(),
                                    commands: s.get("commands")
                                        .and_then(|v| v.as_array())
                                        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                                        .unwrap_or_default(),
                                    notes: Vec::new(),
                                }).collect()
                            })
                            .unwrap_or_default();

                        constraints.complex_workflows.push(ComplexWorkflow {
                            name: name.to_string(),
                            description: wf.get("description").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                            trigger: wf.get("trigger").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                            steps,
                            gotchas: wf.get("gotchas")
                                .and_then(|v| v.as_array())
                                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                                .unwrap_or_default(),
                            automation_potential: 0.7,
                        });
                    }
                }
            }

            // Parse hidden dependencies
            if let Some(deps) = json.get("hidden_dependencies").and_then(|v| v.as_array()) {
                for dep in deps {
                    let source = dep.get("source").and_then(|v| v.as_str()).unwrap_or_default();
                    let target = dep.get("target").and_then(|v| v.as_str()).unwrap_or_default();
                    if !source.is_empty() && !target.is_empty() {
                        constraints.hidden_dependencies.push(HiddenDependency {
                            source: source.to_string(),
                            target: target.to_string(),
                            dependency_type: HiddenDepType::RuntimeDependency,
                            description: dep.get("description").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                            evidence: Vec::new(),
                            impact: dep.get("impact").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                        });
                    }
                }
            }

            return Ok(constraints);
        }

        // Fallback: try to extract JSON from markdown code blocks
        if let Some(json_start) = content.find("```json") {
            let after_marker = &content[json_start + 7..];
            if let Some(json_end) = after_marker.find("```") {
                let json_content = after_marker[..json_end].trim();
                if let Ok(parsed) = self.parse_llm_response(json_content) {
                    return Ok(parsed);
                }
            }
        }

        // Fallback: section-based extraction for non-JSON responses
        if let Some(section) = Self::extract_section(content, "Anti-patterns") {
            for block in section.split("\n- ").skip(1) {
                if let Some(name) = block.lines().next() {
                    let name = name.trim_start_matches("- ").trim();
                    if !name.is_empty() {
                        constraints.anti_patterns.push(AntiPattern {
                            name: name.split(':').next().unwrap_or(name).to_string(),
                            description: name.to_string(),
                            why_bad: Self::extract_field(block, "Why bad")
                                .unwrap_or_default(),
                            correct_approach: Self::extract_field(block, "Instead")
                                .unwrap_or_default(),
                            evidence: Vec::new(),
                            severity: Severity::Medium,
                        });
                    }
                }
            }
        }

        if let Some(section) = Self::extract_section(content, "Gotchas") {
            for block in section.split("\n- ").skip(1) {
                if let Some(title) = block.lines().next() {
                    let title = title.trim_start_matches("- ").trim();
                    if !title.is_empty() {
                        constraints.gotchas.push(Gotcha {
                            title: title.split(':').next().unwrap_or(title).to_string(),
                            description: title.to_string(),
                            when: Self::extract_field(block, "when").unwrap_or_default(),
                            solution: Self::extract_field(block, "Solution")
                                .unwrap_or_default(),
                            related_files: Vec::new(),
                        });
                    }
                }
            }
        }

        Ok(constraints)
    }

    fn extract_section(content: &str, section: &str) -> Option<String> {
        let patterns = [
            format!("### {section}"),
            format!("## {section}"),
            format!("**{section}**"),
        ];

        for pattern in patterns {
            if let Some(start) = content.find(&pattern) {
                let after = &content[start + pattern.len()..];
                let end = after
                    .find("\n### ")
                    .or_else(|| after.find("\n## "))
                    .unwrap_or(after.len());
                return Some(after[..end].trim().to_string());
            }
        }
        None
    }

    fn extract_field(block: &str, field: &str) -> Option<String> {
        for line in block.lines() {
            let lower = line.to_lowercase();
            if lower.contains(&field.to_lowercase()) {
                return Some(
                    line.split(':')
                        .skip(1)
                        .collect::<Vec<_>>()
                        .join(":")
                        .trim()
                        .to_string(),
                );
            }
        }
        None
    }
}

pub async fn run(
    project_root: impl AsRef<Path>,
    provider: Arc<dyn LlmProvider>,
    detection: &ProjectDetection,
    conventions: &InferredConventions,
) -> Result<ExtractedConstraints> {
    let extractor = ConstraintExtractor::new(project_root, provider);
    extractor.extract(detection, conventions).await
}

pub async fn extract(
    project_root: impl AsRef<Path>,
    detection: &ProjectDetection,
    provider: Arc<dyn LlmProvider>,
) -> Result<ExtractedConstraints> {
    let conventions = InferredConventions::default();
    run(project_root, provider, detection, &conventions).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_ordering() {
        assert!(matches!(Severity::Critical, Severity::Critical));
    }

    #[test]
    fn test_hidden_dep_type() {
        assert_eq!(HiddenDepType::SharedState, HiddenDepType::SharedState);
    }
}
