//! Value-Centric Configuration System
//!
//! Core principle: "What mistakes would AI make without this information?"
//!
//! Design:
//! - Presets for common use cases (quick/standard/thorough/exhaustive)
//! - Value-based quality metrics (mistake_prevention, discoverability, artifact_fitness)
//! - Multi-dimensional convergence (not single quality_score)
//! - Domain-aware generation (business rules, compliance)
//! - All thresholds user-configurable

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// PRESETS
// =============================================================================

/// Configuration presets for common use cases
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ConfigPreset {
    /// Fast iteration: ~15 LLM calls, basic quality
    Quick,
    /// Balanced quality and cost: ~50 LLM calls
    #[default]
    Standard,
    /// High quality output: ~100 LLM calls
    Thorough,
    /// Maximum quality, long-running: unlimited calls
    Exhaustive,
}

impl ConfigPreset {
    pub fn apply(&self, config: &mut Config) {
        match self {
            Self::Quick => {
                config.llm.default_model = "claude-haiku-4-5-20251001".into();
                config.value.min_overall = 0.5;
                config.convergence.max_iterations = 10;
                config.convergence.consecutive_passes = 1;
                config.analysis.depth = AnalysisDepth::Fast;
            }
            Self::Standard => {
                config.llm.default_model = "claude-sonnet-4-5-20250929".into();
                config.value.min_overall = 0.6;
                config.convergence.max_iterations = 30;
                config.convergence.consecutive_passes = 2;
                config.analysis.depth = AnalysisDepth::Standard;
            }
            Self::Thorough => {
                config.llm.default_model = "claude-sonnet-4-5-20250929".into();
                config.llm.performance_model = Some("claude-opus-4-5-20251101".into());
                config.value.min_overall = 0.7;
                config.convergence.max_iterations = 50;
                config.convergence.consecutive_passes = 2;
                config.analysis.depth = AnalysisDepth::Complete;
            }
            Self::Exhaustive => {
                config.llm.default_model = "claude-opus-4-5-20251101".into();
                config.llm.performance_model = Some("claude-opus-4-5-20251101".into());
                config.value.min_overall = 0.8;
                config.convergence.max_iterations = 100;
                config.convergence.consecutive_passes = 3;
                config.analysis.depth = AnalysisDepth::Complete;
                config.performance.max_runtime_hours = 336; // 2 weeks
            }
        }
    }
}

// =============================================================================
// ROOT CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: String,
    pub preset: Option<ConfigPreset>,
    pub generation: GenerationConfig,
    pub value: ValueConfig,
    pub convergence: ConvergenceConfig,
    pub domain: DomainConfig,
    pub tiers: TierConfig,
    pub artifacts: ArtifactConfigs,
    pub llm: LlmConfig,
    pub analysis: AnalysisConfig,
    pub insight: InsightConfig,
    pub budget: BudgetConfig,
    pub performance: PerformanceConfig,
    pub project: ProjectConfig,
    pub output: OutputConfig,
    pub validation: ValidationConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: "4.0".into(),
            preset: Some(ConfigPreset::Standard),
            generation: GenerationConfig::default(),
            value: ValueConfig::default(),
            convergence: ConvergenceConfig::default(),
            domain: DomainConfig::default(),
            tiers: TierConfig::default(),
            artifacts: ArtifactConfigs::default(),
            llm: LlmConfig::default(),
            analysis: AnalysisConfig::default(),
            insight: InsightConfig::default(),
            budget: BudgetConfig::default(),
            performance: PerformanceConfig::default(),
            project: ProjectConfig::default(),
            output: OutputConfig::default(),
            validation: ValidationConfig::default(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> crate::types::Result<()> {
        use crate::types::ClaudegenError;

        // Value thresholds must be in valid range
        if !(0.0..=1.0).contains(&self.value.min_overall) {
            return Err(ClaudegenError::Config(
                "value.min_overall must be between 0.0 and 1.0".into(),
            ));
        }

        // Convergence validation
        if self.convergence.max_iterations == 0 {
            return Err(ClaudegenError::Config(
                "convergence.max_iterations must be > 0".into(),
            ));
        }
        if self.convergence.consecutive_passes == 0 {
            return Err(ClaudegenError::Config(
                "convergence.consecutive_passes must be > 0".into(),
            ));
        }

        // Budget validation
        if self.budget.total_tokens == 0 {
            return Err(ClaudegenError::Config(
                "budget.total_tokens must be > 0".into(),
            ));
        }

        // Performance validation
        if self.performance.parallel_workers == 0 {
            return Err(ClaudegenError::Config(
                "performance.parallel_workers must be > 0".into(),
            ));
        }

        // =========================================================================
        // Cross-field validation (interdependency checks)
        // Order matters: validate base configs before derived configs
        // =========================================================================

        // 1. Convergence consistency (base config - must validate first)
        //    This must come before deep_review validation because deep_review.required_passes
        //    is derived from convergence.consecutive_passes
        if self.convergence.consecutive_passes > self.convergence.max_iterations {
            return Err(ClaudegenError::Config(format!(
                "convergence.consecutive_passes ({}) must be <= max_iterations ({})",
                self.convergence.consecutive_passes, self.convergence.max_iterations
            )));
        }

        // 2. Clean pass: max_attempts must allow convergence
        if self.validation.clean_pass.max_attempts < self.validation.clean_pass.consecutive_passes {
            return Err(ClaudegenError::Config(format!(
                "validation.clean_pass.max_attempts ({}) must be >= consecutive_passes ({})",
                self.validation.clean_pass.max_attempts,
                self.validation.clean_pass.consecutive_passes
            )));
        }

        // 3. Deep review: max_attempts must allow convergence (derived config)
        let deep_review = self.deep_review();
        if deep_review.max_attempts < deep_review.required_passes {
            return Err(ClaudegenError::Config(format!(
                "deep_review.max_attempts ({}) must be >= required_passes ({})",
                deep_review.max_attempts, deep_review.required_passes
            )));
        }

        // 4. Quality thresholds ordering (warning only)
        if self.convergence.early_exit_threshold < self.value.min_overall {
            tracing::warn!(
                early_exit = self.convergence.early_exit_threshold,
                min_overall = self.value.min_overall,
                "early_exit_threshold < min_overall: may exit before quality target reached"
            );
        }

        self.value.validate()?;
        self.convergence.validate()?;

        Ok(())
    }
}

// =============================================================================
// GENERATION CONFIG
// =============================================================================

