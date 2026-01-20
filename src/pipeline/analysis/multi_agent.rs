//! Multi-Agent Analysis System
//!
//! Parallel specialist agents for comprehensive codebase analysis.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ai::LlmProvider;
use crate::config::AnalysisSpecialty;
use crate::types::Result;

use super::deep_analyzer::{CoreModule, DiscoveredConstraint, EntryPoint, PatternInstance};
use super::DeepAnalysisResult;

#[derive(Debug, Clone, Default)]
pub struct MultiAgentResult {
    pub structure: StructureResult,
    pub patterns: Vec<PatternInstance>,
    pub constraints: Vec<DiscoveredConstraint>,
    pub gaps: Vec<AnalysisGap>,
    pub specialist_confidences: HashMap<AnalysisSpecialty, f32>,
}

#[derive(Debug, Clone, Default)]
pub struct StructureResult {
    pub entry_points: Vec<EntryPoint>,
    pub core_modules: Vec<CoreModule>,
    pub layer_boundaries: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AnalysisGap {
    pub area: String,
    pub description: String,
    pub severity: GapSeverity,
}

#[derive(Debug, Clone, Copy)]
pub enum GapSeverity {
    Low,
    Medium,
    High,
}

#[async_trait]
pub trait SpecialistAgent: Send + Sync {
    fn specialty(&self) -> AnalysisSpecialty;
    async fn analyze(&self, context: &AnalysisContext) -> Result<SpecialistResult>;
}

#[derive(Debug, Clone)]
pub struct AnalysisContext {
    pub project_root: String,
    pub file_list: Vec<String>,
    pub file_contents: HashMap<String, String>,
    pub project_type: String,
    pub languages: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SpecialistResult {
    pub specialty: Option<AnalysisSpecialty>,
    pub confidence: f32,
    pub findings: Value,
    pub gaps: Vec<AnalysisGap>,
}

pub struct MultiAgentAnalyzer {
    provider: Arc<dyn LlmProvider>,
    enabled: bool,
    specialists: Vec<AnalysisSpecialty>,
    timeout_secs: u64,
    cross_validate: bool,
}

impl MultiAgentAnalyzer {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            enabled: true,
            specialists: vec![
                AnalysisSpecialty::Structure,
                AnalysisSpecialty::Pattern,
                AnalysisSpecialty::Constraint,
            ],
            timeout_secs: 60,
            cross_validate: true,
        }
    }

