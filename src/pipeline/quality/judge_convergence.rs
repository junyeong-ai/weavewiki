use super::judge::JudgmentResult;

#[derive(Debug, Clone)]
pub struct ConvergenceResult {
    pub should_stop: bool,
    pub converged: bool,
    pub reason: ConvergenceReason,
}

#[derive(Debug, Clone)]
pub enum ConvergenceReason {
    TargetReached { score: f32 },
    BelowMinimum { score: f32 },
    Plateau { iterations: usize },
    CriticalIssues { count: usize },
    MaxIterations { count: usize },
    Continuing,
}

impl std::fmt::Display for ConvergenceReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TargetReached { score } => write!(f, "Target reached: {:.2}", score),
            Self::BelowMinimum { score } => write!(f, "Below minimum: {:.2}", score),
            Self::Plateau { iterations } => write!(f, "Plateau after {} iterations", iterations),
            Self::CriticalIssues { count } => write!(f, "{} critical issues remain", count),
            Self::MaxIterations { count } => write!(f, "Max iterations reached: {}", count),
            Self::Continuing => write!(f, "Continuing"),
        }
    }
}

pub struct ConvergenceChecker {
    target: f32,
    minimum: f32,
    plateau_threshold: usize,
    history: Vec<f32>,
}

impl ConvergenceChecker {
    pub fn new(target: f32, minimum: f32) -> Self {
        Self {
            target,
            minimum,
            plateau_threshold: 3,
            history: Vec::new(),
        }
    }

    pub fn with_plateau_threshold(mut self, threshold: usize) -> Self {
        self.plateau_threshold = threshold;
        self
    }

    pub fn check(&mut self, judgment: &JudgmentResult, max_iterations: usize) -> ConvergenceResult {
        let score = judgment.overall_score;
        self.history.push(score);

        if score >= self.target {
            return ConvergenceResult {
                should_stop: true,
                converged: true,
                reason: ConvergenceReason::TargetReached { score },
            };
        }

        if self.history.len() >= max_iterations {
            return ConvergenceResult {
                should_stop: true,
                converged: score >= self.minimum,
                reason: ConvergenceReason::MaxIterations { count: self.history.len() },
            };
        }

        let critical_count = judgment
            .issues
            .iter()
            .filter(|i| matches!(i.severity, super::judge::IssueSeverity::Critical))
            .count();

        if critical_count > 0 && self.history.len() > 5 {
            return ConvergenceResult {
                should_stop: true,
                converged: false,
                reason: ConvergenceReason::CriticalIssues { count: critical_count },
            };
        }

        if self.is_plateau() {
            return ConvergenceResult {
                should_stop: true,
                converged: score >= self.minimum,
                reason: ConvergenceReason::Plateau {
                    iterations: self.plateau_threshold,
                },
            };
        }

        if score < self.minimum && self.history.len() > 3 && !self.is_improving() {
            return ConvergenceResult {
                should_stop: true,
                converged: false,
                reason: ConvergenceReason::BelowMinimum { score },
            };
        }

        ConvergenceResult {
            should_stop: false,
            converged: false,
            reason: ConvergenceReason::Continuing,
        }
    }

    fn is_plateau(&self) -> bool {
        if self.history.len() < self.plateau_threshold {
            return false;
        }

        let recent: Vec<f32> = self.history.iter().rev().take(self.plateau_threshold).copied().collect();
        let max = recent.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let min = recent.iter().copied().fold(f32::INFINITY, f32::min);

        (max - min).abs() < 0.01
    }

    fn is_improving(&self) -> bool {
        if self.history.len() < 2 {
            return true;
        }
        self.history[self.history.len() - 1] > self.history[self.history.len() - 2]
    }

    pub fn reset(&mut self) {
        self.history.clear();
    }

    pub fn iteration_count(&self) -> usize {
        self.history.len()
    }

    pub fn score_history(&self) -> &[f32] {
        &self.history
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::quality::judge::QualityIssue;
    use crate::types::ContentTier;

    fn mock_judgment(score: f32) -> JudgmentResult {
        JudgmentResult {
            overall_score: score,
            tier: ContentTier::Tier2Convention,
            issues: vec![],
            suggestions: vec![],
        }
    }

    fn mock_judgment_with_critical(score: f32, critical_count: usize) -> JudgmentResult {
        let issues = (0..critical_count)
            .map(|i| QualityIssue {
                code: format!("CRIT-{}", i),
                message: "Critical issue".to_string(),
                severity: super::super::judge::IssueSeverity::Critical,
                evidence: vec![],
            })
            .collect();

        JudgmentResult {
            overall_score: score,
            tier: ContentTier::Tier2Convention,
            issues,
            suggestions: vec![],
        }
    }

    #[test]
    fn test_target_reached() {
        let mut checker = ConvergenceChecker::new(0.85, 0.5);
        let result = checker.check(&mock_judgment(0.90), 10);
        assert!(result.should_stop);
        assert!(result.converged);
    }

    #[test]
    fn test_plateau() {
        let mut checker = ConvergenceChecker::new(0.85, 0.5).with_plateau_threshold(3);

        checker.check(&mock_judgment(0.70), 10);
        checker.check(&mock_judgment(0.70), 10);
        let result = checker.check(&mock_judgment(0.70), 10);

        assert!(result.should_stop);
        assert!(result.converged);
    }

    #[test]
    fn test_max_iterations() {
        let mut checker = ConvergenceChecker::new(0.85, 0.5);

        for _ in 0..9 {
            checker.check(&mock_judgment(0.60), 10);
        }
        let result = checker.check(&mock_judgment(0.60), 10);

        assert!(result.should_stop);
        assert!(result.converged);
        assert!(matches!(result.reason, ConvergenceReason::MaxIterations { .. }));
    }

    #[test]
    fn test_critical_issues() {
        let mut checker = ConvergenceChecker::new(0.85, 0.5);

        for _ in 0..6 {
            checker.check(&mock_judgment_with_critical(0.60, 2), 20);
        }

        assert!(checker.iteration_count() == 6);
    }
}
