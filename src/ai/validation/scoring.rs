//! Confidence Scoring with Automatic Penalties
//!
//! Calculates quality confidence scores based on:
//! - Field completeness
//! - Evidence quality
//! - Content depth
//! - Structural integrity
//!
//! Penalties are automatically applied for missing or weak indicators.

use serde_json::Value;

/// Configuration for confidence scoring
#[derive(Debug, Clone)]
pub struct ScoringConfig {
    /// Base confidence score (before penalties)
    pub base_score: f32,
    /// Penalty for missing purpose_summary
    pub missing_purpose_penalty: f32,
    /// Penalty for empty sections
    pub empty_sections_penalty: f32,
    /// Penalty for section without evidence_lines
    pub missing_evidence_penalty: f32,
    /// Penalty for missing hidden_assumptions (v2.0 field)
    pub missing_assumptions_penalty: f32,
    /// Penalty for missing modification_risks (v2.0 field)
    pub missing_risks_penalty: f32,
    /// Penalty for missing key_insights
    pub missing_insights_penalty: f32,
    /// Bonus for high-quality evidence
    pub evidence_bonus: f32,
    /// Minimum allowed confidence
    pub min_confidence: f32,
    /// Maximum allowed confidence
    pub max_confidence: f32,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            base_score: 1.0,
            missing_purpose_penalty: 0.10,
            empty_sections_penalty: 0.15,
            missing_evidence_penalty: 0.08,
            missing_assumptions_penalty: 0.05,
            missing_risks_penalty: 0.05,
            missing_insights_penalty: 0.05,
            evidence_bonus: 0.05,
            min_confidence: 0.3,
            max_confidence: 1.0,
        }
    }
}

/// Quality metrics calculated from analysis
#[derive(Debug, Clone)]
pub struct QualityMetrics {
    /// Overall confidence score (0.0 - 1.0)
    pub overall_confidence: f32,
    /// Per-file confidence scores
    pub file_scores: Vec<FileScore>,
    /// Total penalties applied
    pub total_penalties: f32,
    /// Total bonuses applied
    pub total_bonuses: f32,
    /// Quality warnings
    pub warnings: Vec<String>,
    /// Summary statistics
    pub stats: QualityStats,
}

/// Per-file quality score
#[derive(Debug, Clone)]
pub struct FileScore {
    pub path: String,
    pub confidence: f32,
    pub penalties: Vec<String>,
    pub bonuses: Vec<String>,
}

/// Summary statistics
#[derive(Debug, Clone, Default)]
pub struct QualityStats {
    pub total_files: usize,
    pub total_sections: usize,
    pub sections_with_evidence: usize,
    pub files_with_purpose: usize,
    pub files_with_insights: usize,
    pub files_with_assumptions: usize,
    pub files_with_risks: usize,
    pub avg_sections_per_file: f32,
    pub avg_evidence_lines: f32,
}

/// Confidence scorer with penalty-based calculation
pub struct ConfidenceScorer {
    config: ScoringConfig,
}

impl Default for ConfidenceScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfidenceScorer {
    pub fn new() -> Self {
        Self {
            config: ScoringConfig::default(),
        }
    }

    pub fn with_config(config: ScoringConfig) -> Self {
        Self { config }
    }

    /// Calculate quality metrics for a batch response
    pub fn calculate_quality(&self, response: &Value) -> QualityMetrics {
        let mut metrics = QualityMetrics {
            overall_confidence: self.config.base_score,
            file_scores: Vec::new(),
            total_penalties: 0.0,
            total_bonuses: 0.0,
            warnings: Vec::new(),
            stats: QualityStats::default(),
        };

        let files = match response.get("files").and_then(|v| v.as_array()) {
            Some(f) => f,
            None => {
                metrics.overall_confidence = self.config.min_confidence;
                metrics
                    .warnings
                    .push("No files array in response".to_string());
                return metrics;
            }
        };

        if files.is_empty() {
            metrics.overall_confidence = self.config.min_confidence;
            metrics.warnings.push("Empty files array".to_string());
            return metrics;
        }

        metrics.stats.total_files = files.len();

        // Score each file
        for file in files {
            let score = self.score_file(file);
            metrics.total_penalties += score.penalties.len() as f32 * 0.05;
            metrics.total_bonuses += score.bonuses.len() as f32 * 0.02;
            metrics.file_scores.push(score);
        }

        // Collect statistics
        self.collect_stats(files, &mut metrics.stats);

        // Calculate aggregate warnings
        self.generate_warnings(&metrics.stats, &mut metrics.warnings);

        // Calculate overall confidence
        if !metrics.file_scores.is_empty() {
            let sum: f32 = metrics.file_scores.iter().map(|f| f.confidence).sum();
            metrics.overall_confidence = sum / metrics.file_scores.len() as f32;
        }

        // Clamp to valid range
        metrics.overall_confidence = metrics
            .overall_confidence
            .clamp(self.config.min_confidence, self.config.max_confidence);

        metrics
    }