    pub fn with_specialists(mut self, specialists: Vec<AnalysisSpecialty>) -> Self {
        self.specialists = specialists;
        self
    }

    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    pub fn with_cross_validation(mut self, enabled: bool) -> Self {
        self.cross_validate = enabled;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub async fn analyze(&self, context: AnalysisContext) -> Result<MultiAgentResult> {
        if !self.enabled {
            return Ok(MultiAgentResult::default());
        }

        let context = Arc::new(context);
        let timeout = Duration::from_secs(self.timeout_secs);

        let mut handles = Vec::new();
        for specialty in &self.specialists {
            let provider = Arc::clone(&self.provider);
            let ctx = Arc::clone(&context);
            let specialty = *specialty;

            let handle = tokio::spawn(async move {
                let specialist = Self::create_specialist(provider, specialty);
                tokio::time::timeout(timeout, specialist.analyze(&ctx)).await
            });
            handles.push((specialty, handle));
        }

        let mut all_results = Vec::new();
        let mut failed_count = 0;

        for (specialty, handle) in handles {
            match handle.await {
                Ok(Ok(Ok(result))) => all_results.push(result),
                Ok(Ok(Err(e))) => {
                    tracing::warn!(?specialty, error = %e, "Specialist analysis failed");
                    failed_count += 1;
                    all_results.push(SpecialistResult {
                        specialty: Some(specialty),
                        confidence: 0.0,
                        findings: Value::Null,
                        gaps: vec![AnalysisGap {
                            area: format!("{:?}", specialty).to_lowercase(),
                            description: format!("Analysis error: {}", e),
                            severity: GapSeverity::High,
                        }],
                    });
                }
                Ok(Err(_)) => {
                    tracing::warn!(?specialty, timeout_secs = self.timeout_secs, "Specialist timed out");
                    failed_count += 1;
                    all_results.push(SpecialistResult {
                        specialty: Some(specialty),
                        confidence: 0.0,
                        findings: Value::Null,
                        gaps: vec![AnalysisGap {
                            area: format!("{:?}", specialty).to_lowercase(),
                            description: format!("Timed out after {}s", self.timeout_secs),
                            severity: GapSeverity::Medium,
                        }],
                    });
                }
                Err(e) => {
                    tracing::error!(?specialty, error = %e, "Specialist task panicked");
                    failed_count += 1;
                    all_results.push(SpecialistResult {
                        specialty: Some(specialty),
                        confidence: 0.0,
                        findings: Value::Null,
                        gaps: vec![AnalysisGap {
                            area: format!("{:?}", specialty).to_lowercase(),
                            description: "Internal error (task panic)".to_string(),
                            severity: GapSeverity::High,
                        }],
                    });
                }
            }
        }

        if failed_count > 0 {
            let total = self.specialists.len();
            tracing::warn!(failed = failed_count, total = total, "Multi-agent analysis partially failed");
        }

        self.synthesize_results(all_results)
    }

    fn create_specialist(provider: Arc<dyn LlmProvider>, specialty: AnalysisSpecialty) -> Box<dyn SpecialistAgent> {
        match specialty {
            AnalysisSpecialty::Structure => Box::new(StructureSpecialist::new(provider)),
            AnalysisSpecialty::Pattern => Box::new(PatternSpecialist::new(provider.clone())),
            AnalysisSpecialty::Constraint => Box::new(ConstraintSpecialist::new(provider.clone())),
            // Additional specialties use pattern specialist as fallback
            AnalysisSpecialty::Architecture
            | AnalysisSpecialty::Security
            | AnalysisSpecialty::Performance
            | AnalysisSpecialty::Testing
            | AnalysisSpecialty::Documentation
            | AnalysisSpecialty::Domain => Box::new(PatternSpecialist::new(provider)),
        }
    }

    fn synthesize_results(&self, results: Vec<SpecialistResult>) -> Result<MultiAgentResult> {
        let mut output = MultiAgentResult::default();
        let mut all_findings: Vec<(AnalysisSpecialty, SpecialistResult)> = Vec::new();

        for result in results {
            if let Some(specialty) = result.specialty {
                output.specialist_confidences.insert(specialty, result.confidence);
                all_findings.push((specialty, result.clone()));

                match specialty {
                    AnalysisSpecialty::Structure => {
                        match serde_json::from_value::<StructureFindings>(result.findings.clone()) {
                            Ok(structure) => {
                                output.structure.entry_points.extend(structure.entry_points);
                                output.structure.core_modules.extend(structure.core_modules);
                                output.structure.layer_boundaries.extend(structure.layers);
                            }
                            Err(e) if !result.findings.is_null() => {
                                tracing::debug!(?specialty, error = %e, "Failed to parse structure findings");
                            }
                            _ => {}
                        }
                    }
                    AnalysisSpecialty::Pattern => {
                        match serde_json::from_value::<Vec<PatternInstance>>(result.findings.clone()) {
                            Ok(patterns) => output.patterns.extend(patterns),
                            Err(e) if !result.findings.is_null() => {
                                tracing::debug!(?specialty, error = %e, "Failed to parse pattern findings");
                            }
                            _ => {}
                        }
                    }
                    AnalysisSpecialty::Constraint => {
                        match serde_json::from_value::<Vec<DiscoveredConstraint>>(result.findings.clone()) {
                            Ok(constraints) => output.constraints.extend(constraints),
                            Err(e) if !result.findings.is_null() => {
                                tracing::debug!(?specialty, error = %e, "Failed to parse constraint findings");
                            }
                            _ => {}
                        }
                    }
                    // Other specialties currently don't have specialized parsing
                    _ => {}
                }

                output.gaps.extend(result.gaps);
            }
        }

        if self.cross_validate {
            let validation_gaps = self.cross_validate_findings(&all_findings, &output);
            output.gaps.extend(validation_gaps);
        }

        Ok(output)
    }

    fn cross_validate_findings(
        &self,
        findings: &[(AnalysisSpecialty, SpecialistResult)],
        output: &MultiAgentResult,
    ) -> Vec<AnalysisGap> {
        let mut gaps = Vec::new();

        let structure_modules: std::collections::HashSet<_> = output.structure.core_modules
            .iter()
            .map(|m| m.name.as_str())
            .collect();

        for pattern in &output.patterns {
            for loc in &pattern.locations {
                let module_from_path = loc.file.split('/').nth(1).unwrap_or("");
                if !module_from_path.is_empty()
                    && !structure_modules.iter().any(|m| loc.file.contains(m))
                {
                    gaps.push(AnalysisGap {
                        area: format!("cross-validation:pattern:{}", pattern.name),
                        description: format!(
                            "Pattern '{}' references file '{}' in module '{}' not identified by structure specialist",
                            pattern.name, loc.file, module_from_path
                        ),
                        severity: GapSeverity::Low,
                    });
                }
            }
        }

        for constraint in &output.constraints {
            for evidence in &constraint.evidence {
                let has_structural_backing = output.structure.core_modules
                    .iter()
                    .any(|m| evidence.file.contains(&m.name));

                if !has_structural_backing && !evidence.file.is_empty() {
                    gaps.push(AnalysisGap {
                        area: format!("cross-validation:constraint:{}", constraint.title),
                        description: format!(
                            "Constraint '{}' has evidence in '{}' which lacks structural backing",
                            constraint.title, evidence.file
                        ),
                        severity: GapSeverity::Medium,
                    });
                }
            }
        }

        let structure_conf = findings.iter()
            .find(|(s, _)| *s == AnalysisSpecialty::Structure)
            .map(|(_, r)| r.confidence)
            .unwrap_or(0.0);

        let pattern_conf = findings.iter()
            .find(|(s, _)| *s == AnalysisSpecialty::Pattern)
            .map(|(_, r)| r.confidence)
            .unwrap_or(0.0);

        if (structure_conf - pattern_conf).abs() > 0.3 {
            gaps.push(AnalysisGap {
                area: "cross-validation:confidence".into(),
                description: format!(
                    "Significant confidence gap between Structure ({:.0}%) and Pattern ({:.0}%) specialists",
                    structure_conf * 100.0, pattern_conf * 100.0
                ),
                severity: GapSeverity::Medium,
            });
        }

        if !gaps.is_empty() {
            tracing::debug!(
                gaps = gaps.len(),
                "Cross-validation identified inter-specialist inconsistencies"
            );
        }

        gaps
    }

    pub fn to_deep_analysis_result(&self, result: MultiAgentResult) -> DeepAnalysisResult {
        use super::deep_analyzer::{AnalysisQuality, StructureAnalysis};

        let avg_confidence = if result.specialist_confidences.is_empty() {
            0.0
        } else {
            result.specialist_confidences.values().sum::<f32>()
                / result.specialist_confidences.len() as f32
        };

        DeepAnalysisResult {
            structure: StructureAnalysis {
                entry_points: result.structure.entry_points,
                core_modules: result.structure.core_modules,
                layer_boundaries: result.structure.layer_boundaries
                    .into_iter()
                    .map(|b| super::deep_analyzer::LayerBoundary {
                        from_layer: b,
                        to_layer: String::new(),
                        allowed: true,
                        evidence: String::new(),
                    })
                    .collect(),
                config_locations: Vec::new(),
            },
            patterns: result.patterns,
            constraints: result.constraints,
            dependencies: Vec::new(),
            insights: Vec::new(),
            key_abstractions: Vec::new(),
            analysis_quality: AnalysisQuality {
                files_analyzed: 0,
                lines_analyzed: 0,
                coverage_ratio: avg_confidence,
                evidence_count: 0,
                validated_refs: 0,
                filtered_hallucinations: 0,
                confidence_score: avg_confidence,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StructureFindings {
    #[serde(default)]
    entry_points: Vec<EntryPoint>,
    #[serde(default)]
    core_modules: Vec<CoreModule>,
    #[serde(default)]
    layers: Vec<String>,
}

struct StructureSpecialist {
    provider: Arc<dyn LlmProvider>,
}

impl StructureSpecialist {
    fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl SpecialistAgent for StructureSpecialist {
    fn specialty(&self) -> AnalysisSpecialty {
        AnalysisSpecialty::Structure
    }

    async fn analyze(&self, context: &AnalysisContext) -> Result<SpecialistResult> {
        let file_summary = context.file_list.iter()
            .take(50)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            r#"Analyze structure of {} project ({}):
Files: {}

Return JSON: {{"entry_points": [{{"path": "...", "kind": "main", "description": "..."}}], "core_modules": [{{"path": "...", "name": "...", "responsibility": "...", "public_items": [], "internal_deps": []}}], "layers": ["..."]}}
Only output valid JSON."#,
            context.project_type,
            context.languages.join(", "),
            file_summary
        );

        let schema = serde_json::json!({"type": "object"});

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => Ok(SpecialistResult {
                specialty: Some(self.specialty()),
                confidence: 0.8,
                findings: response.content,
                gaps: Vec::new(),
            }),
            Err(e) => {
                tracing::warn!(error = %e, "Structure analysis failed");
                Ok(SpecialistResult {
                    specialty: Some(self.specialty()),
                    confidence: 0.0,
                    findings: Value::Null,
                    gaps: vec![AnalysisGap {
                        area: "structure".into(),
                        description: e.to_string(),
                        severity: GapSeverity::High,
                    }],
                })
            }
        }
    }
}

struct PatternSpecialist {
    provider: Arc<dyn LlmProvider>,
}

impl PatternSpecialist {
    fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl SpecialistAgent for PatternSpecialist {
    fn specialty(&self) -> AnalysisSpecialty {
        AnalysisSpecialty::Pattern
    }

    async fn analyze(&self, context: &AnalysisContext) -> Result<SpecialistResult> {
        let code_samples: Vec<_> = context.file_contents
            .iter()
            .take(10)
            .map(|(path, content)| {
                let preview = content.lines().take(50).collect::<Vec<_>>().join("\n");
                format!("=== {} ===\n{}", path, preview)
            })
            .collect();

        let prompt = format!(
            r#"Identify patterns in {} project:
{}

Return JSON array: [{{"name": "...", "category": "architecture", "description": "...", "locations": [{{"file": "...", "line": 1, "snippet": "..."}}], "usage_guidance": "..."}}]
Only output valid JSON."#,
            context.project_type,
            code_samples.join("\n\n")
        );

        let schema = serde_json::json!({"type": "array"});

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => Ok(SpecialistResult {
                specialty: Some(self.specialty()),
                confidence: 0.75,
                findings: response.content,
                gaps: Vec::new(),
            }),
            Err(e) => {
                tracing::warn!(error = %e, "Pattern analysis failed");
                Ok(SpecialistResult::default())
            }
        }
    }
}

struct ConstraintSpecialist {
    provider: Arc<dyn LlmProvider>,
}

impl ConstraintSpecialist {
    fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl SpecialistAgent for ConstraintSpecialist {
    fn specialty(&self) -> AnalysisSpecialty {
        AnalysisSpecialty::Constraint
    }

    async fn analyze(&self, context: &AnalysisContext) -> Result<SpecialistResult> {
        let code_samples: Vec<_> = context.file_contents
            .iter()
            .take(15)
            .map(|(path, content)| {
                let preview = content.lines().take(100).collect::<Vec<_>>().join("\n");
                format!("=== {} ===\n{}", path, preview)
            })
            .collect();

        let prompt = format!(
            r#"Find hidden constraints in {} project:
{}

Return JSON array: [{{"kind": "anti_pattern", "title": "...", "description": "...", "rationale": "...", "evidence": [{{"file": "...", "line": 1, "context": "..."}}], "severity": "high"}}]
Only output valid JSON."#,
            context.project_type,
            code_samples.join("\n\n")
        );

        let schema = serde_json::json!({"type": "array"});

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => Ok(SpecialistResult {
                specialty: Some(self.specialty()),
                confidence: 0.7,
                findings: response.content,
                gaps: Vec::new(),
            }),
            Err(e) => {
                tracing::warn!(error = %e, "Constraint analysis failed");
                Ok(SpecialistResult::default())
            }
        }
    }
}
