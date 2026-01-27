//! Hidden Constraint Extractor
//!
//! Extracts Tier 3 value: hidden constraints, anti-patterns, and complex workflows
//! that are not obvious from code structure alone.

use std::path::Path;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::ai::LlmProvider;
use crate::ai::response::generate_schema;
use crate::ai::validation::deserialize_llm_response;
use crate::types::Result;
use crate::types::severity::Severity;

use super::convention_inference::InferredConventions;
use super::project_detection::ProjectDetection;
use crate::pipeline::analysis::SynthesizedAnalysis;

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct ExtractedConstraints {
    pub anti_patterns: Vec<AntiPattern>,
    pub hidden_dependencies: Vec<HiddenDependency>,
    pub complex_workflows: Vec<ComplexWorkflow>,
    pub implicit_rules: Vec<ImplicitRule>,
    pub gotchas: Vec<Gotcha>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AntiPattern {
    pub name: String,
    pub description: String,
    pub why_bad: String,
    pub correct_approach: String,
    pub evidence: Vec<Evidence>,
    pub severity: Severity,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Evidence {
    pub file: String,
    pub line: Option<u32>,
    pub snippet: Option<String>,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HiddenDependency {
    pub source: String,
    pub target: String,
    pub dependency_type: HiddenDepType,
    pub description: String,
    pub evidence: Vec<Evidence>,
    pub impact: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HiddenDepType {
    ImplicitOrdering,
    SharedState,
    ConfigDependency,
    RuntimeDependency,
    BuildTimeDependency,
    DataFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ComplexWorkflow {
    pub name: String,
    pub description: String,
    pub trigger: String,
    pub steps: Vec<WorkflowStep>,
    pub gotchas: Vec<String>,
    pub automation_potential: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowStep {
    pub order: u32,
    pub action: String,
    pub files_involved: Vec<String>,
    pub commands: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImplicitRule {
    pub name: String,
    pub description: String,
    pub applies_to: Vec<String>,
    pub enforcement: RuleEnforcement,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleEnforcement {
    Linter,
    CiCheck,
    Convention,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
        self.extract_with_synthesis(detection, conventions, None)
            .await
    }

    pub async fn extract_with_synthesis(
        &self,
        detection: &ProjectDetection,
        conventions: &InferredConventions,
        synthesis: Option<&SynthesizedAnalysis>,
    ) -> Result<ExtractedConstraints> {
        let mut constraints = ExtractedConstraints::default();

        if let Some(synth) = synthesis {
            self.extract_from_synthesis(synth, &mut constraints);
        }

        match self.extract_with_llm(detection, conventions).await {
            Ok(llm_constraints) => {
                // Merge LLM constraints with synthesis constraints (append, don't replace)
                constraints.anti_patterns.extend(llm_constraints.anti_patterns);
                constraints.hidden_dependencies.extend(llm_constraints.hidden_dependencies);
                constraints.complex_workflows.extend(llm_constraints.complex_workflows);
                constraints.implicit_rules.extend(llm_constraints.implicit_rules);
                constraints.gotchas.extend(llm_constraints.gotchas);
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    has_synthesis = synthesis.is_some(),
                    "LLM constraint extraction failed, using synthesis-only constraints"
                );
                // Keep synthesis-extracted constraints if available
            }
        }

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
        for module in &synthesis.modules {
            for constraint_desc in &module.constraints {
                if !constraint_desc.is_empty() {
                    constraints.gotchas.push(Gotcha {
                        title: format!("{} module constraint", module.name),
                        description: constraint_desc.clone(),
                        when: format!("Working with {} module", module.name),
                        solution: "Follow the identified constraint".to_string(),
                        related_files: vec![module.path.clone()],
                    });
                }
            }
        }

        for pattern in &synthesis.deep.patterns {
            // Check if pattern is only used in test files
            // More precise test file detection to avoid false positives (e.g., "contest", "attestation")
            if pattern.locations.len() == 1 && is_test_file(&pattern.locations[0].file) {
                constraints.anti_patterns.push(AntiPattern {
                    name: format!("Test-only {} pattern", pattern.name),
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
                    evidence: pattern
                        .locations
                        .iter()
                        .map(|loc| Evidence {
                            file: loc.file.clone(),
                            line: Some(loc.line),
                            snippet: Some(loc.snippet.clone()),
                            context: "Pattern usage found here".to_string(),
                        })
                        .collect(),
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
                evidence: constraint
                    .evidence
                    .iter()
                    .map(|e| Evidence {
                        file: e.file.clone(),
                        line: e.line,
                        snippet: None,
                        context: e.context.clone(),
                    })
                    .collect(),
                impact: format!(
                    "Violating '{}' constraint: {}",
                    constraint.title, constraint.rationale
                ),
            });
        }
    }

    async fn extract_with_llm(
        &self,
        detection: &ProjectDetection,
        conventions: &InferredConventions,
    ) -> Result<ExtractedConstraints> {
        let prompt = self.build_extraction_prompt(detection, conventions).await?;

        let schema = generate_schema::<ConstraintExtractionOutput>();

        let response = self.provider.generate(&prompt, &schema).await?;
        let output: ConstraintExtractionOutput =
            deserialize_llm_response(&response.content, "constraint_extraction")?;

        Ok(self.convert_output(output))
    }

    fn convert_output(&self, output: ConstraintExtractionOutput) -> ExtractedConstraints {
        let anti_patterns = output
            .anti_patterns
            .into_iter()
            .filter(|ap| !ap.name.is_empty())
            .map(|ap| AntiPattern {
                name: ap.name,
                description: ap.description,
                why_bad: ap.why_bad,
                correct_approach: ap.correct_approach,
                evidence: Vec::new(),
                severity: match ap.severity.as_str() {
                    "critical" => Severity::Critical,
                    "high" => Severity::High,
                    "low" => Severity::Low,
                    _ => Severity::Medium,
                },
            })
            .collect();

        let gotchas = output
            .gotchas
            .into_iter()
            .filter(|g| !g.title.is_empty())
            .map(|g| Gotcha {
                title: g.title,
                description: g.description,
                when: g.when,
                solution: g.solution,
                related_files: g.related_files,
            })
            .collect();

        let complex_workflows = output
            .complex_workflows
            .into_iter()
            .filter(|wf| !wf.name.is_empty())
            .map(|wf| ComplexWorkflow {
                name: wf.name,
                description: wf.description,
                trigger: wf.trigger,
                steps: wf
                    .steps
                    .into_iter()
                    .map(|s| WorkflowStep {
                        order: s.order,
                        action: s.action,
                        files_involved: s.files_involved,
                        commands: s.commands,
                        notes: Vec::new(),
                    })
                    .collect(),
                gotchas: wf.gotchas,
                automation_potential: 0.7,
            })
            .collect();

        let hidden_dependencies = output
            .hidden_dependencies
            .into_iter()
            .filter(|hd| !hd.source.is_empty() && !hd.target.is_empty())
            .map(|hd| HiddenDependency {
                source: hd.source,
                target: hd.target,
                dependency_type: HiddenDepType::RuntimeDependency,
                description: hd.description,
                evidence: Vec::new(),
                impact: hd.impact,
            })
            .collect();

        ExtractedConstraints {
            anti_patterns,
            hidden_dependencies,
            complex_workflows,
            implicit_rules: Vec::new(),
            gotchas,
        }
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

/// Check if a file path indicates a test file.
/// Uses common test file patterns across languages to avoid false positives.
fn is_test_file(path: &str) -> bool {
    let lower = path.to_lowercase();

    // Directory-based patterns (tests/, test/, __tests__/, spec/)
    // Check both path starts and contains to handle various formats
    if lower.starts_with("tests/")
        || lower.starts_with("test/")
        || lower.starts_with("__tests__/")
        || lower.starts_with("spec/")
        || lower.contains("/tests/")
        || lower.contains("/test/")
        || lower.contains("/__tests__/")
        || lower.contains("/spec/")
    {
        return true;
    }

    // File naming patterns (more precise than just "contains test")
    // _test.rs, .test.ts, _spec.rb, etc.
    lower.ends_with("_test.rs")
        || lower.ends_with("_test.go")
        || lower.ends_with("_test.py")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".test.tsx")
        || lower.ends_with(".test.js")
        || lower.ends_with(".test.jsx")
        || lower.ends_with(".spec.ts")
        || lower.ends_with(".spec.tsx")
        || lower.ends_with(".spec.js")
        || lower.ends_with(".spec.rb")
        || lower.ends_with("_spec.rb")
        || lower.contains("/test_")  // Python test files
        || lower.ends_with("test.java")  // Java test files
        || lower.ends_with("test.kt")    // Kotlin test files
}

// =============================================================================
// LLM OUTPUT TYPES (for schema generation)
// =============================================================================

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct ConstraintExtractionOutput {
    #[serde(default)]
    anti_patterns: Vec<AntiPatternOutput>,
    #[serde(default)]
    gotchas: Vec<GotchaOutput>,
    #[serde(default)]
    complex_workflows: Vec<WorkflowOutput>,
    #[serde(default)]
    hidden_dependencies: Vec<HiddenDepOutput>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct AntiPatternOutput {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    why_bad: String,
    #[serde(default)]
    correct_approach: String,
    #[serde(default)]
    severity: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct GotchaOutput {
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    when: String,
    #[serde(default)]
    solution: String,
    #[serde(default)]
    related_files: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct WorkflowOutput {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    trigger: String,
    #[serde(default)]
    steps: Vec<StepOutput>,
    #[serde(default)]
    gotchas: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct StepOutput {
    #[serde(default)]
    order: u32,
    #[serde(default)]
    action: String,
    #[serde(default)]
    files_involved: Vec<String>,
    #[serde(default)]
    commands: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct HiddenDepOutput {
    #[serde(default)]
    source: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    impact: String,
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

    #[test]
    fn test_is_test_file() {
        // Positive cases
        assert!(is_test_file("src/tests/unit.rs"));
        assert!(is_test_file("tests/integration.rs"));
        assert!(is_test_file("src/__tests__/component.test.ts"));
        assert!(is_test_file("src/utils_test.go"));
        assert!(is_test_file("spec/models/user_spec.rb"));

        // Negative cases - avoid false positives
        assert!(!is_test_file("src/contest/main.rs"));
        assert!(!is_test_file("src/attestation.rs"));
        assert!(!is_test_file("src/testimony/service.ts"));
        assert!(!is_test_file("src/latest_version.rs"));
    }
}
