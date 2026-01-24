# claudegen

Rust CLI that generates Claude Code plugins (CLAUDE.md, skills, agents, rules) by analyzing codebases.

## Architecture

```
src/
├── pipeline/
│   ├── adaptive.rs       # Multi-phase orchestrator
│   ├── phases/           # Analysis phases (detection, inference, extraction)
│   ├── strategy/         # Refinement strategies (semantic, evidence, regeneration)
│   ├── validation/       # Quality result types
│   ├── quality_loop.rs   # Outer loop with checkpoint recovery
│   └── learning.rs       # Cross-session pattern learning
├── ai/
│   ├── provider/         # LlmProvider trait, ProviderChain with circuit breaker
│   └── budget.rs         # Atomic token budget (CAS loop)
├── config/types.rs       # Configuration with presets
├── utils/                # Path security, patterns
└── types/                # Skill, Agent, Rule, Plugin domain types
```

## Key Abstractions

### LlmProvider
```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn generate(&self, prompt: &str, schema: &Value) -> Result<LlmResponse>;
    fn name(&self) -> &str;
    fn model(&self) -> &str;
}
```
All LLM interactions go through this trait. Providers are shared via `Arc<dyn LlmProvider>`.

### RefinementStrategy
```rust
#[async_trait]
pub trait RefinementStrategy: Send + Sync {
    fn name(&self) -> &str;
    fn applicable_to(&self, issue: &IssueKind) -> bool;
    async fn refine_skill(&self, skill: &mut Skill, ctx: &StrategyContext<'_>) -> Result<StrategyResult>;
    async fn refine_agent(&self, agent: &mut Agent, ctx: &StrategyContext<'_>) -> Result<StrategyResult>;
    async fn refine_rule(&self, rule: &mut Rule, ctx: &StrategyContext<'_>) -> Result<StrategyResult>;
}
```
Implementations: `SemanticStrategy`, `EvidenceStrategy`, `RegenerationStrategy` in `pipeline/strategy/`

### IssueKind
Quality issues detected during refinement:
- `LowActionability` - Lacks clear action items
- `TooGeneric` - Not project-specific
- `WeakEvidence` - Missing @file:line refs
- `Tier1Content` - Generic knowledge (rejected)
- `MissingModule` - Key module not covered
- `Shallow`, `TooShort`, `MissingSections`, `Redundant`, `PlanMismatch`, `PartialModuleCoverage`

## Critical Constraints

### Provider Sharing
```rust
// MUST share via Arc::clone (rate limit counter is per-instance)
let provider = Arc::clone(&shared_provider);

// WRONG: New instance loses rate limit state
let provider = OpenAiProvider::new(config);
```

### Budget Atomicity
```rust
// Uses CAS loop for thread-safe consumption
pub fn consume(&self, tokens: u64) -> Result<()> {
    loop {
        let current = self.consumed.load(Ordering::Acquire);
        if current + tokens > self.total_budget {
            return Err(ClaudegenError::Budget { ... });
        }
        if self.consumed.compare_exchange_weak(...).is_ok() {
            return Ok(());
        }
    }
}
```

### OnceCell for File Registry
```rust
// Expensive to build - cache with OnceCell
file_registry: OnceCell<VerifiedFileRegistry>

async fn get_file_registry(&self) -> Result<VerifiedFileRegistry> {
    self.file_registry.get_or_try_init(|| async {
        VerifiedFileRegistry::build(&self.project_root).await
    }).await.cloned()
}
```

### HashMap Bounds
```rust
// MUST bound all learning HashMaps to prevent unbounded growth
if self.failing_patterns.len() >= self.config.max_patterns {
    self.prune_oldest_failing_patterns();
}
```

### Path Security
```rust
// Use utils/path.rs for all path resolution
use crate::utils::safe_join;

// safe_join returns None if traversal detected
if let Some(path) = safe_join(&root, relative_path) {
    // Safe to use
}
```

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

## Error Handling

| Category | Retry | Fallback | Action |
|----------|-------|----------|--------|
| RateLimit | Yes | No | Parse retry-after header |
| TokenLimit | No | Yes | Try next provider |
| Auth | No | No | Fail fast |
| Network | Yes | No | Exponential backoff |
| Unavailable | No | Yes | Try next provider |

## Configuration Presets

| Preset | Model | Quality Target | Strategy |
|--------|-------|----------------|----------|
| Quick | Haiku | 0.60 | 10 iterations, 1 review pass |
| Standard | Sonnet | 0.80 | 30 iterations, 2 review passes |
| Thorough | Sonnet | 0.90 | 50 iterations, 2 review passes |
| Exhaustive | Opus | 0.95 | 100 iterations, 3 review passes |

Config resolution: defaults → `~/.config/claudegen/config.toml` → `.claudegen/config.toml` → `CLAUDEGEN_*` env vars

## Extension Points

**Add new refinement strategy:**
1. Create `src/pipeline/strategy/{name}.rs`
2. Implement `RefinementStrategy` trait
3. Add to `RefinementStrategyType` enum in `config/types.rs`
4. Register in `StrategyRotator::new()`

**Add new validation:**
1. Create validator in `src/pipeline/validation/`
2. Call from `AdaptivePipeline::run()` final validation phase

**Add new analysis phase:**
1. Create phase in `src/pipeline/phases/`
2. Integrate into `AdaptivePipeline::run()`