    /// Score a single file analysis
    fn score_file(&self, file: &Value) -> FileScore {
        let path = file
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let mut confidence = self.config.base_score;
        let mut penalties = Vec::new();
        let mut bonuses = Vec::new();

        // Check purpose_summary
        if file
            .get("purpose_summary")
            .and_then(|v| v.as_str())
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            confidence -= self.config.missing_purpose_penalty;
            penalties.push("missing_purpose".to_string());
        }

        // Check sections
        let sections = file.get("sections").and_then(|v| v.as_array());
        match sections {
            Some(secs) if secs.is_empty() => {
                confidence -= self.config.empty_sections_penalty;
                penalties.push("no_sections".to_string());
            }
            Some(secs) => {
                // Check evidence in sections
                let mut sections_without_evidence = 0;
                let mut total_evidence_lines = 0;

                for section in secs {
                    let evidence = section.get("evidence_lines").and_then(|v| v.as_array());
                    match evidence {
                        Some(lines) if lines.is_empty() => {
                            sections_without_evidence += 1;
                        }
                        Some(lines) => {
                            total_evidence_lines += lines.len();
                        }
                        None => {
                            sections_without_evidence += 1;
                        }
                    }
                }

                if sections_without_evidence > 0 {
                    let penalty = self.config.missing_evidence_penalty
                        * (sections_without_evidence as f32 / secs.len() as f32);
                    confidence -= penalty;
                    penalties.push(format!(
                        "{}_sections_without_evidence",
                        sections_without_evidence
                    ));
                }

                // Bonus for rich evidence
                if total_evidence_lines > secs.len() * 3 {
                    confidence += self.config.evidence_bonus;
                    bonuses.push("rich_evidence".to_string());
                }
            }
            None => {
                confidence -= self.config.empty_sections_penalty;
                penalties.push("missing_sections".to_string());
            }
        }