/// What to generate and how
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GenerationConfig {
    pub strategy: GenerationStrategy,
    pub artifacts: Vec<ArtifactType>,
    pub limits: ArtifactLimits,
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            strategy: GenerationStrategy::ValueDriven,
            artifacts: vec![
                ArtifactType::ClaudeMd,
                ArtifactType::Rules,
                ArtifactType::Skills,
                ArtifactType::Agents,
            ],
            limits: ArtifactLimits::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GenerationStrategy {
    /// Focus on high-value, mistake-preventing content
    #[default]
    ValueDriven,
    /// Maximize coverage of codebase
    CoverageDriven,
    /// Minimal output, only critical constraints
    Minimal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    ClaudeMd,
    Rules,
    Skills,
    Agents,
}

impl ArtifactType {
    pub fn all() -> Vec<Self> {
        vec![Self::ClaudeMd, Self::Rules, Self::Skills, Self::Agents]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeMd => "claude_md",
            Self::Rules => "rules",
            Self::Skills => "skills",
            Self::Agents => "agents",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtifactLimits {
    pub max_rules: usize,
    pub max_skills: usize,
    pub max_agents: usize,
}

impl Default for ArtifactLimits {
    fn default() -> Self {
        Self {
            max_rules: 20,
            max_skills: 15,
            max_agents: 10,
        }
    }
}

// =============================================================================
// VALUE CONFIG - Core of the new system
// =============================================================================

/// Value-based quality configuration
/// Core question: "Would AI make mistakes without this information?"
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ValueConfig {
    /// Minimum overall value score (weighted average of dimensions)
    pub min_overall: f32,
    /// Per-dimension thresholds
    pub dimensions: ValueDimensions,
    /// Weights for calculating overall score
    pub weights: ValueWeights,
}

impl Default for ValueConfig {
    fn default() -> Self {
        Self {
            min_overall: 0.6,
            dimensions: ValueDimensions::default(),
            weights: ValueWeights::default(),
        }
    }
}

impl ValueConfig {
    pub fn validate(&self) -> crate::types::Result<()> {
        use crate::types::ClaudegenError;

        // Check dimension thresholds
        let dims = &self.dimensions;
        for (name, val) in [
            ("mistake_prevention", dims.mistake_prevention),
            ("discoverability", dims.discoverability),
            ("artifact_fitness", dims.artifact_fitness),
        ] {
            if !(0.0..=1.0).contains(&val) {
                return Err(ClaudegenError::Config(format!(
                    "value.dimensions.{name} must be between 0.0 and 1.0"
                )));
            }
        }

        // Weights should sum to ~1.0 (allow small tolerance)
        let w = &self.weights;
        let sum = w.mistake_prevention + w.discoverability + w.artifact_fitness;
        if (sum - 1.0).abs() > 0.01 {
            return Err(ClaudegenError::Config(format!(
                "value.weights should sum to 1.0 (current: {sum:.2})"
            )));
        }

        Ok(())
    }

    /// Calculate overall value score from dimension scores
    pub fn calculate_overall(&self, scores: &ValueScores) -> f32 {
        let w = &self.weights;
        w.mistake_prevention * scores.mistake_prevention
            + w.discoverability * scores.discoverability
            + w.artifact_fitness * scores.artifact_fitness
    }
}

/// Minimum thresholds per value dimension
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ValueDimensions {
    /// How well does this prevent AI mistakes? (0.0 = no prevention, 1.0 = prevents critical mistakes)
    pub mistake_prevention: f32,
    /// How hard is this to discover from code alone? (0.0 = obvious, 1.0 = requires experience)
    pub discoverability: f32,
    /// How well does this fit the artifact type? (0.0 = wrong place, 1.0 = perfect fit)
    pub artifact_fitness: f32,
}

impl Default for ValueDimensions {
    fn default() -> Self {
        Self {
            mistake_prevention: 0.5,
            discoverability: 0.4,
            artifact_fitness: 0.6,
        }
    }
}

/// Weights for calculating overall value score
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ValueWeights {
    pub mistake_prevention: f32,
    pub discoverability: f32,
    pub artifact_fitness: f32,
}

impl Default for ValueWeights {
    fn default() -> Self {
        Self {
            mistake_prevention: 0.4,
            discoverability: 0.3,
            artifact_fitness: 0.3,
        }
    }
}

/// Calculated value scores for content
#[derive(Debug, Clone, Default)]
pub struct ValueScores {
    pub mistake_prevention: f32,
    pub discoverability: f32,
    pub artifact_fitness: f32,
}

impl ValueScores {
    pub fn new(mistake_prevention: f32, discoverability: f32, artifact_fitness: f32) -> Self {
        Self {
            mistake_prevention,
            discoverability,
            artifact_fitness,
        }
    }

    pub fn meets_thresholds(&self, dims: &ValueDimensions) -> bool {
        self.mistake_prevention >= dims.mistake_prevention
            && self.discoverability >= dims.discoverability
            && self.artifact_fitness >= dims.artifact_fitness
    }
}

// =============================================================================
// CONVERGENCE CONFIG
// =============================================================================

/// Multi-dimensional convergence criteria
/// NOT just "quality >= threshold" - requires formal + value + stability
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConvergenceConfig {
    /// Maximum iterations before forced termination
    pub max_iterations: usize,
    /// Consecutive passes required for stability
    pub consecutive_passes: usize,
    /// Maximum score oscillations before forcing termination
    pub max_oscillations: usize,
    /// Early exit if this score reached
    pub early_exit_threshold: f32,
    /// Iterations without improvement before escalation
    pub stagnation_patience: usize,
    /// Minimum improvement per iteration
    pub min_improvement: f32,
    /// Formal validation must pass
    pub require_formal_pass: bool,
    /// Cross-artifact validation must pass
    pub require_cross_artifact_pass: bool,
}

impl Default for ConvergenceConfig {
    fn default() -> Self {
        Self {
            max_iterations: 30,
            consecutive_passes: 2,
            max_oscillations: 3,
            early_exit_threshold: 0.9,
            stagnation_patience: 5,
            min_improvement: 0.01,
            require_formal_pass: true,
            require_cross_artifact_pass: true,
        }
    }
}

impl ConvergenceConfig {
    pub fn validate(&self) -> crate::types::Result<()> {
        use crate::types::ClaudegenError;

        if self.consecutive_passes > self.max_iterations {
            return Err(ClaudegenError::Config(
                "consecutive_passes cannot exceed max_iterations".into(),
            ));
        }

        if !(0.0..=1.0).contains(&self.early_exit_threshold) {
            return Err(ClaudegenError::Config(
                "early_exit_threshold must be between 0.0 and 1.0".into(),
            ));
        }

        Ok(())
    }
}

/// Current convergence state
#[derive(Debug, Clone, Default)]
pub struct ConvergenceState {
    pub iteration: usize,
    pub consecutive_pass_count: usize,
    pub oscillation_count: usize,
    pub stagnation_count: usize,
    pub last_score: f32,
    pub score_history: Vec<f32>,
}

impl ConvergenceState {
    pub fn record_result(&mut self, passed: bool, score: f32) {
        self.iteration += 1;

        if passed {
            self.consecutive_pass_count += 1;
        } else {
            self.consecutive_pass_count = 0;
        }

        // Detect oscillation
        if self.score_history.len() >= 2 {
            let prev = self.score_history[self.score_history.len() - 1];
            let prev_prev = self.score_history[self.score_history.len() - 2];
            if (score - prev_prev).abs() < 0.01 && (score - prev).abs() > 0.05 {
                self.oscillation_count += 1;
            }
        }

        // Detect stagnation
        let improvement = score - self.last_score;
        if improvement.abs() < 0.01 {
            self.stagnation_count += 1;
        } else {
            self.stagnation_count = 0;
        }

        self.last_score = score;
        self.score_history.push(score);
    }

    pub fn should_terminate(&self, config: &ConvergenceConfig) -> ConvergenceStatus {
        // Check max iterations
        if self.iteration >= config.max_iterations {
            return ConvergenceStatus::MaxIterationsReached;
        }

        // Check oscillation
        if self.oscillation_count >= config.max_oscillations {
            return ConvergenceStatus::Oscillating;
        }

        // Check stagnation
        if self.stagnation_count >= config.stagnation_patience {
            return ConvergenceStatus::Stagnated;
        }

        // Check early exit
        if self.last_score >= config.early_exit_threshold {
            return ConvergenceStatus::EarlyExit;
        }

        // Check stability (consecutive passes)
        if self.consecutive_pass_count >= config.consecutive_passes {
            return ConvergenceStatus::Converged;
        }

        ConvergenceStatus::InProgress
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvergenceStatus {
    InProgress,
    Converged,
    EarlyExit,
    MaxIterationsReached,
    Oscillating,
    Stagnated,
}

impl ConvergenceStatus {
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::InProgress)
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Self::Converged | Self::EarlyExit)
    }
}

// =============================================================================
// DOMAIN CONFIG
// =============================================================================

/// Domain-specific configuration for business rule extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DomainConfig {
    /// Domain type for specialized extraction
    pub domain_type: DomainType,
    /// Domain-specific terminology
    pub terminology: Vec<TermDefinition>,
    /// Compliance/regulatory requirements
    pub compliance: Vec<String>,
    /// Custom business rule patterns to look for
    pub business_patterns: Vec<String>,
}

impl Default for DomainConfig {
    fn default() -> Self {
        Self {
            domain_type: DomainType::Generic,
            terminology: Vec::new(),
            compliance: Vec::new(),
            business_patterns: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum DomainType {
    ECommerce,
    FinTech,
    Healthcare,
    SaaS,
    #[default]
    Generic,
}

impl DomainType {
    pub fn default_compliance(&self) -> Vec<&'static str> {
        match self {
            Self::ECommerce => vec!["PCI-DSS", "GDPR"],
            Self::FinTech => vec!["PCI-DSS", "AML", "KYC", "SOX"],
            Self::Healthcare => vec!["HIPAA", "GDPR"],
            Self::SaaS => vec!["SOC2", "GDPR"],
            Self::Generic => vec![],
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ECommerce => "ecommerce",
            Self::FinTech => "fintech",
            Self::Healthcare => "healthcare",
            Self::SaaS => "saas",
            Self::Generic => "generic",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermDefinition {
    pub term: String,
    pub definition: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

// =============================================================================
// TIER CONFIG - Value classification patterns
// =============================================================================

/// Patterns for classifying content value
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TierConfig {
    /// Tier 0: Immediately reject (generic knowledge)
    pub tier0: TierPatterns,
    /// Tier 2: Medium value (requires analysis)
    pub tier2: TierPatterns,
    /// Tier 3: High value (must keep)
    pub tier3: TierPatterns,
}

impl Default for TierConfig {
    fn default() -> Self {
        Self {
            tier0: TierPatterns::default_tier0(),
            tier2: TierPatterns::default_tier2(),
            tier3: TierPatterns::default_tier3(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TierPatterns {
    /// Keywords that indicate this tier
    pub keywords: Vec<String>,
    /// Regex patterns for matching
    pub patterns: Vec<String>,
}

impl TierPatterns {
    /// Default patterns for Tier 0 (reject)
    pub fn default_tier0() -> Self {
        Self {
            keywords: vec![
                "best practices".into(),
                "clean code".into(),
                "follow conventions".into(),
                "write tests".into(),
                "handle errors".into(),
                "cargo build".into(),
                "npm install".into(),
                "pip install".into(),
                "go build".into(),
                "docker build".into(),
            ],
            patterns: vec![
                r"use `?[a-z]+`? (for|to) (build|test|run)".into(),
                r"(always|should) (write|add|include) (tests|comments|docs)".into(),
            ],
        }
    }

    /// Default patterns for Tier 2 (medium value - requires analysis)
    pub fn default_tier2() -> Self {
        Self {
            keywords: vec![
                "convention".into(),
                "pattern".into(),
                "approach".into(),
                "strategy".into(),
                "recommended".into(),
                "preferred".into(),
                "idiom".into(),
                "style guide".into(),
            ],
            patterns: vec![
                r"(we|this project) (use|follow|prefer)".into(),
                r"(standard|typical|usual) (way|approach|pattern)".into(),
                r"by convention".into(),
            ],
        }
    }

    /// Default patterns for Tier 3 (high value)
    pub fn default_tier3() -> Self {
        Self {
            keywords: vec![
                "must".into(),
                "never".into(),
                "critical".into(),
                "constraint".into(),
                "breaks if".into(),
                "fails when".into(),
                "required for".into(),
                "gotcha".into(),
                "pitfall".into(),
                "hidden".into(),
                "order matters".into(),
                "race condition".into(),
                "deadlock".into(),
            ],
            patterns: vec![
                r"(must|always) .+ (before|after) .+".into(),
                r"(never|do not|don't) .+ (without|unless) .+".into(),
                r"(will|can) (fail|break|crash) (if|when|unless)".into(),
            ],
        }
    }

    pub fn matches(&self, content: &str) -> bool {
        let lower = content.to_lowercase();

        // Check keywords
        if self.keywords.iter().any(|k| lower.contains(&k.to_lowercase())) {
            return true;
        }

        // Check patterns (compile lazily - this could be optimized with caching)
        for pattern in &self.patterns {
            if let Ok(re) = regex::Regex::new(pattern)
                && re.is_match(&lower) {
                    return true;
                }
        }

        false
    }
}

// =============================================================================
// ARTIFACT CONFIGS
// =============================================================================

/// Per-artifact-type configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct ArtifactConfigs {
    pub claude_md: ClaudeMdConfig,
    pub rules: RulesConfig,
    pub skills: SkillsConfig,
    pub agents: AgentsConfig,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeMdConfig {
    /// Required sections
    pub sections: Vec<String>,
    /// Minimum file references
    pub min_file_refs: usize,
    /// Include architecture diagram
    pub include_diagram: bool,
}

impl Default for ClaudeMdConfig {
    fn default() -> Self {
        Self {
            sections: vec![
                "Core Abstraction".into(),
                "Critical Constraints".into(),
                "Architecture Intent".into(),
                "Gotchas".into(),
            ],
            min_file_refs: 5,
            include_diagram: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RulesConfig {
    /// Rule types to extract
    pub types: Vec<RuleType>,
    /// Minimum evidence references per rule
    pub min_evidence_refs: usize,
    /// Require "why" explanation
    pub require_why: bool,
    /// Require example (correct/wrong)
    pub require_example: bool,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            types: vec![
                RuleType::Technical,
                RuleType::Business,
                RuleType::Security,
                RuleType::Compliance,
            ],
            min_evidence_refs: 1,
            require_why: true,
            require_example: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum RuleType {
    Technical,
    Business,
    Security,
    Compliance,
}

impl RuleType {
    pub fn all() -> Vec<Self> {
        vec![Self::Technical, Self::Business, Self::Security, Self::Compliance]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillsConfig {
    /// Minimum evidence references per skill
    pub min_evidence_refs: usize,
    /// Require context section
    pub require_context: bool,
    /// Require verification checklist
    pub require_verification: bool,
    /// Minimum steps in task section
    pub min_steps: usize,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            min_evidence_refs: 2,
            require_context: true,
            require_verification: true,
            min_steps: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentsConfig {
    /// Minimum evidence references per agent
    pub min_evidence_refs: usize,
    /// Require domain expertise section
    pub require_domain_expertise: bool,
    /// Default tools for agents
    pub default_tools: Vec<String>,
    /// Model mapping by role patterns
    pub role_model_mapping: HashMap<String, AgentModel>,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        let mut role_model_mapping = HashMap::new();
        role_model_mapping.insert("architect".into(), AgentModel::Opus);
        role_model_mapping.insert("design".into(), AgentModel::Opus);
        role_model_mapping.insert("debug".into(), AgentModel::Sonnet);
        role_model_mapping.insert("review".into(), AgentModel::Sonnet);

        Self {
            min_evidence_refs: 3,
            require_domain_expertise: true,
            default_tools: vec!["Read".into(), "Glob".into(), "Grep".into()],
            role_model_mapping,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, Hash)]
#[serde(rename_all = "lowercase")]
pub enum AgentModel {
    Haiku,
    #[default]
    Sonnet,
    Opus,
}

impl AgentModel {
    pub fn model_id(&self) -> &'static str {
        match self {
            Self::Haiku => "claude-haiku-4-5-20251001",
            Self::Sonnet => "claude-sonnet-4-5-20250929",
            Self::Opus => "claude-opus-4-5-20251101",
        }
    }

    pub fn to_types_model(&self) -> crate::types::AgentModel {
        match self {
            Self::Haiku => crate::types::AgentModel::Haiku,
            Self::Sonnet => crate::types::AgentModel::Sonnet,
            Self::Opus => crate::types::AgentModel::Opus,
        }
    }
}

impl std::fmt::Display for AgentModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Haiku => write!(f, "haiku"),
            Self::Sonnet => write!(f, "sonnet"),
            Self::Opus => write!(f, "opus"),
        }
    }
}

// =============================================================================
// LLM CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// Balanced model for standard operations (Sonnet-class)
    /// This is the workhorse model for generation, refinement, review
    pub default_model: String,

    /// Performance model for critical high-intelligence tasks (Opus-class)
    /// Used for: constraint extraction, mistake discovery, deep analysis
    /// Industry terminology: highest capability tier
    pub performance_model: Option<String>,

    /// Fast model for quick, simple tasks (Haiku-class)
    /// Used for: classification, detection, validation
    /// Industry terminology: speed-optimized tier
    pub fast_model: Option<String>,

    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Temperature (0.0 = deterministic, 1.0 = creative)
    pub temperature: f32,
    /// Provider (claude-agent, openai)
    pub provider: String,
    /// Context configuration
    pub context: ContextWindowConfig,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            default_model: "claude-sonnet-4-5-20250929".into(),
            performance_model: None,
            fast_model: None,
            timeout_secs: 300,
            temperature: 0.0,
            provider: "claude-agent".into(),
            context: ContextWindowConfig::default(),
        }
    }
}

/// Context window configuration for managing token limits
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextWindowConfig {
    /// Authentication mode (api_key or oauth)
    /// OAuth (Claude Code CLI) doesn't support extended context
    pub auth_mode: AuthModeConfig,
    /// Whether to use extended context (1M) if available
    /// Only works with API key authentication
    pub use_extended_context: bool,
    /// Override context window size (tokens)
    /// If set, ignores model defaults and auth mode restrictions
    pub override_window_size: Option<u64>,
    /// Ratio of context window for input vs output (0.0-1.0)
    pub input_ratio: f32,
    /// Safety margin tokens reserved for overhead
    pub safety_margin_tokens: u64,
    /// Maximum tokens per batch (if unset, calculated from window size)
    pub max_batch_tokens: Option<u64>,
}

impl Default for ContextWindowConfig {
    fn default() -> Self {
        Self {
            auth_mode: AuthModeConfig::OAuth, // Default to OAuth (Claude Code CLI)
            use_extended_context: false,       // Disabled by default
            override_window_size: None,
            input_ratio: 0.90,
            safety_margin_tokens: 10_000,
            max_batch_tokens: None, // Calculated dynamically
        }
    }
}

impl ContextWindowConfig {
    /// Get effective context window size for a model
    pub fn effective_window_size(&self, model_id: &str) -> u64 {
        if let Some(override_size) = self.override_window_size {
            return override_size;
        }

        use crate::ai::model_capabilities::{AuthMode, ModelRegistry};

        let registry = ModelRegistry::global();
        let caps = registry.get_or_default(model_id);
        let auth_mode = match self.auth_mode {
            AuthModeConfig::ApiKey => AuthMode::ApiKey,
            AuthModeConfig::OAuth => AuthMode::OAuth,
        };

        caps.effective_context_window(auth_mode, self.use_extended_context)
    }

    /// Get effective maximum batch tokens
    pub fn effective_batch_tokens(&self, model_id: &str) -> u64 {
        if let Some(batch_tokens) = self.max_batch_tokens {
            return batch_tokens;
        }

        let window = self.effective_window_size(model_id);
        let available = (window as f32 * self.input_ratio) as u64 - self.safety_margin_tokens;

        // Batch ratio based on window size
        let batch_ratio = if window >= 500_000 {
            0.10
        } else if window >= 200_000 {
            0.25
        } else {
            0.30
        };

        (available as f32 * batch_ratio) as u64
    }

    /// Get available tokens for content
    pub fn available_for_content(&self, model_id: &str) -> u64 {
        let window = self.effective_window_size(model_id);
        let input_budget = (window as f32 * self.input_ratio) as u64;
        input_budget.saturating_sub(self.safety_margin_tokens)
    }
}

/// Authentication mode configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthModeConfig {
    /// API key authentication - supports all features
    ApiKey,
    /// OAuth authentication (Claude Code CLI) - limited features
    OAuth,
}

impl Default for AuthModeConfig {
    fn default() -> Self {
        Self::OAuth
    }
}

impl LlmConfig {
    pub fn performance_model(&self) -> &str {
        self.performance_model.as_deref().unwrap_or(&self.default_model)
    }

    pub fn fast_model(&self) -> &str {
        self.fast_model.as_deref().unwrap_or(&self.default_model)
    }

    pub fn model_for_phase(&self, phase: &str) -> &str {
        match phase {
            "project_detection" | "convention_inference" => self.fast_model(),
            "constraint_extraction" => self.performance_model(),
            "generation" | "verification" => &self.default_model,
            _ => &self.default_model,
        }
    }
}

// =============================================================================
// ANALYSIS CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisConfig {
    pub depth: AnalysisDepth,
    /// Glob patterns to include
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub max_file_size_bytes: usize,
    pub max_file_size: usize,
    pub max_file_samples: usize,
    /// Enable constraint detection (concurrency, init order, etc.)
    pub detect_constraints: bool,
    /// Enable business rule detection
    pub detect_business_rules: bool,
    pub deep_analysis: DeepAnalysisConfig,
    pub few_shots: FewShotConfig,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            depth: AnalysisDepth::Standard,
            include: vec!["**/*".into()],
            exclude: vec![
                ".git/**".into(),
                "target/**".into(),
                "dist/**".into(),
                "build/**".into(),
                "node_modules/**".into(),
                "vendor/**".into(),
                "__pycache__/**".into(),
                ".venv/**".into(),
                ".claudegen/**".into(),
            ],
            max_file_size_bytes: 5 * 1024 * 1024, // 5MB
            max_file_size: 5 * 1024 * 1024, // 5MB (alias)
            max_file_samples: 100,
            detect_constraints: true,
            detect_business_rules: true,
            deep_analysis: DeepAnalysisConfig::default(),
            few_shots: FewShotConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AnalysisDepth {
    Fast,
    #[default]
    Standard,
    Complete,
}

impl AnalysisDepth {
    pub fn max_files(&self) -> usize {
        match self {
            Self::Fast => 20,
            Self::Standard => 100,
            Self::Complete => usize::MAX,
        }
    }

    pub fn max_llm_calls(&self) -> usize {
        match self {
            Self::Fast => 15,
            Self::Standard => 50,
            Self::Complete => usize::MAX,
        }
    }

    pub fn enable_deep_analysis(&self) -> bool {
        matches!(self, Self::Standard | Self::Complete)
    }
}

// =============================================================================
// INSIGHT EXTRACTION CONFIG
// =============================================================================

/// Configuration for insight extraction components
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct InsightConfig {
    /// Mistake finder configuration
    pub mistakes: MistakeFinderConfig,
    /// Constraint detection configuration
    pub constraints: ConstraintDetectionConfig,
    /// Domain analysis configuration
    pub domain: DomainAnalysisConfig,
    /// Classification configuration
    pub classification: ClassificationConfig,
    /// Scoring configuration
    pub scoring: ScoringConfig,
}


/// Mistake finder configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MistakeFinderConfig {
    /// Minimum likelihood threshold to retain a mistake (0.0-1.0)
    pub min_likelihood: f32,
    /// Enable concurrency mistake detection
    pub detect_concurrency: bool,
    /// Enable initialization order detection
    pub detect_init_order: bool,
    /// Enable error handling mistake detection
    pub detect_error_handling: bool,
    /// Enable resource management mistake detection
    pub detect_resource_management: bool,
}

impl Default for MistakeFinderConfig {
    fn default() -> Self {
        Self {
            min_likelihood: 0.3,
            detect_concurrency: true,
            detect_init_order: true,
            detect_error_handling: true,
            detect_resource_management: true,
        }
    }
}

/// Constraint detection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConstraintDetectionConfig {
    /// Enable concurrency constraint detection
    pub detect_concurrency: bool,
    /// Enable initialization order detection
    pub detect_init_order: bool,
    /// Enable security constraint detection
    pub detect_security: bool,
    /// Enable boundary constraint detection
    pub detect_boundary: bool,
    /// Enable performance constraint detection
    pub detect_performance: bool,
    /// Minimum severity to retain constraints
    pub min_severity: ConstraintSeverity,
}

impl Default for ConstraintDetectionConfig {
    fn default() -> Self {
        Self {
            detect_concurrency: true,
            detect_init_order: true,
            detect_security: true,
            detect_boundary: true,
            detect_performance: true,
            min_severity: ConstraintSeverity::Low,
        }
    }
}

/// Constraint severity levels
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
#[serde(rename_all = "lowercase")]
pub enum ConstraintSeverity {
    #[default]
    Low,
    Medium,
    High,
    Critical,
}

/// Domain analysis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DomainAnalysisConfig {
    /// Minimum occurrences to consider a term domain-specific
    pub min_term_occurrences: usize,
    /// Maximum terminology entries to extract
    pub max_terminology: usize,
    /// Enable LLM enrichment for domain analysis
    pub llm_enrichment: bool,
    /// Business rule types to focus on
    pub rule_type_priorities: Vec<BusinessRuleType>,
}

impl Default for DomainAnalysisConfig {
    fn default() -> Self {
        Self {
            min_term_occurrences: 2,
            max_terminology: 50,
            llm_enrichment: true,
            rule_type_priorities: vec![
                BusinessRuleType::Validation,
                BusinessRuleType::StateTransition,
                BusinessRuleType::Authorization,
                BusinessRuleType::Calculation,
                BusinessRuleType::Policy,
            ],
        }
    }
}

/// Business rule types for domain analysis
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BusinessRuleType {
    Validation,
    StateTransition,
    Authorization,
    Calculation,
    Policy,
    Constraint,
}

/// Classification configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClassificationConfig {
    /// Similarity threshold for duplicate detection (0.0-1.0)
    pub duplicate_similarity_threshold: f32,
    /// Enable tier classification
    pub enable_tier_classification: bool,
    /// Enable artifact routing
    pub enable_artifact_routing: bool,
    /// Enable LLM-based classification (hybrid approach)
    pub enable_llm_classification: bool,
    /// Model to use for LLM classification (defaults to haiku for speed/cost)
    pub llm_model: String,
    /// Batch size for LLM classification
    pub llm_batch_size: usize,
    /// Confidence threshold below which to escalate to stronger model
    pub llm_confidence_threshold: f32,
    /// Fallback model when confidence is below threshold
    pub llm_fallback_model: String,
    /// Cache TTL in hours for classification results
    pub cache_ttl_hours: u64,
    /// Maximum cache entries
    pub cache_max_entries: usize,
}

impl Default for ClassificationConfig {
    fn default() -> Self {
        Self {
            duplicate_similarity_threshold: 0.7,
            enable_tier_classification: true,
            enable_artifact_routing: true,
            enable_llm_classification: true,
            llm_model: "claude-haiku-4-5-20251001".to_string(),
            llm_batch_size: 10,
            llm_confidence_threshold: 0.8,
            llm_fallback_model: "claude-sonnet-4-5-20250929".to_string(),
            cache_ttl_hours: 24,
            cache_max_entries: 1000,
        }
    }
}

/// Value scoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScoringConfig {
    /// Custom keyword bonuses for mistake prevention scoring
    pub mistake_prevention_keywords: HashMap<String, f32>,
    /// Custom keyword bonuses for discoverability scoring
    pub discoverability_keywords: HashMap<String, f32>,
    /// Severity to score bonus mapping
    pub severity_bonuses: SeverityBonuses,
    /// Evidence bonus per reference
    pub evidence_bonus_per_ref: f32,
    /// Maximum evidence bonus
    pub max_evidence_bonus: f32,
    /// Category-based scores for mistake prevention
    pub category_scores: CategoryScores,
    /// Bonus for having prevention info
    pub prevention_info_bonus: f32,
    /// Source-based scores for discoverability
    pub source_scores: SourceScores,
    /// Category adjustments for discoverability
    pub category_adjustments: CategoryAdjustments,
    /// Artifact fitness configuration
    pub artifact_fitness: ArtifactFitnessConfig,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        let mut mistake_keywords = HashMap::new();
        mistake_keywords.insert("security".into(), 0.3);
        mistake_keywords.insert("vulnerability".into(), 0.35);
        mistake_keywords.insert("critical".into(), 0.25);
        mistake_keywords.insert("must".into(), 0.2);
        mistake_keywords.insert("never".into(), 0.2);
        mistake_keywords.insert("required".into(), 0.15);

        let mut discover_keywords = HashMap::new();
        discover_keywords.insert("hidden".into(), 0.3);
        discover_keywords.insert("gotcha".into(), 0.35);
        discover_keywords.insert("pitfall".into(), 0.3);
        discover_keywords.insert("subtle".into(), 0.25);
        discover_keywords.insert("non-obvious".into(), 0.25);

        Self {
            mistake_prevention_keywords: mistake_keywords,
            discoverability_keywords: discover_keywords,
            severity_bonuses: SeverityBonuses::default(),
            evidence_bonus_per_ref: 0.03,
            max_evidence_bonus: 0.15,
            category_scores: CategoryScores::default(),
            prevention_info_bonus: 0.15,
            source_scores: SourceScores::default(),
            category_adjustments: CategoryAdjustments::default(),
            artifact_fitness: ArtifactFitnessConfig::default(),
        }
    }
}

/// Severity to score bonus mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SeverityBonuses {
    pub critical: f32,
    pub high: f32,
    pub medium: f32,
    pub low: f32,
}

impl Default for SeverityBonuses {
    fn default() -> Self {
        Self {
            critical: 0.3,
            high: 0.2,
            medium: 0.1,
            low: 0.05,
        }
    }
}

/// Category-based scores for mistake prevention
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CategoryScores {
    pub security_constraint: f32,
    pub technical_constraint: f32,
    pub compliance: f32,
    pub business_rule: f32,
    pub gotcha: f32,
    pub performance_constraint: f32,
    pub architecture_intent: f32,
    pub domain_knowledge: f32,
}

impl Default for CategoryScores {
    fn default() -> Self {
        Self {
            security_constraint: 0.4,
            technical_constraint: 0.3,
            compliance: 0.4,
            business_rule: 0.25,
            gotcha: 0.35,
            performance_constraint: 0.2,
            architecture_intent: 0.15,
            domain_knowledge: 0.1,
        }
    }
}

/// Source-based scores for discoverability
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceScores {
    pub domain_analysis: f32,
    pub mistake_analysis: f32,
    pub constraint_detection: f32,
    pub pattern_mining: f32,
    pub manual_annotation: f32,
}

impl Default for SourceScores {
    fn default() -> Self {
        Self {
            domain_analysis: 0.2,
            mistake_analysis: 0.15,
            constraint_detection: 0.1,
            pattern_mining: -0.1,
            manual_annotation: 0.25,
        }
    }
}

/// Category adjustments for discoverability
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CategoryAdjustments {
    pub gotcha: f32,
    pub domain_knowledge: f32,
    pub business_rule: f32,
    pub architecture_intent: f32,
}

impl Default for CategoryAdjustments {
    fn default() -> Self {
        Self {
            gotcha: 0.2,
            domain_knowledge: 0.15,
            business_rule: 0.1,
            architecture_intent: -0.05,
        }
    }
}

/// Artifact fitness configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtifactFitnessConfig {
    /// Bonus for having evidence
    pub evidence_bonus: f32,
    /// Bonus for having prevention info
    pub prevention_info_bonus: f32,
    /// Category-specific fitness bonuses
    pub constraint_category_bonus: f32,
    pub domain_category_bonus: f32,
    pub architecture_bonus: f32,
    pub gotcha_bonus: f32,
    /// Content length thresholds
    pub min_length_bonus_threshold: usize,
    pub extended_length_bonus_threshold: usize,
    pub length_bonus: f32,
}

impl Default for ArtifactFitnessConfig {
    fn default() -> Self {
        Self {
            evidence_bonus: 0.2,
            prevention_info_bonus: 0.15,
            constraint_category_bonus: 0.15,
            domain_category_bonus: 0.1,
            architecture_bonus: 0.1,
            gotcha_bonus: 0.12,
            min_length_bonus_threshold: 50,
            extended_length_bonus_threshold: 100,
            length_bonus: 0.05,
        }
    }
}

// =============================================================================
// BUDGET CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BudgetConfig {
    /// Total token budget
    pub total_tokens: u64,
    /// Budget allocation per phase
    pub allocation: BudgetAllocation,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            total_tokens: 2_000_000,
            allocation: BudgetAllocation::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BudgetAllocation {
    /// Fraction for understanding phase
    pub understanding: f32,
    /// Fraction for insight extraction
    pub insight_extraction: f32,
    /// Fraction for artifact generation
    pub generation: f32,
    /// Fraction for validation
    pub validation: f32,
    /// Fraction for refinement
    pub refinement: f32,
}

impl Default for BudgetAllocation {
    fn default() -> Self {
        Self {
            understanding: 0.15,
            insight_extraction: 0.25,
            generation: 0.30,
            validation: 0.20,
            refinement: 0.10,
        }
    }
}

impl BudgetAllocation {
    pub fn tokens_for_phase(&self, total: u64, phase: &str) -> u64 {
        let fraction = match phase {
            "understanding" => self.understanding,
            "insight_extraction" => self.insight_extraction,
            "generation" => self.generation,
            "validation" => self.validation,
            "refinement" => self.refinement,
            _ => 0.1, // Unknown phases get 10%
        };
        (total as f64 * fraction as f64) as u64
    }
}

// =============================================================================
// PERFORMANCE CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformanceConfig {
    /// Parallel workers for analysis
    pub parallel_workers: usize,
    /// Checkpoint interval in minutes
    pub checkpoint_interval_minutes: u64,
    /// Maximum runtime in hours
    pub max_runtime_hours: u64,
    /// Resume from checkpoint on crash
    pub resume_on_crash: bool,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            parallel_workers: 4,
            checkpoint_interval_minutes: 5,
            max_runtime_hours: 168, // 1 week
            resume_on_crash: true,
        }
    }
}

