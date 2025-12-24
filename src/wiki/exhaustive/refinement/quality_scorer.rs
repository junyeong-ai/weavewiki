//! Quality Scoring for Content-First Documentation
//!
//! Multi-dimensional quality assessment based on:
//! - content_coverage: % of domains with meaningful content
//! - diagram_coverage: % of domains with diagrams
//! - relationships: % of domains with cross-references
//! - purpose_clarity: quality of purpose statements
//! - completeness: depth of documentation
//!
//! Also provides:
//! - Per-tier quality breakdown
//! - Actionable recommendations
//! - Improvement targeting

use crate::wiki::exhaustive::bottom_up::{FileInsight, ProcessingTier};
use crate::wiki::exhaustive::consolidation::DomainInsight;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Quality score dimensions for content-first documentation
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QualityScore {
    /// % of domains with meaningful content (>100 words)
    pub content_coverage: f32,
    /// % of domains with diagrams
    pub diagram_coverage: f32,
    /// % of domains with cross-references
    pub relationships: f32,
}

impl QualityScore {
    /// Calculate weighted overall score
    pub fn overall(&self) -> f32 {
        self.content_coverage * 0.50 + self.diagram_coverage * 0.30 + self.relationships * 0.20
    }

    /// Get categories below threshold
    pub fn gaps(&self, threshold: f32) -> Vec<String> {
        let mut gaps = vec![];

        if self.content_coverage < threshold {
            gaps.push("content_coverage".to_string());
        }
        if self.diagram_coverage < threshold {
            gaps.push("diagram_coverage".to_string());
        }
        if self.relationships < threshold {
            gaps.push("relationships".to_string());
        }

        gaps
    }
}

// =============================================================================
// Enhanced Multi-Dimensional Quality Metrics
// =============================================================================

/// Comprehensive quality metrics with multiple dimensions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityMetrics {
    /// Overall weighted score (0.0 - 1.0)
    pub overall: f64,

    /// Individual dimension scores
    pub dimensions: QualityDimensions,

    /// Per-tier quality breakdown
    pub by_tier: HashMap<String, TierQuality>,

    /// Actionable recommendations
    pub recommendations: Vec<QualityRecommendation>,

    /// Timestamp of calculation
    pub calculated_at: String,
}

impl Default for QualityMetrics {
    fn default() -> Self {
        Self {
            overall: 0.0,
            dimensions: QualityDimensions::default(),
            by_tier: HashMap::new(),
            recommendations: Vec::new(),
            calculated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Individual quality dimensions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QualityDimensions {
    /// Code coverage: % of files documented
    pub coverage: f64,

    /// Documentation completeness: avg content richness
    pub completeness: f64,

    /// Cross-reference accuracy: valid links
    pub accuracy: f64,

    /// Diagram quality: valid Mermaid diagrams
    pub diagrams: f64,

    /// Purpose clarity: clear purpose statements
    pub clarity: f64,
}

impl QualityDimensions {
    /// Calculate weighted average
    pub fn weighted_average(&self) -> f64 {
        // Weights: coverage (25%), completeness (30%), accuracy (15%), diagrams (15%), clarity (15%)
        self.coverage * 0.25
            + self.completeness * 0.30
            + self.accuracy * 0.15
            + self.diagrams * 0.15
            + self.clarity * 0.15
    }

    /// Get dimensions below threshold
    pub fn weak_dimensions(&self, threshold: f64) -> Vec<(&'static str, f64)> {
        let mut weak = Vec::new();

        if self.coverage < threshold {
            weak.push(("coverage", self.coverage));
        }
        if self.completeness < threshold {
            weak.push(("completeness", self.completeness));
        }
        if self.accuracy < threshold {
            weak.push(("accuracy", self.accuracy));
        }
        if self.diagrams < threshold {
            weak.push(("diagrams", self.diagrams));
        }
        if self.clarity < threshold {
            weak.push(("clarity", self.clarity));
        }

        weak.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        weak
    }
}

/// Per-tier quality metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierQuality {
    pub tier: String,
    pub file_count: usize,
    pub avg_content_length: usize,
    pub diagram_rate: f64,
    pub purpose_quality: f64,
    pub overall: f64,
}

/// Actionable quality recommendation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityRecommendation {
    pub dimension: String,
    pub priority: RecommendationPriority,
    pub current: f64,
    pub target: f64,
    pub action: String,
    pub affected_files: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RecommendationPriority {
    Critical,
    High,
    Medium,
    Low,
}

impl std::fmt::Display for RecommendationPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecommendationPriority::Critical => write!(f, "CRITICAL"),
            RecommendationPriority::High => write!(f, "HIGH"),
            RecommendationPriority::Medium => write!(f, "MEDIUM"),
            RecommendationPriority::Low => write!(f, "LOW"),
        }
    }
}

