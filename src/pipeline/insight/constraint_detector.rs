//! Constraint Detector
//!
//! Detects hidden constraints in the codebase:
//! - Concurrency constraints (shared state, locks, channels)
//! - Initialization order dependencies
//! - Security constraints (auth, encryption, validation)
//! - Boundary constraints (limits, ranges, timeouts)
//! - Performance constraints (hot paths, SLAs)

use std::sync::Arc;

use tracing::debug;

use crate::config::Config;
use crate::pipeline::phases::convention_inference::AsyncStyle;

use super::types::{text_contains_any, Constraint, ConstraintType};
use super::InsightContext;

/// Trait for internal constraint analyzers
trait ConstraintAnalyzer: Send + Sync {
    fn name(&self) -> &str;
    fn detect(&self, ctx: &InsightContext<'_>) -> Vec<Constraint>;
}

/// Main constraint detector that orchestrates multiple analyzers
pub struct ConstraintDetector {
    analyzers: Vec<Box<dyn ConstraintAnalyzer>>,
    config: Arc<Config>,
}

impl ConstraintDetector {
    pub fn new(config: Arc<Config>) -> Self {
        let constraint_config = &config.insight.constraints;
        let mut analyzers: Vec<Box<dyn ConstraintAnalyzer>> = Vec::new();

        // Only add enabled analyzers
        if constraint_config.detect_concurrency {
            analyzers.push(Box::new(ConcurrencyAnalyzer));
        }
        if constraint_config.detect_init_order {
            analyzers.push(Box::new(InitOrderAnalyzer));
        }
        if constraint_config.detect_security {
            analyzers.push(Box::new(SecurityAnalyzer));
        }
        if constraint_config.detect_boundary {
            analyzers.push(Box::new(BoundaryAnalyzer));
        }
        if constraint_config.detect_performance {
            analyzers.push(Box::new(PerformanceAnalyzer));
        }

        Self { analyzers, config }
    }

    pub fn detect_all(&self, ctx: &InsightContext<'_>) -> Vec<Constraint> {
        let mut all_constraints = Vec::new();

        for analyzer in &self.analyzers {
            let constraints = analyzer.detect(ctx);
            debug!(
                analyzer = analyzer.name(),
                count = constraints.len(),
                "Detected constraints"
            );
            all_constraints.extend(constraints);
        }

        // Filter by minimum severity from config
        let min_severity = &self.config.insight.constraints.min_severity;
        all_constraints.retain(|c| self.meets_severity_threshold(c, min_severity));

        all_constraints
    }

    fn meets_severity_threshold(
        &self,
        constraint: &Constraint,
        min_severity: &crate::config::ConstraintSeverity,
    ) -> bool {
        use crate::config::ConstraintSeverity;

        let constraint_severity = match constraint.severity.to_lowercase().as_str() {
            "critical" => ConstraintSeverity::Critical,
            "high" => ConstraintSeverity::High,
            "medium" => ConstraintSeverity::Medium,
            _ => ConstraintSeverity::Low,
        };

        constraint_severity >= *min_severity
    }
}

struct ConcurrencyAnalyzer;

impl ConstraintAnalyzer for ConcurrencyAnalyzer {
    fn name(&self) -> &str {
        "concurrency"
    }