// =============================================================================
// PROJECT CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    pub name: Option<String>,
    pub project_type: ProjectType,
    pub root: Option<std::path::PathBuf>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: None,
            project_type: ProjectType::Auto,
            root: None,
        }
    }
}

// =============================================================================
// OUTPUT CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct OutputConfig {
    pub dir: Option<std::path::PathBuf>,
    pub agents: OutputAgentConfig,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputAgentConfig {
    pub max_agents: usize,
    pub min_evidence_refs: usize,
    pub require_domain_expertise: bool,
    pub default_tools: Vec<String>,
    /// Default model for agents
    pub default_model: AgentModelType,
    /// Per-agent overrides by name
    pub overrides: std::collections::HashMap<String, AgentOverride>,
    /// Role-based model/tool mappings
    pub role_mappings: Vec<RoleMapping>,
    /// Tool sets for different roles
    pub tools: AgentToolsets,
}

impl Default for OutputAgentConfig {
    fn default() -> Self {
        Self {
            max_agents: 10,
            min_evidence_refs: 3,
            require_domain_expertise: true,
            default_tools: vec!["Read".into(), "Glob".into(), "Grep".into()],
            default_model: AgentModelType::default(),
            overrides: std::collections::HashMap::new(),
            role_mappings: vec![],
            tools: AgentToolsets::default(),
        }
    }
}

