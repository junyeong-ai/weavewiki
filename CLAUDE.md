# claudegen

Rust CLI generating Claude Code plugins (CLAUDE.md, skills, agents, rules) via codebase analysis.

---

## Design Philosophy

> **"No generation without evidence."**

All outputs must be grounded in actual codebase evidence. Prevents LLM hallucination and extracts only project-specific valuable knowledge.

**Operating Model**:
1. Gather evidence from codebase analysis
2. Generate evidence-backed content
3. Complete when 2+ consecutive clean reviews pass
4. On quality failure: rotate strategy and retry

---

## Core Principles

### I. Evidence-Based Generation (NON-NEGOTIABLE)

All generated content must have verifiable evidence.

| Requirement | Description |
|-------------|-------------|
| `@file:line` references | Every claim must cite source location |
| WeakEvidence blocks | Insufficient evidence fails generation |
| Tier 1 rejection | Generic knowledge is never generated |

```
# BAD: Claim without evidence
"This project uses Clean Architecture"

# GOOD: Evidence-backed claim
"Clean Architecture - Controller → UseCase dependency at adapter/inbound/web/:42"
```

### II. Universal Applicability (NON-NEGOTIABLE)

This system MUST work across:
- **All languages**: Rust, Python, Go, Java, TypeScript, Ruby, PHP, C++, etc.
- **All frameworks**: React, Django, Spring, Rails, Express, etc.
- **All project structures**: monorepo, polyglot, microservices, libraries

**Critical**: Any logic that assumes specific language/framework/structure will fail.

### III. Convergent Quality Verification (NON-NEGOTIABLE)

Quality verified through iterative convergence until stable.

| Condition | Result |
|-----------|--------|
| N consecutive clean passes | Success |
| max_iterations reached | Failure |
| Oscillation detected (3+ fix-break cycles) | Strategy rotation or failure |
| All strategies exhausted | Early termination |

### IV. Use First (NON-NEGOTIABLE)

Implemented code must be integrated immediately.

**Enforcement**:
- New modules require integration code in same change
- `cargo test` must exercise new code paths
- Unused `pub` exports are bugs

---

## LLM vs Logic Decision Framework

### Key Insight: Bad Info is Worse Than No Info

When programmatic logic provides **inaccurate or incomplete information**, it can:
1. **Mislead LLM** into wrong decisions
2. **Override LLM's correct intuition** with incorrect "facts"
3. **Limit LLM's ability** to handle edge cases

**Rule**: If you need domain knowledge to interpret it → use LLM.

### Decision Tree

```
Should this be programmatic?

1. Is it DETERMINISTIC? (same input → always same output)
   NO  → Use LLM
   YES ↓

2. Is it UNIVERSAL? (works for ALL languages/frameworks/projects)
   NO  → Use LLM or Two-Phase
   YES ↓

3. What if it's WRONG?
   Silent failure / Bad data → DON'T USE (let LLM decide)
   Slight inefficiency only → OK to use
```

### Safe Universal Signals

| Signal | Why Universal |
|--------|---------------|
| File existence | Filesystem API, language-agnostic |
| File modification (mtime + size) | OS-level, deterministic |
| HTTP status codes (429, 404, 502) | RFC-defined |
| Exit code 0 vs non-zero | POSIX standard |
| Marker files (Cargo.toml, package.json) | Spec-defined |
| Mathematical convergence (N clean rounds) | Pure logic |

### Dangerous Context-Dependent Patterns

| Pattern | Problem |
|---------|---------|
| Directory names (`build`, `dist`, `target`) | Mean different things in different projects |
| File extensions for "code vs config" | `.yml` is config? Or Ansible code? |
| "Test" detection | Varies by framework |
| Error message keywords | English only, format varies |

### Two-Phase Pattern

When you need speed but also accuracy:

```
Phase 1: Programmatic (fast path)
  - Handle UNAMBIGUOUS cases only
  - High confidence threshold (≥95%)
  - Skip if ANY uncertainty

Phase 2: LLM (if Phase 1 doesn't match)
  - Provide raw data, not interpreted data
  - Let LLM apply its judgment
```