    fn detect(&self, ctx: &InsightContext<'_>) -> Vec<Constraint> {
        const CONCURRENCY_KEYWORDS: &[&str] = &[
            "arc", "mutex", "rwlock", "atomic", "shared", "thread", "async",
        ];
        const GOTCHA_KEYWORDS: &[&str] = &[
            "race", "concurrent", "thread", "race condition", "deadlock",
        ];

        let mut constraints = Vec::new();

        for dep in &ctx.constraints.hidden_dependencies {
            let desc_lower = dep.description.to_lowercase();
            if text_contains_any(&desc_lower, CONCURRENCY_KEYWORDS) {
                constraints.push(Constraint {
                    name: format!("Shared state: {} <-> {}", dep.source, dep.target),
                    constraint_type: ConstraintType::Concurrency,
                    description: dep.description.clone(),
                    prevention: Some(dep.impact.clone()),
                    evidence: dep.evidence.iter().map(|e| e.file.clone()).collect(),
                    severity: "high".to_string(),
                });
            }
        }

        // Check async pattern for concurrency implications
        if ctx.conventions.async_pattern.style != AsyncStyle::Synchronous {
            for pattern in &ctx.conventions.async_pattern.concurrency_patterns {
                constraints.push(Constraint {
                    name: format!("Concurrency pattern: {}", pattern),
                    constraint_type: ConstraintType::Concurrency,
                    description: format!(
                        "Project uses {} concurrency pattern with {:?} async style.",
                        pattern, ctx.conventions.async_pattern.style
                    ),
                    prevention: Some("Follow established concurrency patterns".to_string()),
                    evidence: Vec::new(),
                    severity: "medium".to_string(),
                });
            }

            if let Some(runtime) = &ctx.conventions.async_pattern.runtime
                && runtime.to_lowercase().contains("tokio")
            {
                constraints.push(Constraint {
                    name: "Tokio runtime usage".to_string(),
                    constraint_type: ConstraintType::Concurrency,
                    description: "Project uses Tokio runtime. Blocking operations must be avoided in async context.".to_string(),
                    prevention: Some("Use spawn_blocking for CPU-heavy or blocking operations".to_string()),
                    evidence: Vec::new(),
                    severity: "high".to_string(),
                });
            }
        }

        for gotcha in &ctx.constraints.gotchas {
            let combined = format!("{} {}", gotcha.title, gotcha.description).to_lowercase();
            if text_contains_any(&combined, GOTCHA_KEYWORDS) {
                constraints.push(Constraint {
                    name: gotcha.title.clone(),
                    constraint_type: ConstraintType::Concurrency,
                    description: gotcha.description.clone(),
                    prevention: Some(gotcha.solution.clone()),
                    evidence: gotcha.related_files.clone(),
                    severity: "high".to_string(),
                });
            }
        }

        constraints
    }
}

struct InitOrderAnalyzer;

impl ConstraintAnalyzer for InitOrderAnalyzer {
    fn name(&self) -> &str {
        "init_order"
    }

    fn detect(&self, ctx: &InsightContext<'_>) -> Vec<Constraint> {
        const DEP_KEYWORDS: &[&str] = &[
            "init", "before", "after", "order", "depends", "require", "setup",
        ];
        const RULE_KEYWORDS: &[&str] = &["first", "before", "prerequisite"];

        let mut constraints = Vec::new();

        for dep in &ctx.constraints.hidden_dependencies {
            let desc_lower = dep.description.to_lowercase();
            if text_contains_any(&desc_lower, DEP_KEYWORDS) {
                constraints.push(Constraint {
                    name: format!("Init order: {} → {}", dep.source, dep.target),
                    constraint_type: ConstraintType::InitOrder,
                    description: dep.description.clone(),
                    prevention: Some(format!(
                        "Ensure {} is initialized before {}. {}",
                        dep.source, dep.target, dep.impact
                    )),
                    evidence: dep.evidence.iter().map(|e| e.file.clone()).collect(),
                    severity: "high".to_string(),
                });
            }
        }

        for rule in &ctx.constraints.implicit_rules {
            let desc_lower = rule.description.to_lowercase();
            if text_contains_any(&desc_lower, RULE_KEYWORDS) {
                constraints.push(Constraint {
                    name: rule.name.clone(),
                    constraint_type: ConstraintType::InitOrder,
                    description: rule.description.clone(),
                    prevention: Some(rule.description.clone()),
                    evidence: rule.evidence.iter().map(|e| e.file.clone()).collect(),
                    severity: "medium".to_string(),
                });
            }
        }

        for workflow in &ctx.constraints.complex_workflows {
            if workflow.steps.len() > 2 {
                let steps_desc = workflow
                    .steps
                    .iter()
                    .map(|s| s.action.as_str())
                    .collect::<Vec<_>>()
                    .join(" → ");
                constraints.push(Constraint {
                    name: format!("Workflow order: {}", workflow.name),
                    constraint_type: ConstraintType::InitOrder,
                    description: format!("Steps must follow order: {}", steps_desc),
                    prevention: Some(format!("Follow the workflow sequence: {}", steps_desc)),
                    evidence: workflow
                        .steps
                        .iter()
                        .flat_map(|s| s.files_involved.clone())
                        .collect(),
                    severity: "medium".to_string(),
                });
            }
        }

        constraints
    }
}

struct SecurityAnalyzer;

impl ConstraintAnalyzer for SecurityAnalyzer {
    fn name(&self) -> &str {
        "security"
    }