/// Agent model type for configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentModelType {
    Haiku,
    #[default]
    Sonnet,
    Opus,
}

impl AgentModelType {
    pub fn to_agent_model(&self) -> crate::types::AgentModel {
        match self {
            Self::Haiku => crate::types::AgentModel::Haiku,
            Self::Sonnet => crate::types::AgentModel::Sonnet,
            Self::Opus => crate::types::AgentModel::Opus,
        }
    }
}

/// Per-agent configuration override
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AgentOverride {
    pub model: Option<AgentModelType>,
    pub tools: Option<Vec<String>>,
}

/// Role-based mapping for agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleMapping {
    /// Patterns to match in role name
    pub patterns: Vec<String>,
    /// Model to use for matching roles
    pub model: AgentModelType,
}

/// Tool sets for different agent roles
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentToolsets {
    pub default: Vec<String>,
    pub debug: Vec<String>,
    pub architect: Vec<String>,
    pub coordinator: Vec<String>,
}

impl Default for AgentToolsets {
    fn default() -> Self {
        Self {
            default: vec!["Read".into(), "Glob".into(), "Grep".into()],
            debug: vec!["Read".into(), "Glob".into(), "Grep".into(), "Bash".into()],
            architect: vec!["Read".into(), "Glob".into(), "Grep".into(), "Task".into()],
            coordinator: vec!["Read".into(), "Glob".into(), "Grep".into(), "Task".into()],
        }
    }
}

// =============================================================================
// PROJECT TYPE (used elsewhere in codebase)
// =============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ProjectType {
    Cli,
    Library,
    Backend,
    Frontend,
    Monorepo,
    Agent,
    Hybrid,
    #[default]
    Auto,
}

impl ProjectType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Library => "library",
            Self::Backend => "backend",
            Self::Frontend => "frontend",
            Self::Monorepo => "monorepo",
            Self::Agent => "agent",
            Self::Hybrid => "hybrid",
            Self::Auto => "auto",
        }
    }

    pub fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }
}

impl std::fmt::Display for ProjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// NETWORK & RETRY CONFIG (for AI providers)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub timeout_ms: u64,
    pub connect_timeout_ms: u64,
    pub max_retry_timeout_ms: u64,
    pub analysis_phase_timeout_secs: u64,
    pub generation_phase_timeout_secs: u64,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 300_000,
            connect_timeout_ms: 30_000,
            max_retry_timeout_ms: 600_000,
            analysis_phase_timeout_secs: 600,
            generation_phase_timeout_secs: 300,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RetryConfig {
    pub max_retries: usize,
    pub max_total_attempts: usize,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub max_delay_secs: u64,
    pub backoff_factor: f32,
    pub rate_limit_fallback_secs: u64,
    pub rate_limit_delay_secs: u64,
    pub network_retry_delay_secs: u64,
    pub transient_retry_delay_secs: u64,
    pub parse_error_retry_delay_secs: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            max_total_attempts: 10,
            base_delay_ms: 1000,
            max_delay_ms: 60_000,
            max_delay_secs: 60,
            backoff_factor: 2.0,
            rate_limit_fallback_secs: 30,
            rate_limit_delay_secs: 30,
            network_retry_delay_secs: 5,
            transient_retry_delay_secs: 2,
            parse_error_retry_delay_secs: 1,
        }
    }
}