impl QualityMetrics {
    /// Calculate comprehensive quality metrics from file insights
    pub fn from_insights(insights: &[FileInsight], total_files: usize) -> Self {
        if insights.is_empty() {
            return Self::default();
        }

        let dimensions = Self::calculate_dimensions(insights, total_files);
        let by_tier = Self::calculate_tier_breakdown(insights);
        let recommendations = Self::generate_recommendations(&dimensions, insights);

        Self {
            overall: dimensions.weighted_average(),
            dimensions,
            by_tier,
            recommendations,
            calculated_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Calculate dimension scores
    fn calculate_dimensions(insights: &[FileInsight], total_files: usize) -> QualityDimensions {
        let documented = insights.len();

        // Coverage: % of files documented
        let coverage = if total_files > 0 {
            documented as f64 / total_files as f64
        } else {
            0.0
        };

        // Completeness: based on content length and sections
        let completeness = if documented > 0 {
            let avg_tokens: f64 =
                insights.iter().map(|i| i.token_count as f64).sum::<f64>() / documented as f64;
            // Normalize: 500 tokens = 0.5, 1000+ tokens = 1.0
            (avg_tokens / 1000.0).min(1.0)
        } else {
            0.0
        };

        // Accuracy: based on related_files presence
        let accuracy = if documented > 0 {
            let with_refs = insights
                .iter()
                .filter(|i| !i.related_files.is_empty())
                .count();
            with_refs as f64 / documented as f64
        } else {
            0.0
        };

        // Diagrams: % with valid diagrams
        let diagrams = if documented > 0 {
            let with_diagrams = insights.iter().filter(|i| i.has_diagram()).count();
            with_diagrams as f64 / documented as f64
        } else {
            0.0
        };

        // Clarity: based on purpose length and quality
        let clarity = if documented > 0 {
            let clarity_scores: f64 = insights
                .iter()
                .map(|i| Self::score_purpose_clarity(&i.purpose))
                .sum();
            clarity_scores / documented as f64
        } else {
            0.0
        };

        QualityDimensions {
            coverage,
            completeness,
            accuracy,
            diagrams,
            clarity,
        }
    }

    /// Score purpose statement clarity (0.0 - 1.0)
    fn score_purpose_clarity(purpose: &str) -> f64 {
        if purpose.is_empty() {
            return 0.0;
        }

        let words = purpose.split_whitespace().count();
        let has_verb = purpose.contains("is ")
            || purpose.contains("are ")
            || purpose.contains("handles")
            || purpose.contains("provides")
            || purpose.contains("implements")
            || purpose.contains("manages");

        let length_score = if (5..=30).contains(&words) {
            1.0
        } else if words > 30 {
            0.7 // Too long
        } else {
            words as f64 / 5.0 // Too short
        };

        let verb_bonus = if has_verb { 0.2 } else { 0.0 };

        (length_score * 0.8 + verb_bonus).min(1.0)
    }

    /// Calculate per-tier quality breakdown
    fn calculate_tier_breakdown(insights: &[FileInsight]) -> HashMap<String, TierQuality> {
        let mut tier_groups: HashMap<ProcessingTier, Vec<&FileInsight>> = HashMap::new();

        for insight in insights {
            tier_groups.entry(insight.tier).or_default().push(insight);
        }

        tier_groups
            .into_iter()
            .map(|(tier, files)| {
                let file_count = files.len();
                let avg_content_length = if file_count > 0 {
                    files.iter().map(|f| f.content.len()).sum::<usize>() / file_count
                } else {
                    0
                };
                let diagram_rate = if file_count > 0 {
                    files.iter().filter(|f| f.has_diagram()).count() as f64 / file_count as f64
                } else {
                    0.0
                };
                let purpose_quality = if file_count > 0 {
                    files
                        .iter()
                        .map(|f| Self::score_purpose_clarity(&f.purpose))
                        .sum::<f64>()
                        / file_count as f64
                } else {
                    0.0
                };

                // Overall tier quality
                let overall = (avg_content_length as f64 / 2000.0).min(1.0) * 0.4
                    + diagram_rate * 0.3
                    + purpose_quality * 0.3;

                let tier_name = format!("{:?}", tier);
                (
                    tier_name.clone(),
                    TierQuality {
                        tier: tier_name,
                        file_count,
                        avg_content_length,
                        diagram_rate,
                        purpose_quality,
                        overall,
                    },
                )
            })
            .collect()
    }

    /// Generate actionable recommendations
    fn generate_recommendations(
        dimensions: &QualityDimensions,
        insights: &[FileInsight],
    ) -> Vec<QualityRecommendation> {
        let mut recs = Vec::new();

        // Coverage recommendation
        if dimensions.coverage < 0.9 {
            let priority = if dimensions.coverage < 0.5 {
                RecommendationPriority::Critical
            } else if dimensions.coverage < 0.7 {
                RecommendationPriority::High
            } else {
                RecommendationPriority::Medium
            };

            recs.push(QualityRecommendation {
                dimension: "coverage".to_string(),
                priority,
                current: dimensions.coverage,
                target: 0.95,
                action: "Increase file processing coverage or adjust tier thresholds".to_string(),
                affected_files: Vec::new(),
            });
        }

        // Completeness recommendation
        if dimensions.completeness < 0.7 {
            let priority = if dimensions.completeness < 0.3 {
                RecommendationPriority::Critical
            } else if dimensions.completeness < 0.5 {
                RecommendationPriority::High
            } else {
                RecommendationPriority::Medium
            };

            // Find files with low content
            let low_content_files: Vec<String> = insights
                .iter()
                .filter(|i| i.token_count < 200)
                .take(10)
                .map(|i| i.file_path.clone())
                .collect();

            recs.push(QualityRecommendation {
                dimension: "completeness".to_string(),
                priority,
                current: dimensions.completeness,
                target: 0.8,
                action: "Increase token budgets or add refinement passes for low-content files"
                    .to_string(),
                affected_files: low_content_files,
            });
        }

        // Diagram recommendation
        if dimensions.diagrams < 0.6 {
            let priority = if dimensions.diagrams < 0.3 {
                RecommendationPriority::High
            } else {
                RecommendationPriority::Medium
            };

            // Find important files without diagrams
            let no_diagram_files: Vec<String> = insights
                .iter()
                .filter(|i| {
                    !i.has_diagram()
                        && matches!(i.tier, ProcessingTier::Core | ProcessingTier::Important)
                })
                .take(10)
                .map(|i| i.file_path.clone())
                .collect();

            recs.push(QualityRecommendation {
                dimension: "diagrams".to_string(),
                priority,
                current: dimensions.diagrams,
                target: 0.7,
                action: "Enable diagram generation for Important/Core tier files".to_string(),
                affected_files: no_diagram_files,
            });
        }

        // Clarity recommendation
        if dimensions.clarity < 0.7 {
            let priority = if dimensions.clarity < 0.4 {
                RecommendationPriority::High
            } else {
                RecommendationPriority::Medium
            };

            // Find files with poor purpose statements
            let poor_purpose_files: Vec<String> = insights
                .iter()
                .filter(|i| Self::score_purpose_clarity(&i.purpose) < 0.5)
                .take(10)
                .map(|i| i.file_path.clone())
                .collect();

            recs.push(QualityRecommendation {
                dimension: "clarity".to_string(),
                priority,
                current: dimensions.clarity,
                target: 0.8,
                action: "Improve purpose statement prompts to generate clearer descriptions"
                    .to_string(),
                affected_files: poor_purpose_files,
            });
        }

        // Accuracy recommendation
        if dimensions.accuracy < 0.6 {
            recs.push(QualityRecommendation {
                dimension: "accuracy".to_string(),
                priority: RecommendationPriority::Low,
                current: dimensions.accuracy,
                target: 0.7,
                action: "Enable cross-reference extraction in file analysis".to_string(),
                affected_files: Vec::new(),
            });
        }

        // Sort by priority
        recs.sort_by(|a, b| {
            let priority_order = |p: &RecommendationPriority| match p {
                RecommendationPriority::Critical => 0,
                RecommendationPriority::High => 1,
                RecommendationPriority::Medium => 2,
                RecommendationPriority::Low => 3,
            };
            priority_order(&a.priority).cmp(&priority_order(&b.priority))
        });

        recs
    }

    /// Format as human-readable summary
    pub fn summary(&self) -> String {
        format!(
            "Quality: {:.1}% | Coverage: {:.0}% | Completeness: {:.0}% | Diagrams: {:.0}% | Clarity: {:.0}%",
            self.overall * 100.0,
            self.dimensions.coverage * 100.0,
            self.dimensions.completeness * 100.0,
            self.dimensions.diagrams * 100.0,
            self.dimensions.clarity * 100.0
        )
    }

    /// Format as detailed markdown report
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str("# Quality Metrics Report\n\n");
        md.push_str(&format!(
            "**Overall Score:** {:.1}%\n\n",
            self.overall * 100.0
        ));

        md.push_str("## Dimension Breakdown\n\n");
        md.push_str("| Dimension | Score | Status |\n");
        md.push_str("|-----------|-------|--------|\n");

        let status = |score: f64| {
            if score >= 0.8 {
                "✅ Good"
            } else if score >= 0.6 {
                "⚠️ Fair"
            } else {
                "❌ Needs Work"
            }
        };

        md.push_str(&format!(
            "| Coverage | {:.1}% | {} |\n",
            self.dimensions.coverage * 100.0,
            status(self.dimensions.coverage)
        ));
        md.push_str(&format!(
            "| Completeness | {:.1}% | {} |\n",
            self.dimensions.completeness * 100.0,
            status(self.dimensions.completeness)
        ));
        md.push_str(&format!(
            "| Accuracy | {:.1}% | {} |\n",
            self.dimensions.accuracy * 100.0,
            status(self.dimensions.accuracy)
        ));
        md.push_str(&format!(
            "| Diagrams | {:.1}% | {} |\n",
            self.dimensions.diagrams * 100.0,
            status(self.dimensions.diagrams)
        ));
        md.push_str(&format!(
            "| Clarity | {:.1}% | {} |\n",
            self.dimensions.clarity * 100.0,
            status(self.dimensions.clarity)
        ));

        if !self.by_tier.is_empty() {
            md.push_str("\n## Per-Tier Quality\n\n");
            md.push_str("| Tier | Files | Avg Content | Diagrams | Quality |\n");
            md.push_str("|------|-------|-------------|----------|--------|\n");

            let mut tiers: Vec<_> = self.by_tier.values().collect();
            tiers.sort_by(|a, b| {
                b.overall
                    .partial_cmp(&a.overall)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            for tier in tiers {
                md.push_str(&format!(
                    "| {} | {} | {} chars | {:.0}% | {:.1}% |\n",
                    tier.tier,
                    tier.file_count,
                    tier.avg_content_length,
                    tier.diagram_rate * 100.0,
                    tier.overall * 100.0
                ));
            }
        }

        if !self.recommendations.is_empty() {
            md.push_str("\n## Recommendations\n\n");

            for rec in &self.recommendations {
                md.push_str(&format!("### [{}] {}\n\n", rec.priority, rec.dimension));
                md.push_str(&format!(
                    "- **Current:** {:.1}% → **Target:** {:.1}%\n",
                    rec.current * 100.0,
                    rec.target * 100.0
                ));
                md.push_str(&format!("- **Action:** {}\n", rec.action));

                if !rec.affected_files.is_empty() {
                    md.push_str("- **Affected files:**\n");
                    for file in rec.affected_files.iter().take(5) {
                        md.push_str(&format!("  - `{}`\n", file));
                    }
                    if rec.affected_files.len() > 5 {
                        md.push_str(&format!(
                            "  - ... and {} more\n",
                            rec.affected_files.len() - 5
                        ));
                    }
                }
                md.push('\n');
            }
        }

        md.push_str(&format!("\n---\n*Generated: {}*\n", self.calculated_at));

        md
    }
}

/// Quality scorer implementation
#[derive(Debug, Clone, Default)]
pub struct QualityScorer;

impl QualityScorer {
    pub fn new() -> Self {
        Self
    }

    /// Calculate quality score from domain summaries
    pub fn score(&self, domains: &[DomainInsight]) -> QualityScore {
        if domains.is_empty() {
            return QualityScore::default();
        }

        QualityScore {
            content_coverage: self.score_content_coverage(domains),
            diagram_coverage: self.score_diagram_coverage(domains),
            relationships: self.score_relationships(domains),
        }
    }

    /// Score content coverage
    fn score_content_coverage(&self, domains: &[DomainInsight]) -> f32 {
        let total = domains.len();
        if total == 0 {
            return 0.0;
        }

        let domains_with_content = domains.iter().filter(|d| d.has_content()).count();

        let base_score = domains_with_content as f32 / total as f32;

        // Bonus for rich content (>500 words average)
        let avg_words: usize = domains
            .iter()
            .map(|d| d.content_word_count())
            .sum::<usize>()
            / total.max(1);
        let richness_bonus = if avg_words > 500 {
            0.1
        } else if avg_words > 200 {
            0.05
        } else {
            0.0
        };

        (base_score + richness_bonus).min(1.0)
    }

    /// Score diagram coverage
    fn score_diagram_coverage(&self, domains: &[DomainInsight]) -> f32 {
        let total = domains.len();
        if total == 0 {
            return 0.0;
        }

        let domains_with_diagrams = domains.iter().filter(|d| d.diagram.is_some()).count();

        domains_with_diagrams as f32 / total as f32
    }

    /// Score relationship documentation
    fn score_relationships(&self, domains: &[DomainInsight]) -> f32 {
        let total = domains.len();
        if total == 0 {
            return 0.0;
        }

        let domains_with_relationships = domains
            .iter()
            .filter(|d| !d.related_files.is_empty())
            .count();

        domains_with_relationships as f32 / total as f32
    }

    /// Validate cross-references against file system
    pub fn validate_cross_references(
        &self,
        domains: &[DomainInsight],
        project_root: &Path,
    ) -> Vec<CrossRefIssue> {
        let mut issues = vec![];

        for domain in domains {
            for rel in &domain.related_files {
                let target_path = project_root.join(&rel.path);
                if !target_path.exists() {
                    issues.push(CrossRefIssue {
                        source: domain.name.clone(),
                        target: rel.path.clone(),
                        issue_type: "missing_file".to_string(),
                    });
                }
            }
        }

        issues
    }
}

/// Cross-reference validation issue
#[derive(Debug, Clone)]
pub struct CrossRefIssue {
    pub source: String,
    pub target: String,
    pub issue_type: String,
}

/// Gap report for documentation
#[derive(Debug, Clone)]
pub struct GapReport {
    pub category: String,
    pub current_score: f32,
    pub files: Vec<String>,
}

/// Quality report for documentation
#[derive(Debug, Clone)]
pub struct QualityReport {
    pub overall_score: f32,
    pub target_score: f32,
    pub category_scores: QualityScore,
    pub gaps: Vec<GapReport>,
    pub refinement_turns_used: u8,
    pub recommendation: String,
}

impl QualityReport {
    pub fn to_markdown(&self) -> String {
        let mut content = String::new();

        content.push_str("# Documentation Quality Report\n\n");
        content.push_str(&format!(
            "**Overall Score:** {:.1}% (Target: {:.1}%)\n\n",
            self.overall_score * 100.0,
            self.target_score * 100.0
        ));

        content.push_str("## Category Scores\n\n");
        content.push_str("| Category | Score |\n");
        content.push_str("|----------|-------|\n");
        content.push_str(&format!(
            "| Content Coverage | {:.1}% |\n",
            self.category_scores.content_coverage * 100.0
        ));
        content.push_str(&format!(
            "| Diagram Coverage | {:.1}% |\n",
            self.category_scores.diagram_coverage * 100.0
        ));
        content.push_str(&format!(
            "| Cross-References | {:.1}% |\n",
            self.category_scores.relationships * 100.0
        ));

        if !self.gaps.is_empty() {
            content.push_str("\n## Identified Gaps\n\n");
            for gap in &self.gaps {
                content.push_str(&format!(
                    "- **{}**: {:.1}%\n",
                    gap.category,
                    gap.current_score * 100.0
                ));
            }
        }

        content.push_str(&format!(
            "\n**Refinement Turns Used:** {}\n",
            self.refinement_turns_used
        ));
        content.push_str(&format!("\n**Recommendation:** {}\n", self.recommendation));

        content
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki::exhaustive::bottom_up::Importance;

    fn make_domain(name: &str, has_content: bool, has_diagram: bool) -> DomainInsight {
        DomainInsight {
            name: name.to_string(),
            description: "Test domain".to_string(),
            importance: Importance::Medium,
            files: vec!["file.rs".to_string()],
            content: if has_content {
                "This is meaningful content that explains the domain in detail with multiple sentences and paragraphs for developers to understand the codebase thoroughly.".to_string()
            } else {
                String::new()
            },
            diagram: if has_diagram {
                Some("graph TD; A-->B".to_string())
            } else {
                None
            },
            related_files: vec![],
            gaps: vec![],
            token_count: 0,
        }
    }

    #[test]
    fn test_empty_domains() {
        let scorer = QualityScorer::new();
        let score = scorer.score(&[]);
        assert_eq!(score.overall(), 0.0);
    }

    #[test]
    fn test_quality_score_overall() {
        let score = QualityScore {
            content_coverage: 1.0,
            diagram_coverage: 1.0,
            relationships: 1.0,
        };
        assert!((score.overall() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_content_coverage() {
        let scorer = QualityScorer::new();
        let domains = vec![
            make_domain("a", true, false),
            make_domain("b", true, false),
            make_domain("c", false, false),
        ];
        let score = scorer.score(&domains);
        // 2/3 domains have content
        assert!(score.content_coverage > 0.6 && score.content_coverage < 0.8);
    }

    #[test]
    fn test_diagram_coverage() {
        let scorer = QualityScorer::new();
        let domains = vec![make_domain("a", true, true), make_domain("b", true, false)];
        let score = scorer.score(&domains);
        assert!((score.diagram_coverage - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_gaps_detection() {
        let score = QualityScore {
            content_coverage: 0.3,
            diagram_coverage: 0.8,
            relationships: 0.4,
        };
        let gaps = score.gaps(0.5);
        assert_eq!(gaps.len(), 2);
        assert!(gaps.contains(&"content_coverage".to_string()));
        assert!(gaps.contains(&"relationships".to_string()));
    }
}
