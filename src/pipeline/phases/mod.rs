//! Pipeline Phases - Adaptive Generation Pipeline
//!
//! Phase 1: Project Detection - Automatic project type identification
//! Phase 2: Monorepo Analysis - Workspace structure analysis
//! Phase 3: Convention Inference - Few-shot based pattern discovery
//! Phase 4: Constraint Extraction - Hidden dependencies and anti-patterns
//! Phase 5: Output Planning - Determine generation strategy
//! Phase 6: Draft Generation - Create CLAUDE.md, Skills, Agents, Rules
//! Phase 7: Quality-Based Refinement - Iterative quality improvement
//! Phase 8: Final Validation - Tier filtering and consistency checking

use serde::{Deserialize, Serialize};

pub mod constraint_extraction;
pub mod convention_inference;
pub mod few_shot;
pub mod monorepo_analyzer;
pub mod output_router;
pub mod project_detection;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum OutputStrategy {
    Unified,
    SplitByProject,
    SplitByLanguage,
    Hierarchical,
}

impl OutputStrategy {
    pub fn is_split(&self) -> bool {
        !matches!(self, Self::Unified)
    }

    pub fn requires_path_rules(&self) -> bool {
        !matches!(self, Self::Unified)
    }

    pub fn requires_subproject_agents(&self) -> bool {
        matches!(self, Self::SplitByProject | Self::Hierarchical)
    }
}

pub use constraint_extraction::{
    AntiPattern, ComplexWorkflow, ExtractedConstraints, Gotcha, HiddenDependency, Severity,
};
pub use convention_inference::{
    ArchitectureConvention, AsyncPattern, ErrorHandlingPattern, InferredConventions,
    NamingConventions,
};
pub use few_shot::{FewShotExample, get_examples, get_claude_md_example, get_skill_example, get_rule_example};
pub use monorepo_analyzer::{
    CrossDependency, MonorepoAnalysis, SharedPackage, SubprojectInfo,
};
pub use output_router::{AgentsPlan, ClaudeMdPlan, OutputPlan, RulesPlan, SkillsPlan};
pub use project_detection::{
    DetectionSignal, LanguageInfo, ProjectDetection, ProjectDetector, SignalType, WorkspaceConfig,
    WorkspaceMember, WorkspaceType,
};