impl RetryConfig {
    pub fn delay_for_attempt(&self, attempt: usize) -> std::time::Duration {
        let delay = self.base_delay_ms as f64 * (self.backoff_factor as f64).powi(attempt as i32);
        let capped = delay.min(self.max_delay_ms as f64);
        std::time::Duration::from_millis(capped as u64)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: usize,
    pub recovery_timeout_secs: u64,
    pub half_open_max_calls: usize,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout_secs: 60,
            half_open_max_calls: 3,
        }
    }
}

// =============================================================================
// REFINEMENT CONFIG
// =============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RefinementStrategyType {
    Semantic,
    Evidence,
    Regeneration,
}

impl RefinementStrategyType {
    pub fn all() -> Vec<Self> {
        vec![Self::Semantic, Self::Evidence, Self::Regeneration]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RefinementConfig {
    pub enabled_strategies: Vec<RefinementStrategyType>,
    pub max_attempts_per_strategy: usize,
    pub strategy_rotation_enabled: bool,
    pub timeout_secs: u64,
    pub max_iterations: usize,
    pub min_iterations: usize,
    pub stagnation_patience: usize,
    pub stagnation_threshold: f32,
    pub require_all_dimensions: bool,
    pub issues_per_iteration: usize,
    pub strategy_retry_limit: usize,
    pub oscillation_strict_passes: usize,
    pub oscillation_lenient_passes: usize,
    pub oscillation_stability_variance: f32,
    pub oscillation_variance_window: usize,
    pub enable_rollback: bool,
    pub rollback_threshold: f32,
    pub max_rollbacks: usize,
    pub post_convergence_verification: bool,
    pub post_convergence_passes: usize,
    pub max_convergence_detections: usize,
    pub dimension_thresholds: DimensionThresholds,
    pub min_improvement_per_iteration: f32,
    pub detect_oscillation: bool,
    pub oscillation_window: usize,
    pub oscillation_min_amplitude: f32,
    /// Minimum quality improvement to accept a regeneration
    pub quality_acceptance_delta: f32,
}

impl Default for RefinementConfig {
    fn default() -> Self {
        Self {
            enabled_strategies: RefinementStrategyType::all(),
            max_attempts_per_strategy: 3,
            strategy_rotation_enabled: true,
            timeout_secs: 600,
            max_iterations: 30,
            min_iterations: 3,
            stagnation_patience: 5,
            stagnation_threshold: 0.01,
            require_all_dimensions: false,
            issues_per_iteration: 5,
            strategy_retry_limit: 3,
            oscillation_strict_passes: 3,
            oscillation_lenient_passes: 2,
            oscillation_stability_variance: 0.03,
            oscillation_variance_window: 3,
            enable_rollback: true,
            rollback_threshold: 0.1,
            max_rollbacks: 3,
            post_convergence_verification: true,
            post_convergence_passes: 2,
            max_convergence_detections: 3,
            dimension_thresholds: DimensionThresholds::default(),
            min_improvement_per_iteration: 0.01,
            detect_oscillation: true,
            oscillation_window: 3,
            oscillation_min_amplitude: 0.03,
            quality_acceptance_delta: 0.05,
        }
    }
}

// =============================================================================
// LEARNING CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LearningConfig {
    pub enabled: bool,
    pub max_patterns: usize,
    pub pattern_threshold: f32,
    pub persist_to_disk: bool,
    pub quality_thresholds: LearningQualityThresholds,
    pub min_improvement_for_pattern: f32,
    pub recommend_success_threshold: f32,
    pub recommend_min_samples: usize,
    pub fallback_success_threshold: f32,
    pub failing_strategy_min_attempts: usize,
    pub failing_strategy_threshold: f32,
    pub escalation_threshold: f32,
    /// Maximum iterations to store per strategy
    pub max_stored_iterations: usize,
    /// Maximum outcomes to track per strategy
    pub max_outcomes_per_strategy: usize,
    /// Window size for escalation detection
    pub escalation_window: usize,
}

impl Default for LearningConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_patterns: 100,
            pattern_threshold: 0.7,
            persist_to_disk: true,
            quality_thresholds: LearningQualityThresholds::default(),
            min_improvement_for_pattern: 0.05,
            recommend_success_threshold: 0.7,
            recommend_min_samples: 3,
            fallback_success_threshold: 0.5,
            failing_strategy_min_attempts: 5,
            failing_strategy_threshold: 0.3,
            escalation_threshold: 0.8,
            max_stored_iterations: 50,
            max_outcomes_per_strategy: 100,
            escalation_window: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LearningQualityThresholds {
    pub excellent: f32,
    pub good: f32,
    pub acceptable: f32,
    pub poor: f32,
}

impl Default for LearningQualityThresholds {
    fn default() -> Self {
        Self {
            excellent: 0.9,
            good: 0.7,
            acceptable: 0.5,
            poor: 0.3,
        }
    }
}

impl LearningQualityThresholds {
    /// Convert to array format for QualityRange
    pub fn as_array(&self) -> [f32; 4] {
        [self.poor, self.acceptable, self.good, self.excellent]
    }
}

// =============================================================================
// QUALITY CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct QualityConfig {
    pub enabled: bool,
    pub min_score: f32,
    pub min_value_score: f32,
    pub target_score: f32,
    pub minimum_quality: f32,
    pub min_file_refs: usize,
    pub min_actionable_count: usize,
    pub max_tier1_ratio: f32,
    pub reference_only_mode: bool,
    pub scoring: TierScoringWeights,
    pub skill: SkillQualityConfig,
    pub agent: AgentQualityConfig,
    pub memory: MemoryQualityConfig,
    pub rule: RuleQualityConfig,
    pub project_specific: ProjectSpecificQuality,
    pub min_overall_score: f32,
    pub semantic: SemanticQualityThresholds,
}

impl Default for QualityConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_score: 0.5,
            min_value_score: 0.5,
            target_score: 0.8,
            minimum_quality: 0.5,
            min_file_refs: 2,
            min_actionable_count: 3,
            max_tier1_ratio: 0.2,
            reference_only_mode: false,
            scoring: TierScoringWeights::default(),
            skill: SkillQualityConfig::default(),
            agent: AgentQualityConfig::default(),
            memory: MemoryQualityConfig::default(),
            rule: RuleQualityConfig::default(),
            project_specific: ProjectSpecificQuality::default(),
            min_overall_score: 0.6,
            semantic: SemanticQualityThresholds::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SemanticQualityThresholds {
    pub min_actionability: f32,
    pub min_specificity: f32,
    pub min_evidence_quality: f32,
    pub min_depth: f32,
    pub max_redundancy: f32,
}