### Implementation Mapping

| Task | Approach | Rationale |
|------|----------|-----------|
| File registry | Logic | Filesystem is deterministic |
| Reference extraction | Hybrid | Regex extract + LLM validate |
| Tier classification | LLM | Semantic judgment required |
| Strategy selection | Logic | IssueKind → Strategy is deterministic |
| Refinement execution | LLM | Creative improvement needed |
| Budget management | Logic | Atomic operations required |

---

## Content Tier Classification

| Tier | Definition | Action |
|------|------------|--------|
| Tier 0 | Hallucinated/invalid content | **ALWAYS REJECT** |
| Tier 1 | Generic language/tool knowledge | **REJECT** |
| Tier 2 | Project conventions | Keep |
| Tier 3 | Hidden constraints, gotchas | **Essential** |

**Tier 3 Indicators**:
- "MUST", "NEVER" keywords
- Bug/failure-derived patterns
- Undocumented implicit rules
- Order/combination-dependent logic

---

## Architecture

```
src/
├── pipeline/
│   ├── adaptive.rs       # Multi-phase orchestrator
│   ├── phases/           # Detection, inference, extraction
│   ├── analysis/         # Distributed, deep, domain analyzers
│   ├── strategy/         # Semantic, evidence, regeneration strategies
│   ├── quality/          # LLM judge, quality assessment
│   ├── validation/       # Quality validation
│   ├── quality_loop.rs   # Convergent verification loop
│   └── learning.rs       # Cross-session pattern learning
├── ai/
│   ├── provider/         # LlmProvider trait, ProviderChain, circuit breaker
│   ├── response/         # Schema generation, structured output parsing
│   ├── validation/       # LLM response validation
│   └── budget.rs         # Atomic token budget (CAS loop)
├── config/types.rs       # Single high-quality configuration
├── utils/                # Path security, patterns
└── types/                # Skill, Agent, Rule, Plugin domain types
```

### Key Abstractions

**LlmProvider** - Single entry point for all LLM interactions
```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn generate(&self, prompt: &str, schema: &Value) -> Result<LlmResponse>;
    fn name(&self) -> &str;
    fn model(&self) -> &str;
    async fn health_check(&self) -> Result<bool>;
}
```

**RefinementStrategy** - Pluggable quality improvement strategies
```rust
#[async_trait]
pub trait RefinementStrategy: Send + Sync {
    fn name(&self) -> &str;
    fn applicable_to(&self, issue: &IssueKind) -> bool;
    fn priority(&self) -> u8 { 50 }
    async fn refine_skill(&self, skill: &mut Skill, ctx: &StrategyContext<'_>) -> Result<StrategyResult>;
    async fn refine_agent(&self, agent: &mut Agent, ctx: &StrategyContext<'_>) -> Result<StrategyResult>;
    async fn refine_rule(&self, rule: &mut Rule, ctx: &StrategyContext<'_>) -> Result<StrategyResult> { Ok(default()) }
}
```

### IssueKind → Strategy Mapping

| IssueKind | Primary Strategy | Fallback |
|-----------|------------------|----------|
| WeakEvidence | EvidenceStrategy | RegenerationStrategy |
| MissingReferences | EvidenceStrategy | RegenerationStrategy |
| TooGeneric | SemanticStrategy | RegenerationStrategy |
| LowActionability | SemanticStrategy | RegenerationStrategy |
| Shallow | SemanticStrategy | RegenerationStrategy |
| Redundant | SemanticStrategy | RegenerationStrategy |
| Tier1Content | **REJECT** (no retry) | - |
| MissingModule | RegenerationStrategy | - |

---

## Critical Constraints (NON-NEGOTIABLE)

### Provider Sharing
```rust
// MUST share via Arc::clone (rate limit counter is per-instance)
let provider = Arc::clone(&shared_provider);

// WRONG: New instance loses rate limit state
let provider = OpenAiProvider::new(config);  // FORBIDDEN
```