        // Check key_insights
        if file
            .get("key_insights")
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true)
        {
            confidence -= self.config.missing_insights_penalty;
            penalties.push("no_insights".to_string());
        }

        // Check hidden_assumptions (v2.0)
        if file
            .get("hidden_assumptions")
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true)
        {
            confidence -= self.config.missing_assumptions_penalty;
            penalties.push("no_hidden_assumptions".to_string());
        }

        // Check modification_risks (v2.0)
        if file
            .get("modification_risks")
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true)
        {
            confidence -= self.config.missing_risks_penalty;
            penalties.push("no_modification_risks".to_string());
        }

        // Bonus for LLM-provided confidence if high
        if let Some(llm_conf) = file.get("confidence").and_then(|v| v.as_f64()) {
            if llm_conf >= 0.9 {
                bonuses.push("high_llm_confidence".to_string());
            } else if llm_conf < 0.7 {
                confidence -= 0.05;
                penalties.push("low_llm_confidence".to_string());
            }
        }

        // Clamp
        confidence = confidence.clamp(self.config.min_confidence, self.config.max_confidence);

        FileScore {
            path,
            confidence,
            penalties,
            bonuses,
        }
    }

    /// Collect statistics from files
    fn collect_stats(&self, files: &[Value], stats: &mut QualityStats) {
        let mut total_sections = 0;
        let mut sections_with_evidence = 0;
        let mut total_evidence_lines = 0;

        for file in files {
            // Purpose
            if file
                .get("purpose_summary")
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false)
            {
                stats.files_with_purpose += 1;
            }

            // Insights
            if file
                .get("key_insights")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false)
            {
                stats.files_with_insights += 1;
            }

            // Assumptions
            if file
                .get("hidden_assumptions")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false)
            {
                stats.files_with_assumptions += 1;
            }

            // Risks
            if file
                .get("modification_risks")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false)
            {
                stats.files_with_risks += 1;
            }

            // Sections
            if let Some(secs) = file.get("sections").and_then(|v| v.as_array()) {
                total_sections += secs.len();

                for section in secs {
                    if let Some(evidence) = section.get("evidence_lines").and_then(|v| v.as_array())
                        && !evidence.is_empty()
                    {
                        sections_with_evidence += 1;
                        total_evidence_lines += evidence.len();
                    }
                }
            }
        }

        stats.total_sections = total_sections;
        stats.sections_with_evidence = sections_with_evidence;

        if !files.is_empty() {
            stats.avg_sections_per_file = total_sections as f32 / files.len() as f32;
        }

        if sections_with_evidence > 0 {
            stats.avg_evidence_lines = total_evidence_lines as f32 / sections_with_evidence as f32;
        }
    }

    /// Generate quality warnings
    fn generate_warnings(&self, stats: &QualityStats, warnings: &mut Vec<String>) {
        if stats.total_files == 0 {
            return;
        }

        let purpose_rate = stats.files_with_purpose as f32 / stats.total_files as f32;
        if purpose_rate < 0.8 {
            warnings.push(format!(
                "Only {:.0}% of files have purpose summaries",
                purpose_rate * 100.0
            ));
        }

        if stats.total_sections > 0 {
            let evidence_rate = stats.sections_with_evidence as f32 / stats.total_sections as f32;
            if evidence_rate < 0.7 {
                warnings.push(format!(
                    "Only {:.0}% of sections have evidence lines",
                    evidence_rate * 100.0
                ));
            }
        }

        let insights_rate = stats.files_with_insights as f32 / stats.total_files as f32;
        if insights_rate < 0.5 {
            warnings.push(format!(
                "Only {:.0}% of files have key insights",
                insights_rate * 100.0
            ));
        }

        // v2.0 quality indicators
        let assumptions_rate = stats.files_with_assumptions as f32 / stats.total_files as f32;
        if assumptions_rate < 0.3 {
            warnings.push(format!(
                "Only {:.0}% of files identify hidden assumptions",
                assumptions_rate * 100.0
            ));
        }

        let risks_rate = stats.files_with_risks as f32 / stats.total_files as f32;
        if risks_rate < 0.3 {
            warnings.push(format!(
                "Only {:.0}% of files identify modification risks",
                risks_rate * 100.0
            ));
        }

        if stats.avg_sections_per_file < 1.5 {
            warnings.push(format!(
                "Low section density: {:.1} sections per file",
                stats.avg_sections_per_file
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_high_quality_response() {
        let scorer = ConfidenceScorer::new();

        let response = json!({
            "files": [{
                "path": "src/main.rs",
                "purpose_summary": "Application entry point",
                "confidence": 0.95,
                "sections": [{
                    "section_name": "Main",
                    "evidence_lines": [1, 5, 10, 15]
                }],
                "key_insights": ["Handles CLI"],
                "hidden_assumptions": ["Config exists"],
                "modification_risks": ["CLI breaking"]
            }]
        });

        let metrics = scorer.calculate_quality(&response);
        assert!(
            metrics.overall_confidence > 0.8,
            "Expected confidence > 0.8, got {}",
            metrics.overall_confidence
        );
        // High quality responses may still have informational warnings (e.g., single file stats)
        assert!(
            metrics.file_scores[0].penalties.is_empty(),
            "Expected no penalties: {:?}",
            metrics.file_scores[0].penalties
        );
    }

    #[test]
    fn test_low_quality_response() {
        let scorer = ConfidenceScorer::new();

        let response = json!({
            "files": [{
                "path": "src/main.rs",
                "sections": []
            }]
        });

        let metrics = scorer.calculate_quality(&response);
        assert!(metrics.overall_confidence < 0.8);
        assert!(!metrics.file_scores[0].penalties.is_empty());
    }

    #[test]
    fn test_missing_evidence_penalty() {
        let scorer = ConfidenceScorer::new();

        let response = json!({
            "files": [{
                "path": "test.rs",
                "purpose_summary": "Test",
                "sections": [
                    {"section_name": "A", "evidence_lines": []},
                    {"section_name": "B", "evidence_lines": []}
                ],
                "key_insights": ["Test"]
            }]
        });

        let metrics = scorer.calculate_quality(&response);
        let file_score = &metrics.file_scores[0];
        assert!(
            file_score
                .penalties
                .iter()
                .any(|p| p.contains("without_evidence"))
        );
    }

    #[test]
    fn test_stats_calculation() {
        let scorer = ConfidenceScorer::new();

        let response = json!({
            "files": [
                {
                    "path": "a.rs",
                    "purpose_summary": "File A",
                    "sections": [{"section_name": "S1", "evidence_lines": [1, 2]}],
                    "key_insights": ["Insight"]
                },
                {
                    "path": "b.rs",
                    "sections": [{"section_name": "S2", "evidence_lines": []}]
                }
            ]
        });

        let metrics = scorer.calculate_quality(&response);
        assert_eq!(metrics.stats.total_files, 2);
        assert_eq!(metrics.stats.files_with_purpose, 1);
        assert_eq!(metrics.stats.sections_with_evidence, 1);
    }
}