impl Default for SemanticQualityThresholds {
    fn default() -> Self {
        Self {
            min_actionability: 0.5,
            min_specificity: 0.5,
            min_evidence_quality: 0.5,
            min_depth: 0.5,
            max_redundancy: 0.3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryQualityConfig {
    pub min_evidence_refs: usize,
    pub min_score: f32,
    pub min_file_refs: usize,
    pub min_chars: usize,
    pub min_sections: usize,
    pub target_file_refs: usize,
}

impl Default for MemoryQualityConfig {
    fn default() -> Self {
        Self {
            min_evidence_refs: 2,
            min_score: 0.6,
            min_file_refs: 1,
            min_chars: 300,
            min_sections: 2,
            target_file_refs: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RuleQualityConfig {
    pub min_evidence_refs: usize,
    pub min_score: f32,
    pub min_file_refs: usize,
}

impl Default for RuleQualityConfig {
    fn default() -> Self {
        Self {
            min_evidence_refs: 1,
            min_score: 0.6,
            min_file_refs: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct ProjectSpecificQuality {
    pub cli: ProjectTypeQuality,
    pub library: ProjectTypeQuality,
    pub backend: ProjectTypeQuality,
    pub frontend: ProjectTypeQuality,
    pub monorepo: ProjectTypeQuality,
    pub agent: ProjectTypeQuality,
    pub hybrid: ProjectTypeQuality,
    pub auto: ProjectTypeQuality,
}


impl ProjectSpecificQuality {
    pub fn get_for_type(&self, project_type: ProjectType) -> &ProjectTypeQuality {
        match project_type {
            ProjectType::Cli => &self.cli,
            ProjectType::Library => &self.library,
            ProjectType::Backend => &self.backend,
            ProjectType::Frontend => &self.frontend,
            ProjectType::Monorepo => &self.monorepo,
            ProjectType::Agent => &self.agent,
            ProjectType::Hybrid => &self.hybrid,
            ProjectType::Auto => &self.auto,
        }
    }
}

// =============================================================================
// DEEP REVIEW CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DeepReviewConfig {
    pub enabled: bool,
    pub max_attempts: u32,
    pub required_passes: u32,
    pub review_timeout_secs: u64,
    pub check_regression: bool,
    pub reject_tier1: bool,
    pub min_evidence_ratio: f32,
}

impl Default for DeepReviewConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_attempts: 3,
            required_passes: 2,
            review_timeout_secs: 300,
            check_regression: true,
            reject_tier1: true,
            min_evidence_ratio: 0.5,
        }
    }
}

// =============================================================================
// VALIDATION CONFIG - Multi-Layer Validation System
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ValidationConfig {
    pub enabled: bool,
    pub layers: ValidationLayerConfigs,
    pub clean_pass: CleanPassConfig,
    pub semantic_context: SemanticContextValidationConfig,
    pub value_assessment: ValueAssessmentConfig,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            layers: ValidationLayerConfigs::default(),
            clean_pass: CleanPassConfig::default(),
            semantic_context: SemanticContextValidationConfig::default(),
            value_assessment: ValueAssessmentConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ValidationLayerConfigs {
    pub format_enabled: bool,
    pub evidence_enabled: bool,
    pub semantic_context_enabled: bool,
    pub value_assessment_enabled: bool,
    pub cross_artifact_enabled: bool,
    pub early_exit_on_critical: bool,
}

impl Default for ValidationLayerConfigs {
    fn default() -> Self {
        Self {
            format_enabled: true,
            evidence_enabled: true,
            semantic_context_enabled: true,
            value_assessment_enabled: true,
            cross_artifact_enabled: true,
            early_exit_on_critical: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CleanPassConfig {
    pub require_zero_issues: bool,
    pub consecutive_passes: usize,
    pub max_attempts: usize,
    pub reset_on_error: bool,
    pub reset_on_critical: bool,
}

impl Default for CleanPassConfig {
    fn default() -> Self {
        Self {
            require_zero_issues: true,
            consecutive_passes: 2,
            max_attempts: 10,
            reset_on_error: true,
            reset_on_critical: true,
        }
    }
}

impl CleanPassConfig {
    /// Build reset severities vector from config flags
    pub fn reset_severities(&self) -> Vec<crate::pipeline::validation::LayerIssueSeverity> {
        use crate::pipeline::validation::LayerIssueSeverity;
        let mut severities = Vec::new();
        if self.reset_on_critical {
            severities.push(LayerIssueSeverity::Critical);
        }
        if self.reset_on_error {
            severities.push(LayerIssueSeverity::Error);
        }
        severities
    }

    pub fn for_preset(preset: ConfigPreset) -> Self {
        match preset {
            ConfigPreset::Quick => Self {
                consecutive_passes: 1,
                max_attempts: 5,
                ..Default::default()
            },
            ConfigPreset::Standard => Self::default(),
            ConfigPreset::Thorough => Self::default(),
            ConfigPreset::Exhaustive => Self {
                consecutive_passes: 3,
                max_attempts: 15,
                ..Default::default()
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SemanticContextValidationConfig {
    pub enabled: bool,
    pub context_lines: usize,
    pub min_similarity: f32,
    pub max_refs_per_artifact: usize,
    pub cache_context: bool,
}

impl Default for SemanticContextValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            context_lines: 5,
            min_similarity: 0.7,
            max_refs_per_artifact: 10,
            cache_context: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ValueAssessmentConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub reject_tier1: bool,
    #[serde(default = "default_min_mistake_prevention")]
    pub min_mistake_prevention: f32,
    #[serde(default = "default_min_discoverability")]
    pub min_discoverability: f32,
    #[serde(default = "default_true")]
    pub use_few_shot: bool,
    #[serde(default = "default_few_shot_count")]
    pub few_shot_examples_count: usize,
}

fn default_true() -> bool {
    true
}
fn default_min_mistake_prevention() -> f32 {
    0.4
}
fn default_min_discoverability() -> f32 {
    0.3
}
fn default_few_shot_count() -> usize {
    3
}

// =============================================================================
// SEMANTIC VALIDATION CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SemanticValidationConfig {
    pub enabled: bool,
    pub use_ai_validation: bool,
    pub dimension_weights: SemanticDimensionWeights,
    pub min_overall_score: f32,
    pub weights: SemanticWeights,
    pub thresholds: SemanticThresholds,
    pub min_actionability: f32,
    pub min_specificity: f32,
    pub min_evidence_quality: f32,
    pub max_redundancy: f32,
    pub min_depth: f32,
    pub reject_generic_content: bool,
    pub require_file_line_refs: bool,
    pub min_actionable_items: usize,
}

impl Default for SemanticValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            use_ai_validation: false,
            dimension_weights: SemanticDimensionWeights::default(),
            min_overall_score: 0.6,
            weights: SemanticWeights::default(),
            thresholds: SemanticThresholds::default(),
            min_actionability: 0.5,
            min_specificity: 0.5,
            min_evidence_quality: 0.5,
            max_redundancy: 0.3,
            min_depth: 0.5,
            reject_generic_content: true,
            require_file_line_refs: false,
            min_actionable_items: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SemanticWeights {
    pub actionability: f32,
    pub specificity: f32,
    pub evidence: f32,
    pub redundancy: f32,
    pub depth: f32,
}

impl Default for SemanticWeights {
    fn default() -> Self {
        Self {
            actionability: 0.25,
            specificity: 0.25,
            evidence: 0.20,
            redundancy: 0.15,
            depth: 0.15,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SemanticThresholds {
    pub low_actionability_multiplier: f32,
    pub medium_specificity_threshold: f32,
    pub high_specificity_threshold: f32,
    pub evidence_base_score: f32,
    pub quantity_score_multiplier: f32,
    pub line_ref_bonus: f32,
    pub high_redundancy_threshold: f32,
    pub max_redundancy_ratio: f32,
    pub complexity_weight: f32,
    pub section_weight: f32,
    pub reference_weight: f32,
    pub depth_min_score: f32,
    pub depth_complexity_bonus: f32,
    // Fields for semantic_validator.rs
    pub min_line_length_actionability: usize,
    pub min_line_length_specificity: usize,
    pub max_generic_penalty: f32,
    pub file_line_bonus: f32,
    pub validity_score_weight: f32,
    pub quantity_score_weight: f32,
    pub overlap_threshold: f32,
    pub min_phrase_line_length: usize,
    pub max_phrase_words: usize,
    pub min_substantive_line_length: usize,
    pub min_substantive_lines_deep: usize,
    pub min_depth_indicators: usize,
    pub min_total_lines_shallow_check: usize,
    pub default_depth_score: f32,
    pub low_actionability_suggestion_threshold: usize,
    pub too_generic_suggestion_threshold: usize,
    pub redundant_suggestion_threshold: usize,
    pub shallow_suggestion_threshold: usize,
}

impl Default for SemanticThresholds {
    fn default() -> Self {
        Self {
            low_actionability_multiplier: 0.8,
            medium_specificity_threshold: 0.5,
            high_specificity_threshold: 0.8,
            evidence_base_score: 0.3,
            quantity_score_multiplier: 2.0,
            line_ref_bonus: 0.2,
            high_redundancy_threshold: 0.5,
            max_redundancy_ratio: 0.3,
            complexity_weight: 0.3,
            section_weight: 0.3,
            reference_weight: 0.4,
            depth_min_score: 0.5,
            depth_complexity_bonus: 0.1,
            // Defaults for semantic_validator.rs
            min_line_length_actionability: 10,
            min_line_length_specificity: 10,
            max_generic_penalty: 0.3,
            file_line_bonus: 0.2,
            validity_score_weight: 0.4,
            quantity_score_weight: 0.3,
            overlap_threshold: 0.5,
            min_phrase_line_length: 15,
            max_phrase_words: 5,
            min_substantive_line_length: 20,
            min_substantive_lines_deep: 3,
            min_depth_indicators: 2,
            min_total_lines_shallow_check: 5,
            default_depth_score: 0.5,
            low_actionability_suggestion_threshold: 2,
            too_generic_suggestion_threshold: 2,
            redundant_suggestion_threshold: 1,
            shallow_suggestion_threshold: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SemanticDimensionWeights {
    pub specificity: f32,
    pub actionability: f32,
    pub evidence: f32,
    pub uniqueness: f32,
    pub redundancy: f32,
    pub depth: f32,
    pub min_substantive_lines_deep: usize,
    pub min_line_length_actionability: usize,
}

impl Default for SemanticDimensionWeights {
    fn default() -> Self {
        Self {
            specificity: 0.3,
            actionability: 0.3,
            evidence: 0.25,
            uniqueness: 0.15,
            redundancy: 0.1,
            depth: 0.2,
            min_substantive_lines_deep: 3,
            min_line_length_actionability: 20,
        }
    }
}

// =============================================================================
// EXECUTION CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExecutionConfig {
    pub parallel_workers: usize,
    pub checkpoint_enabled: bool,
    pub checkpoint_interval_secs: u64,
    pub checkpoint_interval_minutes: u64,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            parallel_workers: 4,
            checkpoint_enabled: true,
            checkpoint_interval_secs: 300,
            checkpoint_interval_minutes: 5,
        }
    }
}

// =============================================================================
// ANALYSIS SPECIALTY
// =============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisSpecialty {
    Architecture,
    Security,
    Performance,
    Testing,
    Documentation,
    Domain,
    Structure,
    Pattern,
    Constraint,
}

impl AnalysisSpecialty {
    pub fn all() -> Vec<Self> {
        vec![
            Self::Architecture,
            Self::Security,
            Self::Performance,
            Self::Testing,
            Self::Documentation,
            Self::Domain,
            Self::Structure,
            Self::Pattern,
            Self::Constraint,
        ]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Architecture => "architecture",
            Self::Security => "security",
            Self::Performance => "performance",
            Self::Testing => "testing",
            Self::Documentation => "documentation",
            Self::Domain => "domain",
            Self::Structure => "structure",
            Self::Pattern => "pattern",
            Self::Constraint => "constraint",
        }
    }
}

// =============================================================================
// MULTI-AGENT CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MultiAgentConfig {
    pub enabled: bool,
    pub specialists: Vec<AnalysisSpecialty>,
    pub enabled_specialists: Vec<AnalysisSpecialty>,
    pub parallel_execution: bool,
    pub conflict_resolution: ConflictResolution,
    pub cross_validate_specialists: bool,
    pub synthesis_retries: usize,
}

impl Default for MultiAgentConfig {
    fn default() -> Self {
        let default_specialists = vec![
            AnalysisSpecialty::Architecture,
            AnalysisSpecialty::Security,
            AnalysisSpecialty::Domain,
        ];
        Self {
            enabled: true,
            specialists: default_specialists.clone(),
            enabled_specialists: default_specialists,
            parallel_execution: true,
            conflict_resolution: ConflictResolution::WeightedVote,
            cross_validate_specialists: true,
            synthesis_retries: 3,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConflictResolution {
    #[default]
    WeightedVote,
    FirstWins,
    Merge,
}

// =============================================================================
// CROSS VALIDATION CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CrossValidationConfig {
    pub enabled: bool,
    pub check_consistency: bool,
    pub check_coverage: bool,
    pub min_overlap_score: f32,
    pub plan_output_consistency: bool,
    pub evidence_traceability: bool,
    pub artifact_consistency: bool,
}

impl Default for CrossValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_consistency: true,
            check_coverage: true,
            min_overlap_score: 0.5,
            plan_output_consistency: true,
            evidence_traceability: true,
            artifact_consistency: true,
        }
    }
}

// =============================================================================
// USABILITY CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UsabilityConfig {
    pub enabled: bool,
    pub check_readability: bool,
    pub check_completeness: bool,
    pub max_complexity_score: f32,
    pub min_usability_score: f32,
    pub max_context_tokens: usize,
    pub thresholds: UsabilityThresholds,
}

impl Default for UsabilityConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_readability: true,
            check_completeness: true,
            max_complexity_score: 0.7,
            min_usability_score: 0.7,
            // 0 = auto-compute from ContextConfig based on model
            max_context_tokens: 0,
            thresholds: UsabilityThresholds::default(),
        }
    }
}

impl UsabilityConfig {
    /// Get effective max context tokens (auto-compute if 0)
    pub fn effective_max_context_tokens(&self, model_id: &str, context_config: &ContextWindowConfig) -> usize {
        if self.max_context_tokens > 0 {
            self.max_context_tokens
        } else {
            // Use ~25% of available context for artifact content validation
            (context_config.available_for_content(model_id) as f32 * 0.25) as usize
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UsabilityThresholds {
    pub progressive_disclosure_weight: f32,
    pub context_efficiency_weight: f32,
    pub task_relevance_weight: f32,
    pub min_entry_point_length: usize,
    pub max_entry_point_length: usize,
    pub min_unique_content_ratio: f32,
    pub min_valuable_rules_ratio: f32,
    pub min_word_length_for_uniqueness: usize,
    pub min_actionable_ratio: f32,
    pub min_description_length: usize,
    pub min_clear_scope_ratio: f32,
    pub tokens_per_word_estimate: f32,
    pub token_score_weight: f32,
    pub redundancy_score_weight: f32,
    pub essential_ratio_weight: f32,
}

impl Default for UsabilityThresholds {
    fn default() -> Self {
        Self {
            progressive_disclosure_weight: 0.4,
            context_efficiency_weight: 0.3,
            task_relevance_weight: 0.3,
            min_entry_point_length: 200,
            max_entry_point_length: 10000,
            min_unique_content_ratio: 0.3,
            min_valuable_rules_ratio: 0.5,
            min_word_length_for_uniqueness: 4,
            min_actionable_ratio: 0.5,
            min_description_length: 20,
            min_clear_scope_ratio: 0.5,
            tokens_per_word_estimate: 1.3,
            token_score_weight: 0.4,
            redundancy_score_weight: 0.3,
            essential_ratio_weight: 0.3,
        }
    }
}

// =============================================================================
// QUALITY LOOP CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct QualityLoopConfig {
    pub enabled: bool,
    pub max_iterations: usize,
    pub max_outer_iterations: usize,
    pub target_score: f32,
    pub min_improvement: f32,
    pub multi_agent: MultiAgentConfig,
    pub insight_driven: InsightDrivenGenConfig,
    pub analysis_confidence_threshold: f32,
    pub synthesis_confidence_threshold: f32,
    pub max_invalid_reference_ratio: f32,
    pub reanalysis_gap_threshold: f32,
}

impl Default for QualityLoopConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_iterations: 10,
            max_outer_iterations: 5,
            target_score: 0.8,
            min_improvement: 0.02,
            multi_agent: MultiAgentConfig::default(),
            insight_driven: InsightDrivenGenConfig::default(),
            analysis_confidence_threshold: 0.7,
            synthesis_confidence_threshold: 0.8,
            max_invalid_reference_ratio: 0.2,
            reanalysis_gap_threshold: 0.3,
        }
    }
}

// =============================================================================
// INSIGHT DRIVEN GENERATION CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InsightDrivenGenConfig {
    pub enabled: bool,
    pub insight_weight: f32,
    pub max_insights_per_artifact: usize,
    pub use_llm_decisions: bool,
    pub self_review_enabled: bool,
    pub max_review_iterations: usize,
    pub review_acceptance_threshold: f32,
    pub min_value_score: f32,
    pub max_inline_code_lines: usize,
    pub reference_only_mode: bool,
    pub min_file_refs: usize,
    pub min_actionable_statements: usize,
}

impl Default for InsightDrivenGenConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            insight_weight: 0.7,
            max_insights_per_artifact: 10,
            use_llm_decisions: true,
            self_review_enabled: true,
            max_review_iterations: 3,
            review_acceptance_threshold: 0.7,
            min_value_score: 0.5,
            max_inline_code_lines: 50,
            reference_only_mode: false,
            min_file_refs: 2,
            min_actionable_statements: 3,
        }
    }
}

// =============================================================================
// AGENT GENERATION CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentGenerationConfig {
    pub max_agents: usize,
    pub min_evidence_refs: usize,
    pub require_domain_expertise: bool,
    pub default_tools: Vec<String>,
}

