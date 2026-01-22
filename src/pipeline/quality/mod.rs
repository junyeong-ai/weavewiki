mod convergence;
mod judge;
mod llm_judge_agent;
mod thinking;

pub use convergence::{ConvergenceChecker, ConvergenceReason, ConvergenceResult};
pub use judge::{
    Artifacts, IssueSeverity, JudgeConfig, JudgmentResult, LlmJudge, QualityIssue, Suggestion,
};
pub use thinking::{Criterion, Issue, ThinkingFramework};

pub use llm_judge_agent::{
    AgentJudgmentResult, AgentQualityIssue, Artifact, ArtifactType, BatchJudgmentResult,
    EvidenceValidity, FileReference, FileValidationResult, ImprovementPriority,
    ImprovementSuggestion, JudgeContext, LlmJudgeAgent, LlmJudgeAgentConfig,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_exports() {
        // Verify that key types are accessible
        let _checker = ConvergenceChecker::new(0.85, 0.5);
    }
}
