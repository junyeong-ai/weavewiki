use std::collections::HashSet;

use crate::pipeline::analysis::StructuralValidationResult;
use crate::pipeline::feedback::AggregatedFeedback;
use crate::pipeline::quality::JudgmentResult;
use crate::pipeline::quality_assessment::QualityAssessment;
use crate::pipeline::strategy::{IssueKind as StrategyIssueKind, StrategyIssue};
use crate::types::DiagnosticLevel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemType {
    Skill,
    Agent,
    Rule,
}

impl std::fmt::Display for ItemType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Skill => write!(f, "skill"),
            Self::Agent => write!(f, "agent"),
            Self::Rule => write!(f, "rule"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RefinementResult {
    pub skills: Vec<crate::types::Skill>,
    pub agents: Vec<crate::types::Agent>,
    pub rules: Vec<crate::types::Rule>,
    pub iterations: usize,
    pub converged: bool,
    pub final_quality: f32,
    pub judgment: Option<JudgmentResult>,
    pub structural_quality: Option<StructuralValidationResult>,
    pub aggregated_feedback: Option<AggregatedFeedback>,
    pub convergence_report: Option<QualityAssessment>,
    /// Names of artifacts modified during refinement (for selective re-linking).
    pub dirty_artifacts: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct DetectedArtifactIssue {
    pub item_type: ItemType,
    pub item_name: String,
    pub issue: DetectedIssue,
    pub severity: DiagnosticLevel,
}

#[derive(Debug, Clone)]
pub enum DetectedIssue {
    TooShort {
        actual: usize,
        min: usize,
    },
    MissingReferences {
        expected: usize,
        actual: usize,
    },
    MissingSections {
        expected: usize,
        actual: usize,
    },
    PlanMismatch,
    LowActionability {
        score: f32,
        threshold: f32,
    },
    TooGeneric {
        description: String,
    },
    WeakEvidence {
        description: String,
    },
    LowVerificationRatio {
        ratio: f32,
        threshold: f32,
    },
    Redundant {
        description: String,
    },
    Shallow {
        description: String,
    },
    Tier1Content {
        description: String,
    },
    MissingModule {
        module_name: String,
        file_count: usize,
        key_files: Vec<String>,
    },
    PartialModuleCoverage {
        module_name: String,
        reference_count: usize,
    },
    Other {
        kind: String,
        description: String,
    },
}

impl DetectedIssue {
    pub fn to_strategy_issue(&self) -> StrategyIssue {
        let kind = StrategyIssueKind::from(self);
        let (severity, message) = match self {
            Self::LowActionability { score, threshold } => (
                DiagnosticLevel::Warning,
                format!(
                    "Low actionability ({:.0}% vs {:.0}% target)",
                    score * 100.0,
                    threshold * 100.0
                ),
            ),
            Self::TooGeneric { description } => (
                DiagnosticLevel::Warning,
                format!("Too generic: {description}"),
            ),
            Self::WeakEvidence { description } => (
                DiagnosticLevel::Warning,
                format!("Weak evidence: {description}"),
            ),
            Self::LowVerificationRatio { ratio, threshold } => (
                DiagnosticLevel::Warning,
                format!(
                    "Low verification ratio ({:.0}% vs {:.0}% target)",
                    ratio * 100.0,
                    threshold * 100.0
                ),
            ),
            Self::Shallow { description } => (
                DiagnosticLevel::Warning,
                format!("Shallow coverage: {description}"),
            ),
            Self::MissingReferences { expected, actual } => (
                DiagnosticLevel::Error,
                format!("Missing references: {actual} of {expected} required"),
            ),
            Self::TooShort { actual, min } => (
                DiagnosticLevel::Error,
                format!("Too short: {actual} chars (min: {min})"),
            ),
            Self::MissingSections { expected, actual } => (
                DiagnosticLevel::Error,
                format!("Missing sections: {actual} of {expected} required"),
            ),
            Self::Redundant { description } => {
                (DiagnosticLevel::Info, format!("Redundant: {description}"))
            }
            Self::Tier1Content { description } => (
                DiagnosticLevel::Error,
                format!("Tier 1 generic content: {description}"),
            ),
            Self::PlanMismatch => (
                DiagnosticLevel::Warning,
                "Item missing from output plan".to_string(),
            ),
            Self::MissingModule {
                module_name,
                file_count,
                key_files,
            } => (
                DiagnosticLevel::Error,
                format!(
                    "Missing module: '{module_name}' ({file_count} files) - key: {}",
                    key_files.join(", ")
                ),
            ),
            Self::PartialModuleCoverage {
                module_name,
                reference_count,
            } => (
                DiagnosticLevel::Warning,
                format!("Partial coverage: '{module_name}' has {reference_count} reference(s)"),
            ),
            Self::Other { kind, description } => {
                (DiagnosticLevel::Warning, format!("{kind}: {description}"))
            }
        };
        StrategyIssue::new(kind, severity, message)
    }
}