impl Default for AgentGenerationConfig {
    fn default() -> Self {
        Self {
            max_agents: 10,
            min_evidence_refs: 3,
            require_domain_expertise: true,
            default_tools: vec!["Read".into(), "Glob".into(), "Grep".into()],
        }
    }
}

/// Runtime config for a specific phase provider
#[derive(Debug, Clone)]
pub struct PhaseProviderConfig {
    pub model: String,
    pub timeout_secs: u64,
}

impl PhaseProviderConfig {
    pub fn new(model: &str, timeout_secs: u64) -> Self {
        Self {
            model: model.to_string(),
            timeout_secs,
        }
    }
}

// =============================================================================
// EVIDENCE CONFIG
// =============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceDepth {
    Minimal,
    #[default]
    Standard,
    Comprehensive,
    FileOnly,
    FileAndLine,
    FileLineContext,
}

impl EvidenceDepth {
    pub fn min_refs(&self) -> usize {
        match self {
            Self::Minimal | Self::FileOnly => 1,
            Self::Standard | Self::FileAndLine => 2,
            Self::Comprehensive | Self::FileLineContext => 5,
        }
    }
}

// =============================================================================
// PROJECT TYPE QUALITY CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectTypeQuality {
    pub min_score: f32,
    pub min_quality_score: f32,
    pub max_iterations: usize,
    pub required_sections: Vec<String>,
    pub min_evidence_refs: usize,
    pub min_file_references: usize,
    pub min_evidence: f32,
    pub evidence_depth: EvidenceDepth,
}

impl Default for ProjectTypeQuality {
    fn default() -> Self {
        Self {
            min_score: 0.6,
            min_quality_score: 0.7,
            max_iterations: 30,
            required_sections: vec![],
            min_evidence_refs: 2,
            min_file_references: 2,
            min_evidence: 0.5,
            evidence_depth: EvidenceDepth::Standard,
        }
    }
}

// =============================================================================
// FEW SHOT CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FewShotConfig {
    pub enabled: bool,
    pub max_examples: usize,
    pub example_selection: ExampleSelection,
}

impl Default for FewShotConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_examples: 3,
            example_selection: ExampleSelection::MostRelevant,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExampleSelection {
    #[default]
    MostRelevant,
    Random,
    Recent,
}

// =============================================================================
// DEEP ANALYSIS CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DeepAnalysisConfig {
    pub enabled: bool,
    pub max_depth: usize,
    pub max_iterations: usize,
    pub follow_imports: bool,
    pub analyze_dependencies: bool,
    pub min_confidence: f32,
    pub max_code_context_chars: usize,
    pub targeted_reanalysis: bool,
    pub multi_agent: MultiAgentConfig,
}

impl Default for DeepAnalysisConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_depth: 3,
            max_iterations: 10,
            follow_imports: true,
            analyze_dependencies: true,
            min_confidence: 0.7,
            max_code_context_chars: 50_000,
            targeted_reanalysis: true,
            multi_agent: MultiAgentConfig::default(),
        }
    }
}

// =============================================================================
// STRUCTURAL VALIDATION CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StructuralValidationConfig {
    pub enabled: bool,
    pub check_file_refs: bool,
    pub check_syntax: bool,
    pub check_completeness: bool,
    pub min_module_coverage: f32,
    pub core_module_threshold: f32,
    pub required_modules: Vec<String>,
}

impl Default for StructuralValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_file_refs: true,
            check_syntax: true,
            check_completeness: true,
            min_module_coverage: 0.7,
            core_module_threshold: 0.5,
            required_modules: Vec::new(),
        }
    }
}

// =============================================================================
// VERIFICATION CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VerificationConfig {
    pub enabled: bool,
    pub check_evidence: bool,
    pub check_consistency: bool,
    pub min_confidence: f32,
    pub cross_validation: CrossValidationConfig,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_evidence: true,
            check_consistency: true,
            min_confidence: 0.7,
            cross_validation: CrossValidationConfig::default(),
        }
    }
}

// =============================================================================
// CONFIG ACCESSORS (compatibility layer)
// =============================================================================

impl Config {
    pub fn network(&self) -> NetworkConfig {
        NetworkConfig::default()
    }

    pub fn retry(&self) -> RetryConfig {
        RetryConfig::default()
    }

    pub fn circuit_breaker(&self) -> CircuitBreakerConfig {
        CircuitBreakerConfig::default()
    }

    pub fn refinement(&self) -> RefinementConfig {
        RefinementConfig::default()
    }

    pub fn learning(&self) -> LearningConfig {
        LearningConfig::default()
    }

    pub fn quality(&self) -> QualityConfig {
        QualityConfig {
            enabled: true,
            min_score: self.value.min_overall,
            min_value_score: self.value.min_overall,
            target_score: self.convergence.early_exit_threshold,
            minimum_quality: self.value.min_overall,
            min_file_refs: self.artifacts.skills.min_evidence_refs,
            min_actionable_count: 3,
            max_tier1_ratio: 0.2,
            reference_only_mode: false,
            scoring: TierScoringWeights::default(),
            skill: SkillQualityConfig::default(),
            agent: AgentQualityConfig::default(),
            memory: MemoryQualityConfig::default(),
            rule: RuleQualityConfig::default(),
            project_specific: ProjectSpecificQuality::default(),
            min_overall_score: self.value.min_overall,
            semantic: SemanticQualityThresholds::default(),
        }
    }

    pub fn deep_review(&self) -> DeepReviewConfig {
        DeepReviewConfig {
            enabled: true,
            max_attempts: (self.convergence.max_iterations / 3) as u32,
            required_passes: self.convergence.consecutive_passes as u32,
            review_timeout_secs: self.llm.timeout_secs,
            check_regression: true,
            reject_tier1: true,
            min_evidence_ratio: 0.5,
        }
    }

    pub fn semantic_validation(&self) -> SemanticValidationConfig {
        SemanticValidationConfig::default()
    }

    pub fn execution(&self) -> ExecutionConfig {
        ExecutionConfig {
            parallel_workers: self.performance.parallel_workers,
            checkpoint_enabled: self.performance.resume_on_crash,
            checkpoint_interval_secs: self.performance.checkpoint_interval_minutes * 60,
            checkpoint_interval_minutes: self.performance.checkpoint_interval_minutes,
        }
    }

    pub fn multi_agent(&self) -> MultiAgentConfig {
        MultiAgentConfig::default()
    }

    pub fn cross_validation(&self) -> CrossValidationConfig {
        CrossValidationConfig {
            enabled: self.convergence.require_cross_artifact_pass,
            ..Default::default()
        }
    }

    pub fn usability(&self) -> UsabilityConfig {
        UsabilityConfig::default()
    }

    pub fn cross_artifact(&self) -> CrossArtifactConfig {
        CrossArtifactConfig {
            enabled: self.convergence.require_cross_artifact_pass,
            ..Default::default()
        }
    }

    pub fn quality_loop(&self) -> QualityLoopConfig {
        QualityLoopConfig {
            enabled: true,
            max_iterations: self.convergence.max_iterations,
            max_outer_iterations: self.convergence.max_iterations / 2,
            target_score: self.convergence.early_exit_threshold,
            min_improvement: self.convergence.min_improvement,
            multi_agent: MultiAgentConfig::default(),
            insight_driven: InsightDrivenGenConfig::default(),
            analysis_confidence_threshold: 0.7,
            synthesis_confidence_threshold: 0.8,
            max_invalid_reference_ratio: 0.2,
            reanalysis_gap_threshold: 0.3,
        }
    }

    pub fn insight_driven(&self) -> InsightDrivenGenConfig {
        InsightDrivenGenConfig::default()
    }

    pub fn agent_generation(&self) -> AgentGenerationConfig {
        AgentGenerationConfig {
            max_agents: self.generation.limits.max_agents,
            min_evidence_refs: self.artifacts.agents.min_evidence_refs,
            require_domain_expertise: self.artifacts.agents.require_domain_expertise,
            default_tools: self.artifacts.agents.default_tools.clone(),
        }
    }

    pub fn evidence_depth(&self) -> EvidenceDepth {
        match self.analysis.depth {
            AnalysisDepth::Fast => EvidenceDepth::Minimal,
            AnalysisDepth::Standard => EvidenceDepth::Standard,
            AnalysisDepth::Complete => EvidenceDepth::Comprehensive,
        }
    }

    pub fn project_type_quality(&self, _project_type: ProjectType) -> ProjectTypeQuality {
        ProjectTypeQuality {
            min_score: self.value.min_overall,
            min_quality_score: self.value.min_overall,
            max_iterations: self.convergence.max_iterations,
            required_sections: self.artifacts.claude_md.sections.clone(),
            min_evidence_refs: self.artifacts.skills.min_evidence_refs,
            min_file_references: self.artifacts.skills.min_evidence_refs,
            min_evidence: self.value.min_overall,
            evidence_depth: self.evidence_depth(),
        }
    }

    pub fn few_shots(&self) -> FewShotConfig {
        FewShotConfig::default()
    }

