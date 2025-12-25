//! Refinement
//!
//! Quality scoring and documentation generation.
//! Focused on quality assessment of domain insights.

pub mod architecture_docs;
pub mod doc_generator;
pub mod quality_scorer;

use crate::config::{AnalysisMode, ModeConfig, ProjectScale};
use crate::storage::SharedDatabase;
use crate::types::error::WeaveError;
use crate::wiki::exhaustive::checkpoint::CheckpointContext;
use crate::wiki::exhaustive::consolidation::DomainInsight;
use quality_scorer::{CrossRefIssue, QualityScore, QualityScorer};
use std::path::Path;

pub struct RefinementAnalyzer {
    project_root: std::path::PathBuf,
    config: ModeConfig,
    mode: AnalysisMode,
    scale: ProjectScale,
    checkpoint: Option<CheckpointContext>,
}

impl RefinementAnalyzer {
    pub fn new(
        project_root: impl AsRef<Path>,
        config: ModeConfig,
        mode: AnalysisMode,
        scale: ProjectScale,
    ) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            config,
            mode,
            scale,
            checkpoint: None,
        }
    }

    pub fn with_checkpoint(mut self, db: SharedDatabase, session_id: String) -> Self {
        self.checkpoint = Some(CheckpointContext::new(db, session_id));
        self
    }

    /// Run quality assessment on domain insights
    pub async fn run(
        &self,
        domain_insights: Vec<DomainInsight>,
    ) -> Result<RefinementInsight, WeaveError> {
        tracing::info!(
            "Refinement: Starting quality assessment (mode={}, scale={}, target={:.0}%)",
            self.mode,
            self.scale,
            self.config.refinement_quality_target * 100.0
        );

        // Load previous refinement state (for resume)
        let (start_turn, mut quality_scores) = self.load_refinement_state()?;
        if start_turn > 0 {
            tracing::info!(
                "Refinement: Resuming from turn {} with {} previous scores",
                start_turn,
                quality_scores.len()
            );
        }

        let scorer = QualityScorer::new();
        let turn = start_turn + 1;

        // Calculate quality score
        let score = scorer.score(&domain_insights);
        let overall = score.overall();
        quality_scores.push(score.clone());

        // Store checkpoint
        self.store_refinement_turn(turn, &score)?;

        tracing::info!(
            "Refinement: Quality score = {:.1}% (target: {:.1}%)",
            overall * 100.0,
            self.config.refinement_quality_target * 100.0
        );

        let target_met = overall >= self.config.refinement_quality_target;
        if target_met {
            tracing::info!("Refinement: Quality target met!");
        } else {
            tracing::info!(
                "Refinement: Quality target not met, but proceeding with generated documentation"
            );
        }

        // Validate cross-references against file system
        let cross_ref_issues =
            scorer.validate_cross_references(&domain_insights, &self.project_root);
        if !cross_ref_issues.is_empty() {
            tracing::warn!(
                "Refinement: Found {} cross-reference issues",
                cross_ref_issues.len()
            );
        }

        Ok(RefinementInsight {
            domain_insights,
            quality_scores,
            turns_used: turn,
            target_met,
            cross_ref_issues,
        })
    }

    /// Load previous refinement state (for resume)
    fn load_refinement_state(&self) -> Result<(usize, Vec<QualityScore>), WeaveError> {
        let Some(ctx) = &self.checkpoint else {
            return Ok((0, vec![]));
        };

        let conn = ctx.db.connection()?;
        let mut stmt = conn.prepare(
            "SELECT refinement_turn, quality_scores_history FROM doc_sessions WHERE id = ?1",
        )?;

        let result = stmt
            .query_row([&ctx.session_id], |row| {
                let turn: i32 = row.get(0)?;
                let history_json: Option<String> = row.get(1)?;
                Ok((turn as usize, history_json))
            })
            .ok();

        match result {
            Some((turn, Some(history_json))) => {
                let scores: Vec<QualityScore> =
                    serde_json::from_str(&history_json).unwrap_or_default();
                Ok((turn, scores))
            }
            _ => Ok((0, vec![])),
        }
    }

    /// Store refinement turn checkpoint
    fn store_refinement_turn(&self, turn: usize, score: &QualityScore) -> Result<(), WeaveError> {
        let Some(ctx) = &self.checkpoint else {
            return Ok(());
        };

        let now = chrono::Utc::now().to_rfc3339();

        ctx.db.execute(
            "UPDATE doc_sessions
             SET refinement_turn = ?1, quality_score = ?2, updated_at = ?3
             WHERE id = ?4",
            &[
                &(turn as i64),
                &(score.overall() as f64),
                &now,
                &ctx.session_id,
            ],
        )?;

        tracing::debug!(
            "Refinement: Checkpointed turn {} (score: {:.1}%)",
            turn,
            score.overall() * 100.0
        );
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct RefinementInsight {
    pub domain_insights: Vec<DomainInsight>,
    pub quality_scores: Vec<QualityScore>,
    pub turns_used: usize,
    pub target_met: bool,
    pub cross_ref_issues: Vec<CrossRefIssue>,
}
