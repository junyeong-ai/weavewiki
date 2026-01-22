mod judge;
mod judge_convergence;
mod llm_judge_agent;
mod thinking;

pub use judge::{
    Artifacts, IssueSeverity, JudgeConfig, JudgmentResult, LlmJudge, QualityIssue, Suggestion,
};
pub use judge_convergence::{ConvergenceChecker, ConvergenceReason, ConvergenceResult};
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
        let _checker = ConvergenceChecker::new(0.85, 0.5);
    }
}
