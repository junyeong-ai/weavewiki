# claudegen

Rust CLI that generates Claude Code plugins (CLAUDE.md, skills, agents, rules) by analyzing codebases.

## Architecture

```
src/
├── pipeline/
│   ├── adaptive.rs       # 9-phase orchestrator: Detection→Analysis→Generation→Refinement→Validation
│   ├── phases/           # ProjectDetection, ConventionInference, ConstraintExtraction
│   ├── strategy/         # RefinementStrategy implementations (Semantic, Evidence, Regeneration)
│   ├── validation/       # TierFilter, SemanticValidator, CrossArtifactValidator
│   ├── quality_loop.rs   # Outer loop with checkpoint recovery
│   └── learning.rs       # Cross-session pattern learning (failing_patterns, issue_patterns)
├── ai/
│   ├── provider/         # LlmProvider trait, ProviderChain with circuit breaker
│   └── budget.rs         # Atomic token budget (CAS loop)
├── config/types.rs       # All configuration with presets (Fast/Standard/Thorough/Exhaustive)
└── types/                # Skill, Agent, Rule, Plugin domain types
```

## Key Abstractions

### LlmProvider (ai/provider/mod.rs:227-244)
```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn generate(&self, prompt: &str, schema: &Value) -> Result<LlmResponse>;
    fn name(&self) -> &str;
    fn model(&self) -> &str;
}
```
All LLM interactions go through this trait. Providers are shared via `Arc<dyn LlmProvider>`.

### RefinementStrategy (pipeline/strategy/mod.rs:94-130)
```rust
#[async_trait]
pub trait RefinementStrategy: Send + Sync {
    fn name(&self) -> &str;
    fn applicable_to(&self, issue: &IssueKind) -> bool;
    fn priority(&self) -> u8 { 50 }
    async fn refine_skill(&self, skill: &mut Skill, ctx: &StrategyContext) -> Result<StrategyResult>;
    async fn refine_agent(&self, agent: &mut Agent, ctx: &StrategyContext) -> Result<StrategyResult>;
    async fn refine_rule(&self, rule: &mut Rule, ctx: &StrategyContext) -> Result<StrategyResult>;
}
```
Implementations: `SemanticStrategy`, `EvidenceStrategy`, `RegenerationStrategy`

### IssueKind (pipeline/strategy/mod.rs:28-41)
```rust
pub enum IssueKind {
    LowActionability,    // Lacks clear action items
    TooGeneric,          // Not project-specific
    WeakEvidence,        // Missing @file:line refs
    MissingReferences,   // Expected refs not found
    Shallow,             // Lacks depth
    Tier1Content,        // Generic knowledge (rejected)
    MissingModule,       // Key module not covered
    // ...
}
```

## Critical Constraints

### Provider Sharing
```rust
// MUST share via Arc::clone (rate limit counter is per-instance)
let provider = Arc::clone(&shared_provider);

// WRONG: New instance loses rate limit state
let provider = OpenAiProvider::new(config);
```

### Budget Atomicity (ai/budget.rs:34-54)
```rust
// Uses CAS loop for thread-safe consumption
pub fn consume(&self, tokens: u64) -> Result<()> {
    loop {
        let current = self.consumed.load(Ordering::Acquire);
        if current + tokens > self.total_budget {
            return Err(ClaudegenError::Budget { ... });
        }
        if self.consumed.compare_exchange_weak(current, current + tokens, ...).is_ok() {
            return Ok(());
        }
    }
}
```

### OnceCell for File Registry (pipeline/adaptive.rs:68,85-92)
```rust
// Expensive to build - cache with OnceCell
file_registry: OnceCell<VerifiedFileRegistry>

async fn get_file_registry(&self) -> Result<VerifiedFileRegistry> {
    self.file_registry.get_or_try_init(|| async {
        VerifiedFileRegistry::build(&self.project_root).await
    }).await.cloned()
}
```

### HashMap Bounds (pipeline/learning.rs:201-237)
```rust
// MUST bound all learning HashMaps to prevent unbounded growth
if self.failing_patterns.len() >= self.config.max_patterns {
    self.prune_oldest_failing_patterns();  // Removes bottom 10% by age
}
```

### Config Validation (config/types.rs:186-271)
Required invariants:
- `quality.minimum_quality <= quality.target`
- `deep_review.max_attempts >= deep_review.required_passes`
- `retry.backoff_factor >= 1.0`
- `execution.parallel_workers > 0`

## Content Value Classification

| Tier | Definition | Action |
|------|------------|--------|
| Tier 1 | Generic language/tool knowledge | **REJECT** |
| Tier 2 | Project conventions | Keep |
| Tier 3 | Hidden constraints, gotchas | **Essential** |

Examples:
- Tier 1 (reject): "Use `cargo build`", "Prefer `async/await`"
- Tier 2 (keep): "Controllers in `adapter/inbound/web/`"
- Tier 3 (essential): "Provider must be Arc-shared for rate limit tracking"

## Error Handling (types/error.rs:35-80)

| Category | Retry | Fallback | Action |
|----------|-------|----------|--------|
| RateLimit | Yes | No | Parse retry-after header |
| TokenLimit | No | Yes | Try next provider |
| Auth | No | No | Fail fast |
| Network | Yes | No | Exponential backoff |
| Unavailable | No | Yes | Try next provider |

## Configuration Presets (config/types.rs:17-68)

| Preset | Model | Quality Target | Strategy |
|--------|-------|----------------|----------|
| Fast | Haiku | 0.80 | 3 iterations, 1 review pass |
| Standard | Sonnet | 0.85 | 10 iterations, 2 review passes |
| Thorough | Sonnet | 0.90 | 20 iterations, 2 review passes |
| Exhaustive | Opus | 0.95 | Long-running (weeks), 3 review passes |

Config resolution: defaults → `~/.config/claudegen/config.toml` → `.claudegen/config.toml` → `CLAUDEGEN_*` env vars

## Extension Points

**Add new refinement strategy:**
1. Create `src/pipeline/strategy/{name}.rs`
2. Implement `RefinementStrategy` trait
3. Add to `RefinementStrategyType` enum in `config/types.rs`
4. Register in `StrategyRotator::new()` at `pipeline/strategy/mod.rs:141-146`

**Add new validation:**
1. Create validator in `src/pipeline/validation/`
2. Call from `AdaptivePipeline::run()` final validation phase (adaptive.rs:380-410)

**Add new analysis phase:**
1. Create phase in `src/pipeline/phases/`
2. Integrate into `AdaptivePipeline::run()` (adaptive.rs:122-437)