    pub fn deep_analysis(&self) -> DeepAnalysisConfig {
        DeepAnalysisConfig {
            enabled: self.analysis.depth.enable_deep_analysis(),
            max_depth: match self.analysis.depth {
                AnalysisDepth::Fast => 1,
                AnalysisDepth::Standard => 2,
                AnalysisDepth::Complete => 5,
            },
            max_iterations: self.convergence.max_iterations,
            follow_imports: true,
            analyze_dependencies: self.analysis.detect_constraints,
            min_confidence: self.value.min_overall,
            max_code_context_chars: 50_000,
            targeted_reanalysis: true,
            multi_agent: MultiAgentConfig::default(),
        }
    }

    pub fn structural_validation(&self) -> StructuralValidationConfig {
        StructuralValidationConfig {
            enabled: true,
            check_file_refs: true,
            check_syntax: true,
            check_completeness: self.convergence.require_formal_pass,
            min_module_coverage: 0.7,
            core_module_threshold: 0.5,
            required_modules: Vec::new(),
        }
    }

    pub fn verification(&self) -> VerificationConfig {
        VerificationConfig {
            enabled: true,
            check_evidence: true,
            check_consistency: self.convergence.require_cross_artifact_pass,
            min_confidence: self.value.min_overall,
            cross_validation: CrossValidationConfig::default(),
        }
    }
}

// =============================================================================
// COMPATIBILITY TYPES (for existing pipeline code)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TierScoringWeights {
    pub specificity: f32,
    pub actionability: f32,
    pub evidence: f32,
    pub uniqueness: f32,
    pub tier_bonus: f32,
    // Additional fields for tier_filter.rs
    pub base_score: f32,
    pub file_ref_weight: f32,
    pub file_ref_max_count: usize,
    pub code_example_weight: f32,
    pub code_example_max_count: usize,
    pub tier3_indicator_weight: f32,
    pub tier3_indicator_max_count: usize,
    pub tier1_penalty: f32,
    pub tool_presence_weight: f32,
    pub section_weight: f32,
    pub generic_phrase_penalty: f32,
    pub example_indicator_weight: f32,
    pub path_scoped_weight: f32,
    pub generic_rule_penalty: f32,
    pub claude_md_base_score: f32,
    pub min_skill_body_lines: usize,
    pub min_claude_md_lines: usize,
    pub tier3_threshold: f32,
    pub tier2_threshold: f32,
    pub primary_content_ratio: f32,
}

impl Default for TierScoringWeights {
    fn default() -> Self {
        Self {
            specificity: 0.25,
            actionability: 0.25,
            evidence: 0.25,
            uniqueness: 0.15,
            tier_bonus: 0.10,
            // Tier filter scoring defaults
            base_score: 0.4,
            file_ref_weight: 0.1,
            file_ref_max_count: 5,
            code_example_weight: 0.05,
            code_example_max_count: 3,
            tier3_indicator_weight: 0.1,
            tier3_indicator_max_count: 3,
            tier1_penalty: 0.15,
            tool_presence_weight: 0.1,
            section_weight: 0.05,
            generic_phrase_penalty: 0.1,
            example_indicator_weight: 0.1,
            path_scoped_weight: 0.1,
            generic_rule_penalty: 0.15,
            claude_md_base_score: 0.5,
            min_skill_body_lines: 5,
            min_claude_md_lines: 20,
            tier3_threshold: 0.7,
            tier2_threshold: 0.4,
            primary_content_ratio: 0.3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FeedbackConfig {
    pub enabled: bool,
    pub aggregation_threshold: f32,
    pub max_feedback_items: usize,
    pub dimension_pass_threshold: f32,
    pub semantic_weight: f32,
    pub structural_weight: f32,
    pub cross_artifact_weight: f32,
    pub usability_weight: f32,
    pub evidence_weight: f32,
}

impl Default for FeedbackConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            aggregation_threshold: 0.7,
            max_feedback_items: 20,
            dimension_pass_threshold: 0.7,
            semantic_weight: 0.25,
            structural_weight: 0.2,
            cross_artifact_weight: 0.2,
            usability_weight: 0.15,
            evidence_weight: 0.2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillQualityConfig {
    pub min_file_refs: usize,
    pub min_actionable_count: usize,
    pub min_score: f32,
    pub min_chars: usize,
    pub min_steps: usize,
    pub target_file_refs: usize,
}

impl Default for SkillQualityConfig {
    fn default() -> Self {
        Self {
            min_file_refs: 2,
            min_actionable_count: 3,
            min_score: 0.6,
            min_chars: 500,
            min_steps: 3,
            target_file_refs: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentQualityConfig {
    pub min_evidence_refs: usize,
    pub min_score: f32,
    pub require_domain_expertise: bool,
    pub min_file_refs: usize,
    pub min_chars: usize,
    pub min_sections: usize,
    pub target_file_refs: usize,
}

impl Default for AgentQualityConfig {
    fn default() -> Self {
        Self {
            min_evidence_refs: 2,
            min_score: 0.6,
            require_domain_expertise: true,
            min_file_refs: 2,
            min_chars: 500,
            min_sections: 3,
            target_file_refs: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DimensionThresholds {
    pub specificity: f32,
    pub actionability: f32,
    pub evidence: f32,
    pub uniqueness: f32,
    pub semantic: f32,
    pub surface: f32,
    pub cross_artifact: f32,
    pub usability: f32,
}

impl Default for DimensionThresholds {
    fn default() -> Self {
        Self {
            specificity: 0.5,
            actionability: 0.5,
            evidence: 0.5,
            uniqueness: 0.3,
            semantic: 0.6,
            surface: 0.5,
            cross_artifact: 0.5,
            usability: 0.6,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CrossArtifactConfig {
    pub enabled: bool,
    pub check_consistency: bool,
    pub check_coverage: bool,
    pub max_inconsistencies: usize,
    pub min_coherence_score: f32,
    pub max_overlap_ratio: f32,
    pub reference_weight: f32,
    pub coverage_weight: f32,
    pub role_clarity_weight: f32,
}

impl Default for CrossArtifactConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_consistency: true,
            check_coverage: true,
            max_inconsistencies: 5,
            min_coherence_score: 0.5,
            max_overlap_ratio: 0.3,
            reference_weight: 0.4,
            coverage_weight: 0.3,
            role_clarity_weight: 0.3,
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_validates() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_preset_application() {
        let mut config = Config::default();

        ConfigPreset::Quick.apply(&mut config);
        assert_eq!(config.value.min_overall, 0.5);
        assert_eq!(config.convergence.max_iterations, 10);
        assert_eq!(config.analysis.depth, AnalysisDepth::Fast);

        ConfigPreset::Thorough.apply(&mut config);
        assert_eq!(config.value.min_overall, 0.7);
        assert_eq!(config.convergence.max_iterations, 50);
        assert!(config.llm.performance_model.is_some());

        ConfigPreset::Exhaustive.apply(&mut config);
        assert_eq!(config.value.min_overall, 0.8);
        assert_eq!(config.performance.max_runtime_hours, 336);
    }

    #[test]
    fn test_value_config_validation() {
        let mut config = Config::default();
        assert!(config.validate().is_ok());

        // Invalid dimension
        config.value.dimensions.mistake_prevention = 1.5;
        assert!(config.validate().is_err());

        // Fix and test weight validation
        config.value.dimensions.mistake_prevention = 0.5;
        config.value.weights.mistake_prevention = 0.5;
        config.value.weights.discoverability = 0.5;
        config.value.weights.artifact_fitness = 0.5; // Sum = 1.5, invalid
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_convergence_state() {
        let config = ConvergenceConfig {
            consecutive_passes: 2,
            max_iterations: 10,
            ..Default::default()
        };

        let mut state = ConvergenceState::default();

        // First pass
        state.record_result(true, 0.7);
        assert_eq!(state.should_terminate(&config), ConvergenceStatus::InProgress);

        // Second consecutive pass
        state.record_result(true, 0.75);
        assert_eq!(state.should_terminate(&config), ConvergenceStatus::Converged);
    }

    #[test]
    fn test_tier_pattern_matching() {
        let tier0 = TierPatterns::default_tier0();
        assert!(tier0.matches("Use cargo build to compile the project"));
        assert!(tier0.matches("You should write tests for all functions"));
        assert!(!tier0.matches("The database connection must be released before timeout"));

        let tier3 = TierPatterns::default_tier3();
        assert!(tier3.matches("This will fail if the lock is not acquired"));
        assert!(tier3.matches("You must call init() before any other method"));
        assert!(!tier3.matches("Use npm install to install dependencies"));
    }

    #[test]
    fn test_value_score_calculation() {
        let value_config = ValueConfig::default();
        let scores = ValueScores::new(0.8, 0.6, 0.7);

        let overall = value_config.calculate_overall(&scores);
        // 0.4 * 0.8 + 0.3 * 0.6 + 0.3 * 0.7 = 0.32 + 0.18 + 0.21 = 0.71
        assert!((overall - 0.71).abs() < 0.01);
    }

    #[test]
    fn test_analysis_depth() {
        assert_eq!(AnalysisDepth::Fast.max_files(), 20);
        assert_eq!(AnalysisDepth::Standard.max_llm_calls(), 50);
        assert!(AnalysisDepth::Complete.enable_deep_analysis());
    }

    #[test]
    fn test_budget_allocation() {
        let allocation = BudgetAllocation::default();
        let total = 1_000_000u64;

        assert_eq!(allocation.tokens_for_phase(total, "understanding"), 150_000);
        assert_eq!(allocation.tokens_for_phase(total, "generation"), 300_000);
    }

    #[test]
    fn test_domain_type() {
        assert_eq!(DomainType::FinTech.as_str(), "fintech");
        assert!(DomainType::FinTech.default_compliance().contains(&"PCI-DSS"));
        assert!(DomainType::Healthcare.default_compliance().contains(&"HIPAA"));
    }

    #[test]
    fn test_artifact_types() {
        let all = ArtifactType::all();
        assert_eq!(all.len(), 4);
        assert!(all.contains(&ArtifactType::ClaudeMd));
        assert!(all.contains(&ArtifactType::Rules));
    }

    // =========================================================================
    // Cross-field validation tests
    // =========================================================================

    #[test]
    fn test_clean_pass_max_attempts_validation() {
        let mut config = Config::default();

        // Valid: max_attempts >= consecutive_passes
        config.validation.clean_pass.max_attempts = 5;
        config.validation.clean_pass.consecutive_passes = 2;
        assert!(config.validate().is_ok());

        // Invalid: max_attempts < consecutive_passes
        config.validation.clean_pass.max_attempts = 1;
        config.validation.clean_pass.consecutive_passes = 3;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("max_attempts"));
    }

    #[test]
    fn test_convergence_passes_vs_iterations() {
        let mut config = Config::default();

        // Valid: consecutive_passes <= max_iterations
        config.convergence.consecutive_passes = 2;
        config.convergence.max_iterations = 10;
        assert!(config.validate().is_ok());

        // Invalid: consecutive_passes > max_iterations
        config.convergence.consecutive_passes = 15;
        config.convergence.max_iterations = 10;
        let result = config.validate();
        assert!(result.is_err());
        // Check that the error mentions the constraint violation
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("consecutive_passes") || err_msg.contains("max_iterations"),
            "Error should mention convergence constraint: {}",
            err_msg
        );
    }
}