    fn detect(&self, ctx: &InsightContext<'_>) -> Vec<Constraint> {
        const GOTCHA_KEYWORDS: &[&str] = &[
            "security", "auth", "permission", "credential",
            "injection", "xss", "csrf", "encrypt", "validate", "sanitize",
        ];
        const ANTI_PATTERN_KEYWORDS: &[&str] = &[
            "security", "unsafe", "vulnerability", "insecure",
        ];
        const RULE_KEYWORDS: &[&str] = &["validate", "sanitize", "auth", "permission"];

        let mut constraints = Vec::new();

        for gotcha in &ctx.constraints.gotchas {
            let combined = format!("{} {}", gotcha.title, gotcha.description).to_lowercase();
            if text_contains_any(&combined, GOTCHA_KEYWORDS) {
                constraints.push(Constraint {
                    name: gotcha.title.clone(),
                    constraint_type: ConstraintType::Security,
                    description: gotcha.description.clone(),
                    prevention: Some(gotcha.solution.clone()),
                    evidence: gotcha.related_files.clone(),
                    severity: "critical".to_string(),
                });
            }
        }

        for pattern in &ctx.constraints.anti_patterns {
            let combined = format!("{} {}", pattern.name, pattern.description).to_lowercase();
            if text_contains_any(&combined, ANTI_PATTERN_KEYWORDS) {
                constraints.push(Constraint {
                    name: format!("Anti-pattern: {}", pattern.name),
                    constraint_type: ConstraintType::Security,
                    description: pattern.description.clone(),
                    prevention: Some(pattern.correct_approach.clone()),
                    evidence: pattern.evidence.iter().map(|e| e.file.clone()).collect(),
                    severity: "high".to_string(),
                });
            }
        }

        for rule in &ctx.constraints.implicit_rules {
            let desc_lower = rule.description.to_lowercase();
            if text_contains_any(&desc_lower, RULE_KEYWORDS) {
                constraints.push(Constraint {
                    name: rule.name.clone(),
                    constraint_type: ConstraintType::Security,
                    description: rule.description.clone(),
                    prevention: Some(rule.description.clone()),
                    evidence: rule.evidence.iter().map(|e| e.file.clone()).collect(),
                    severity: "high".to_string(),
                });
            }
        }

        constraints
    }
}

struct BoundaryAnalyzer;

impl ConstraintAnalyzer for BoundaryAnalyzer {
    fn name(&self) -> &str {
        "boundary"
    }

    fn detect(&self, ctx: &InsightContext<'_>) -> Vec<Constraint> {
        const GOTCHA_KEYWORDS: &[&str] = &[
            "limit", "max", "min", "timeout", "size", "overflow", "boundary", "range",
        ];
        const RULE_KEYWORDS: &[&str] = &["limit", "maximum", "minimum", "bound", "timeout"];

        let mut constraints = Vec::new();

        for gotcha in &ctx.constraints.gotchas {
            let combined = format!("{} {}", gotcha.title, gotcha.description).to_lowercase();
            if text_contains_any(&combined, GOTCHA_KEYWORDS) {
                constraints.push(Constraint {
                    name: gotcha.title.clone(),
                    constraint_type: ConstraintType::Boundary,
                    description: gotcha.description.clone(),
                    prevention: Some(gotcha.solution.clone()),
                    evidence: gotcha.related_files.clone(),
                    severity: "medium".to_string(),
                });
            }
        }

        for rule in &ctx.constraints.implicit_rules {
            let desc_lower = rule.description.to_lowercase();
            if text_contains_any(&desc_lower, RULE_KEYWORDS) {
                constraints.push(Constraint {
                    name: rule.name.clone(),
                    constraint_type: ConstraintType::Boundary,
                    description: rule.description.clone(),
                    prevention: Some(rule.description.clone()),
                    evidence: rule.evidence.iter().map(|e| e.file.clone()).collect(),
                    severity: "medium".to_string(),
                });
            }
        }

        constraints
    }
}

struct PerformanceAnalyzer;

impl ConstraintAnalyzer for PerformanceAnalyzer {
    fn name(&self) -> &str {
        "performance"
    }