### Budget Atomicity
```rust
pub fn consume(&self, tokens: u64) -> Result<()> {
    loop {
        let current = self.consumed.load(Ordering::Acquire);
        if current + tokens > self.total_budget {
            return Err(ClaudegenError::Budget { ... });
        }
        if self.consumed.compare_exchange_weak(
            current, current + tokens,
            Ordering::Release, Ordering::Relaxed
        ).is_ok() {
            return Ok(());
        }
    }
}
```

### OnceCell for Expensive Operations
```rust
file_registry: OnceCell<VerifiedFileRegistry>

async fn get_file_registry(&self) -> Result<VerifiedFileRegistry> {
    self.file_registry.get_or_try_init(|| async {
        VerifiedFileRegistry::build(&self.project_root).await
    }).await.cloned()
}
```

### HashMap Bounds
```rust
// MUST bound all learning HashMaps
if self.failing_patterns.len() >= self.config.max_patterns {
    self.prune_oldest_failing_patterns();
}
```

### Path Security
```rust
use crate::utils::safe_join;

// safe_join returns None if traversal detected
if let Some(path) = safe_join(&root, relative_path) {
    // Safe to use
}
```

---

## Error Handling

| Category | Retry | Fallback | Action |
|----------|-------|----------|--------|
| RateLimit | Yes | No | Parse retry-after header |
| TokenLimit | No | Yes | Try next provider |
| Auth | No | No | Fail fast |
| Network | Yes | No | Exponential backoff |
| Unavailable | No | Yes | Try next provider |

---

## Configuration

Default high-quality settings:

| Setting | Value | Description |
|---------|-------|-------------|
| Models | Haiku + Sonnet + Opus | Tiered for cost/quality balance |
| Quality Floor | 0.75 | Minimum acceptable quality |
| Target Quality | 0.90 | Goal quality score |
| Max Iterations | 100 | Thorough refinement |
| Analysis Depth | Complete | Full codebase analysis |

**Config Resolution Order**:
```
defaults → ~/.config/claudegen/config.toml → .claudegen/config.toml → CLAUDEGEN_* env
```

---

## Extension Points

**Add refinement strategy:**
1. Create `src/pipeline/strategy/{name}.rs`
2. Implement `RefinementStrategy` trait
3. Add to `RefinementStrategyType` enum in `config/types.rs`
4. Register in `StrategyRotator::new()`
5. Add tests exercising the strategy

**Add analysis phase:**
1. Create phase in `src/pipeline/phases/`
2. Integrate into `AdaptivePipeline::run()`
3. Ensure output consumed by downstream

---

## Anti-Patterns

| Anti-Pattern | Why Bad | Correct Approach |
|--------------|---------|------------------|
| Claims without evidence | Hallucination risk | Always cite `@file:line` |
| Include Tier 1 content | Zero value, noise | Classify and reject |
| Unbounded retry | Resource waste | max_iterations + oscillation detection |
| Unbounded HashMap | Memory leak | Bounded + prune |
| Implement without integration | Dead code | Use First principle |
| Ignore oscillation | Cannot converge | Rotate strategy after 3 cycles |
| Interpreted data to LLM | Misleads LLM | Pass raw data, let LLM interpret |
| Context-dependent heuristics | Fails across projects | Use LLM or two-phase |

---

## Conservative Defaults

When uncertain, prefer:

| Instead of | Do this |
|------------|---------|
| Skip (may miss data) | Include (may over-estimate) |
| Interpret (may be wrong) | Pass raw data to LLM |
| Restrict options | Let LLM reason freely |
| Fail silently | Surface uncertainty to LLM |
| Assume (based on pattern) | Ask LLM to verify |

**Principle**: Over-estimation and extra LLM calls are recoverable.
Missing data and wrong interpretations cause cascading failures.

---

## Implementation Checklist

Before adding ANY programmatic logic, verify:

- [ ] Works for Rust, Python, Go, Java, TypeScript, Ruby, C++ (at minimum)
- [ ] Works for monorepo, microservices, single-package projects
- [ ] Works for non-English error messages and file names
- [ ] If wrong, only causes inefficiency (not silent failure)
- [ ] Raw data is preserved for LLM fallback
- [ ] Has clear "I don't know" path that defers to LLM

If ANY checkbox fails → use LLM or two-phase approach.