    fn detect(&self, ctx: &InsightContext<'_>) -> Vec<Constraint> {
        const GOTCHA_KEYWORDS: &[&str] = &[
            "performance", "slow", "latency", "cache", "n+1", "bottleneck", "hot path", "sla",
        ];
        const ANTI_PATTERN_KEYWORDS: &[&str] = &[
            "performance", "slow", "inefficient", "expensive",
        ];

        let mut constraints = Vec::new();

        for gotcha in &ctx.constraints.gotchas {
            let combined = format!("{} {}", gotcha.title, gotcha.description).to_lowercase();
            if text_contains_any(&combined, GOTCHA_KEYWORDS) {
                constraints.push(Constraint {
                    name: gotcha.title.clone(),
                    constraint_type: ConstraintType::Performance,
                    description: gotcha.description.clone(),
                    prevention: Some(gotcha.solution.clone()),
                    evidence: gotcha.related_files.clone(),
                    severity: "medium".to_string(),
                });
            }
        }

        for pattern in &ctx.constraints.anti_patterns {
            let combined = format!("{} {}", pattern.name, pattern.description).to_lowercase();
            if text_contains_any(&combined, ANTI_PATTERN_KEYWORDS) {
                constraints.push(Constraint {
                    name: format!("Performance anti-pattern: {}", pattern.name),
                    constraint_type: ConstraintType::Performance,
                    description: pattern.description.clone(),
                    prevention: Some(pattern.correct_approach.clone()),
                    evidence: pattern.evidence.iter().map(|e| e.file.clone()).collect(),
                    severity: "medium".to_string(),
                });
            }
        }

        constraints
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::phases::constraint_extraction::{
        ExtractedConstraints, Gotcha, HiddenDependency, HiddenDepType,
    };
    use crate::pipeline::phases::convention_inference::{
        ArchitectureConvention, AsyncPattern, ErrorHandlingPattern, FileOrganization,
        InferredConventions, NamingConventions, TestingConvention,
    };
    use crate::pipeline::context::VerifiedFileRegistry;

    fn create_test_context<'a>(
        conventions: &'a InferredConventions,
        constraints: &'a ExtractedConstraints,
        registry: &'a VerifiedFileRegistry,
    ) -> InsightContext<'a> {
        InsightContext {
            conventions,
            constraints,
            synthesis: None,
            file_registry: registry,
        }
    }

    #[test]
    fn test_concurrency_analyzer_detects_shared_state() {
        let conventions = InferredConventions {
            architecture: ArchitectureConvention::default(),
            naming: NamingConventions::default(),
            file_organization: FileOrganization::default(),
            error_handling: ErrorHandlingPattern::default(),
            async_pattern: AsyncPattern::default(),
            patterns: Vec::new(),
            testing: TestingConvention::default(),
        };

        let mut constraints = ExtractedConstraints::default();
        constraints.hidden_dependencies.push(HiddenDependency {
            source: "service_a".to_string(),
            target: "service_b".to_string(),
            dependency_type: HiddenDepType::SharedState,
            description: "Shared Arc<Mutex<State>> between services".to_string(),
            impact: "Race condition if not properly locked".to_string(),
            evidence: Vec::new(),
        });

        let registry = VerifiedFileRegistry::empty();
        let ctx = create_test_context(&conventions, &constraints, &registry);

        let analyzer = ConcurrencyAnalyzer;
        let detected = analyzer.detect(&ctx);

        assert!(!detected.is_empty());
        assert!(detected[0].name.contains("Shared state"));
    }

    #[test]
    fn test_security_analyzer_detects_auth_gotcha() {
        let conventions = InferredConventions {
            architecture: ArchitectureConvention::default(),
            naming: NamingConventions::default(),
            file_organization: FileOrganization::default(),
            error_handling: ErrorHandlingPattern::default(),
            async_pattern: AsyncPattern::default(),
            patterns: Vec::new(),
            testing: TestingConvention::default(),
        };

        let mut constraints = ExtractedConstraints::default();
        constraints.gotchas.push(Gotcha {
            title: "Authentication bypass".to_string(),
            description: "Must validate JWT tokens on all protected endpoints".to_string(),
            when: "When accessing protected resources".to_string(),
            solution: "Use auth middleware for all routes".to_string(),
            related_files: vec!["src/auth/middleware.rs".to_string()],
        });

        let registry = VerifiedFileRegistry::empty();
        let ctx = create_test_context(&conventions, &constraints, &registry);

        let analyzer = SecurityAnalyzer;
        let detected = analyzer.detect(&ctx);

        assert!(!detected.is_empty());
        assert_eq!(detected[0].constraint_type, ConstraintType::Security);
    }
}
