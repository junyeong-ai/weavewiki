# claudegen - Core Architecture Analysis

> Analysis date: 2026-02-07
> Source: 100% fact-based from actual code reading. Every claim includes file:line references.

---

## 1. Project Overview

**claudegen** is a Rust CLI tool that analyzes codebases and generates Claude Code plugins:
- `CLAUDE.md` (project memory)
- `.claude/rules/` (hierarchical rules with category-based subdirectories)
- `.claude/skills/` (automated skills)
- `.claude/agents/` (specialized agents)

**Rust Edition**: 2024, minimum rustc 1.92 (`Cargo.toml:4-5`)

**Key Dependencies** (`Cargo.toml:17-70`):
- `claude-agent` (local path dep, optional feature) - Direct Claude API via SDK
- `modmap` (local path dep) - Shared schema definitions
- `tree-sitter` + language grammars (TS, Python, Rust, Go, Bash) - AST parsing
- `tokio` (full) - Async runtime
- `figment` (toml, env) - Layered config
- `rusqlite` + `r2d2` - SQLite connection pooling
- `dashmap` - Lock-free concurrent maps
- `blake3` - Content hashing
- `schemars` - JSON Schema generation

---

## 2. Module Dependency Graph

```
src/lib.rs:10-19  (top-level modules)

                    main.rs
                      |
                    cli/
                   /    \
           commands/   progress, ui, util
          /  |  |  \
     generate validate sync init/status/clean/config
         |        \       \
     pipeline/     |       pipeline/sync/
         |         |
    quality_loop   |
         |         |
    adaptive       |
    (pipeline)     |
     /    |    \   |
  analysis generation validation
     |        |        |
  ai/      types/    pipeline/context
     |
  provider/
  /  |  \  \
chain circuit_breaker claude_agent tracked
```

### Module Responsibilities

| Module | Purpose | Key File(s) |
|--------|---------|-------------|
| `cli/` | CLI entry, commands, progress | `src/cli/mod.rs:1-5` |
| `pipeline/` | 9-phase pipeline orchestration | `src/pipeline/mod.rs:1-34` |
| `ai/` | LLM integration, provider abstraction | `src/ai/mod.rs:1-39` |
| `config/` | Layered config (Figment) | `src/config/loader.rs:1-8` |
| `types/` | Domain types, error types | `src/types/mod.rs:1-62` |
| `analyzer/` | Tree-sitter AST parsing | `src/lib.rs:12` |
| `storage/` | Persistence | `src/lib.rs:17` |
| `utils/` | Shared utilities | `src/lib.rs:19` |
| `constants/` | Compile-time constants | `src/constants.rs:1-168` |

---

## 3. Data Flow: End-to-End Pipeline

### 3.1 Entry Point

```
main.rs:146-156  main() -> ExitCode
  -> run_cli()
    -> Cli::parse() (clap)
    -> Commands::Generate { output, resume, dry_run }
      -> claudegen::cli::commands::generate::run(GenerateOptions)
```

The `generate` command (`src/cli/commands/generate.rs:25-87`) is the primary workflow:

1. Load config via `ConfigLoader::load_for_project()` (`generate.rs:37`)
2. Create shared budget and metrics (`generate.rs:51-53`)
3. Create tiered `ProviderSet` with tracking (`generate.rs:56-58`)
4. Create `QualityLoop` (`generate.rs:60-62`)
5. Run `quality_loop.run()` (`generate.rs:74`)
6. Write output files (`generate.rs:76`)

### 3.2 Pipeline Phases (AdaptivePipeline::run)

Defined in `src/pipeline/mod.rs:3-11` and implemented in `src/pipeline/adaptive.rs:161-838`:

| Phase | What Happens | Code Location |
|-------|-------------|---------------|
| **Phase 1** | Project Detection | `adaptive.rs:171` -> `project_detection::detect()` |
| **Phase 2** | Monorepo Analysis (conditional) | `adaptive.rs:195-220` -> `monorepo_analyzer::analyze()` |
| **Phase 2.5** | Deep Analysis (with graceful timeout) | `adaptive.rs:230-248` -> `run_deep_analysis()` |
| **Phase 2.6** | Analysis Synthesis + confidence gating + re-analysis loop | `adaptive.rs:271-372` |
| **Phase 2.7** | Domain Analysis (policies, logic, terminology) | `adaptive.rs:393-433` |
| **Phase 2.8** | Cross-Reference Synthesis | `adaptive.rs:436-482` |
| **Phase 3** | Convention Inference | `adaptive.rs:493-501` |
| **Phase 4** | Constraint Extraction (enhanced with synthesis) | `adaptive.rs:524-534` |
| **Phase 4.5** | Module Detection (if multi-agent enabled) | `adaptive.rs:560-622` |
| **Phase 5** | Output Planning (module-aware) | `adaptive.rs:625-639` |
| **Phase 6** | Draft Generation (CLAUDE.md, rules, skills, agents) | `adaptive.rs:642-738` |
| **Phase 7** | Quality-Based Refinement Loop | `adaptive.rs:742-779` |
| **Phase 7.5** | Populate imports and navigation in CLAUDE.md | `adaptive.rs:782-786` |
| **Phase 8** | Final Validation (simplified) | `adaptive.rs:789-806` |

### 3.3 Quality Loop (Outer Loop)

The `QualityLoop` (`src/pipeline/quality_loop.rs:107-115`) wraps the `AdaptivePipeline` with quality gates:

```
QualityLoop::run()  (quality_loop.rs:187)
  |
  +-> try_recover() (crash recovery from checkpoint)
  +-> run_with_checkpoints()  (quality_loop.rs:348)
       |
       while iter_state.should_continue():
         1. Create AdaptivePipeline with current config
         2. pipeline.run() with timeout
         3. Gate 1: Analysis confidence check (quality_loop.rs:499)
         4. Gate 2: Synthesis confidence check (quality_loop.rs:559)
         5. Gate 3: Evidence validation (quality_loop.rs:619)
         6. Gate 4: Deep Review (two-pass LLM review) (quality_loop.rs:684)
         7. Gate 5: Validation pipeline (quality_loop.rs:692)
         8. Clean pass tracking for convergence (quality_loop.rs:696-713)
         |
         If gate fails -> escalate analysis depth and retry
         If converged -> return best result
```

**Convergence Criteria** (`quality_loop.rs:702-710`):
- `clean_streak >= DEFAULT_REQUIRED_CLEAN_PASSES` (2)
- Clean pass = deep_review_passed AND validation_passed

**Depth Escalation** (`quality_loop.rs:1068-1099`):
- `Fast` -> `Standard` -> `Complete`
- Each escalation increases `max_file_samples` by 1.5x

---

## 4. AI Provider Architecture

### 4.1 LlmProvider Trait

Defined at `src/ai/provider/mod.rs:366-399`:

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn generate(&self, prompt: &str, schema: &Value) -> Result<LlmResponse>;
    async fn generate_with_system(&self, system: &str, prompt: &str, schema: &Value) -> Result<LlmResponse>;
    fn name(&self) -> &str;
    fn model(&self) -> &str;
    async fn health_check(&self) -> Result<bool>;
}
```

All providers return `LlmResponse` with `TokenUsage`, `ResponseTiming`, `ResponseMetadata`, and `StopReason` (`mod.rs:56-63`).

### 4.2 Provider Implementations

| Provider | File | Role |
|----------|------|------|
| `ClaudeAgentProvider` | `claude_agent.rs:27-33` | Default. Uses `claude-agent` SDK. OAuth or API key. |
| `OpenAiProvider` | `provider/openai.rs` | OpenAI-compatible endpoints |
| `ProviderChain` | `chain.rs:134-139` | Fallback chain with circuit breakers |
| `TrackedProvider` | `tracked.rs:16-20` | Budget enforcement + metrics collection decorator |

### 4.3 Tiered Model Routing (ProviderSet)

`src/ai/provider/mod.rs:207-284`:

```rust
pub struct ProviderSet {
    pub fast: Arc<dyn LlmProvider>,        // Haiku-class
    pub default: Arc<dyn LlmProvider>,     // Sonnet-class
    pub performance: Arc<dyn LlmProvider>, // Opus-class
}
```

Phase-to-tier mapping (`mod.rs:259-283`):
- **Fast tier**: `project_detection`, `convention_inference`, `validation`, `tier_classification`
- **Default tier**: `generation`, `refinement`, `review`
- **Performance tier**: `constraint_extraction`, `mistake_discovery`, `deep_analysis`, `synthesis`, `deep_review`

Phase IDs are compile-time constants in `phase_id` module (`mod.rs:183-201`).

### 4.4 Circuit Breaker Pattern

`src/ai/provider/circuit_breaker.rs:130-134`:

```
Closed --[failure_threshold reached]--> Open
Open --[timeout elapsed]--> HalfOpen
HalfOpen --[success]--> Closed
HalfOpen --[failure]--> Open
```

Implementation uses `RwLock<CircuitBreakerInner>` for unified state management (`circuit_breaker.rs:93-104`).

The `ProviderChain` (`chain.rs:134-139`) uses `DashMap<String, CircuitBreaker>` for lock-free concurrent access across providers. Error classification (`chain.rs:323-386`) determines retry strategy:
- `Auth` / `TokenLimit` / `Unavailable` -> skip to next provider
- `BadRequest` -> stop chain entirely
- `RateLimit` -> parse retry-after and wait
- `Network` / `Transient` -> exponential backoff with jitter
- `ParseError` -> retry with delay

### 4.5 TrackedProvider (Decorator)

`src/ai/provider/tracked.rs:16-20`:

Wraps any `LlmProvider` to add:
1. **Pre-check**: Estimate tokens from prompt length, reject if budget insufficient (`tracked.rs:40-53`)
2. **Post-response**: Consume actual tokens from `SharedBudget` (`tracked.rs:59-68`)
3. **Post-response**: Record metrics in `SharedMetrics` (`tracked.rs:71-73`)

### 4.6 Claude Agent Provider Details

`src/ai/provider/claude_agent.rs:27-33`:

- Default model: `claude-sonnet-4-5-20250929` (`claude_agent.rs:23`)
- Auto-retry on truncation: up to 4 attempts, doubling `max_tokens` each time (`claude_agent.rs:24-25,184-197`)
- Uses SDK features: `StructuredOutputs`, `PromptCaching`, optional `Context1M` (`claude_agent.rs:70-75`)
- Extended context (1M) requires API key auth, not OAuth (`claude_agent.rs:93-96`)

---

## 5. Configuration System

### 5.1 Layered Resolution

`src/config/loader.rs:37-91`:

```
1. Built-in defaults (Config::default())
2. Global config (~/.config/claudegen/config.toml)
3. Project config (.claudegen/config.toml)
4. Environment variables (CLAUDEGEN_* prefix)
```

Uses `Figment` with `Serialized` -> `Toml` -> `Env` merge chain. Each layer overrides the previous.

### 5.2 Config Structure

Root config has 30+ sub-configs (`src/config/types.rs:24-61`):

| Sub-Config | Purpose |
|-----------|---------|
| `GenerationConfig` | Artifact types, limits, strategy (ValueDriven/CoverageDriven/Minimal) |
| `ValueConfig` | Min overall value score (default: 0.6) |
| `ConvergenceConfig` | Max iterations (100), consecutive passes (2), quality floor (0.75), target (0.90) |
| `LlmConfig` | Provider, models, timeout, temperature, context settings |
| `AnalysisConfig` | Include/exclude patterns, depth, max file samples |
| `BudgetConfig` | Total token budget |
| `CircuitBreakerConfig` | Failure threshold, recovery timeout, half-open max calls |
| `QualityLoopConfig` | Enabled, max iterations, target quality |
| `DistributedAnalysisConfig` | Chunk sizing, concurrency, min files for distributed |
| `MultiAgentConfig` | Module detection thresholds |
| `TimeoutConfig` | Per-phase timeouts, session timeout, checkpoint interval |
| `SyncConfig` | Incremental update settings |

### 5.3 Validation

Config self-validates on load (`config/types.rs:104-196`):
- Value thresholds in [0.0, 1.0]
- Non-zero budget, workers, judges
- Cross-field: `consecutive_passes <= max_iterations`
- Cross-field: `deep_review.max_attempts >= required_passes`
- Warns if `early_exit_threshold < min_overall`

---

## 6. Context Management

### 6.1 ClaudegenContext

`src/pipeline/context.rs:72-77`:

Accumulates analysis results across pipeline phases and quality loop iterations:
- `AnalysisResults` - detection, conventions, constraints, deep analysis, synthesis, domain, cross-insights, modules
- `tier3_constraints` - Capped at 50, deduped by name (case-insensitive) (`context.rs:15`)
- `key_abstractions` - Capped at 100, deduped by name (`context.rs:16`)
- `iteration_count` - Tracks quality loop iterations

`merge_from()` (`context.rs:162-219`) merges contexts across iterations with dedup + bounds.

### 6.2 VerifiedFileRegistry

`src/pipeline/context.rs:370-375`:

Central truth source for file existence validation:
- Built from filesystem walk using `ignore` crate (gitignore-aware) (`context.rs:403-453`)
- Respects analysis include/exclude patterns
- Provides `FileMetadata`: line count, byte size, extension, parent module, estimated complexity/tokens
- Used throughout pipeline for reference validation (hallucination detection)

---

## 7. Output Architecture

### 7.1 Artifact Types

- **CLAUDE.md** (`ProjectMemory`) - Project root, atomic write (`adaptive.rs:1395-1399`)
- **Rules** (`.claude/rules/{category}/{name}.md`) - Category subdirectories: tech, framework, module, group, domain (`adaptive.rs:1511-1542`)
- **Skills** (`.claude/skills/{name}.md`) - YAML frontmatter format (`adaptive.rs:1479-1492`)
- **Agents** (`.claude/agents/{name}.md`) - YAML frontmatter format (`adaptive.rs:1495-1508`)
- **Module Map** (`.claudegen/output/module_map.json`) - Optional, for multi-agent orchestration (`adaptive.rs:1419-1429`)
- **Nested CLAUDE.md** - Per-workspace in monorepos (`adaptive.rs:1457-1473`)

### 7.2 Atomic Writes

`src/pipeline/adaptive.rs:1613-1635`:

All file writes use `atomic_write()`: write to temp file -> sync to disk -> rename. Prevents partial writes on crash.

---

## 8. Design Patterns Identified

### 8.1 Chain of Responsibility
- `ProviderChain` (`chain.rs:134-139`): Cascading provider attempts with circuit breakers
- Config resolution: defaults -> global -> project -> env (`loader.rs:37-91`)

### 8.2 Strategy Pattern
- `GenerationStrategy` enum: ValueDriven, CoverageDriven, Minimal (`config/types.rs:339-349`)
- `AnalysisDepth` enum: Fast, Standard, Complete (used in depth escalation)
- Phase-based provider routing: `ProviderSet::provider_for_phase()` (`mod.rs:259-283`)

### 8.3 Decorator Pattern
- `TrackedProvider` wraps any `LlmProvider` with budget + metrics (`tracked.rs:16-20`)
- `ProviderChain` wraps providers with circuit breaker + retry (`chain.rs:134-139`)

### 8.4 Builder Pattern
- `ProviderChainBuilder` (`chain.rs:512-573`)
- `GenerationContext::new().with_synthesis().with_domain_analysis()` (`adaptive.rs:701-717`)
- `QualityLoop::new().with_budget().with_metrics().with_output_dir()` (`generate.rs:60-69`)

### 8.5 Observer/Event Sourcing
- `EventStore` for pipeline events (phase completion, pattern dedup, etc.)
- Events emitted throughout `AdaptivePipeline::run()` with `store.append()` calls
- `ResumeState::from_events()` for session recovery (`adaptive.rs:104-111`)

### 8.6 Map-Reduce
- Distributed analysis: chunk files -> analyze chunks in parallel -> aggregate results
- `DistributedAnalyzer.analyze_all_chunks()` -> `AnalysisAggregator::aggregate()` (`adaptive.rs:1316-1321`)
- `HierarchicalSummarizer`: chunk results -> module summaries -> project summary (`adaptive.rs:1346-1378`)

### 8.7 Circuit Breaker
- Full implementation: Closed -> Open -> HalfOpen state machine (`circuit_breaker.rs:1-19`)
- Per-provider instance, shared across chain clones via `Arc<DashMap>` (`chain.rs:138`)

### 8.8 Quality Gate / Pipeline Pattern
- Multi-gate quality loop with escalation (`quality_loop.rs:348-844`)
- 5 gates: analysis confidence, synthesis confidence, evidence, deep review, validation
- Convergence requires N consecutive clean passes

---

## 9. Extensibility Points

| Extension Point | Mechanism | Location |
|----------------|-----------|----------|
| New LLM providers | Implement `LlmProvider` trait | `ai/provider/mod.rs:366-399` |
| New languages | Add tree-sitter grammar | `Cargo.toml:44-49`, `analyzer/parser/` |
| New CLI commands | Add variant to `Commands` enum | `main.rs:24-82` |
| Custom rules | `RuleCategory::Custom` variant | `types/rule.rs` |
| Config overrides | TOML files or `CLAUDEGEN_*` env vars | `config/loader.rs:37-91` |
| Generation strategies | `GenerationStrategy` enum | `config/types.rs:339-349` |
| Analysis phases | Add to `AdaptivePipeline::run()` | `pipeline/adaptive.rs:161-838` |
| Quality gates | Add to `QualityLoop::run_with_checkpoints()` | `pipeline/quality_loop.rs:348-844` |

---

## 10. Concurrency Model

- **Async runtime**: Tokio with full features (`Cargo.toml:26`)
- **Some commands sync**: `main.rs` uses `Runtime::new()` + `block_on()` for async commands (`main.rs:194-196`)
- **Lock-free maps**: `DashMap` for circuit breaker state (`chain.rs:138`)
- **Once-initialized caches**: `tokio::sync::OnceCell` for file registry and event store (`adaptive.rs:66-67`)
- **Shared providers**: `Arc<dyn LlmProvider>` for concurrent pipeline phases (`mod.rs:173`)
- **Connection pooling**: `r2d2` + `r2d2_sqlite` for SQLite (`Cargo.toml:39-41`)

---

## 11. Error Handling

- **Primary error type**: `ClaudegenError` (thiserror) with variants: Config, Io, LlmApi, Budget, Timeout, Verification, Json (`types/error.rs`)
- **Result alias**: `pub type Result<T> = std::result::Result<T, ClaudegenError>` (`types/error.rs`)
- **Exit codes**: `e.exit_code()` mapping for CLI (`main.rs:153`)
- **Panic handler**: Custom panic hook with issue reporting URL (`main.rs:114-143`)
- **Graceful degradation**: Most analysis phases use `match/Err => warn + proceed without` pattern (e.g., `adaptive.rs:426-429`, `adaptive.rs:475-478`)

---

## 12. Potential Design Observations

### 12.1 Large AdaptivePipeline::run() Method
- `adaptive.rs:161-838` is 677 lines in a single method
- Contains all 9 phases inline with event emission boilerplate
- Each phase could be extracted into separate methods or a phase trait

### 12.2 Tight Coupling Between Pipeline and Event Store
- Event emission is interleaved with business logic throughout `adaptive.rs`
- Events are fire-and-forget (`if let Err(e) = ... { warn }`) which is good for resilience
- But the event types are specific to pipeline internals

### 12.3 Config Complexity
- 30+ sub-configs with many cross-field dependencies (`config/types.rs:24-61`)
- Default `Config` is ~100 lines of defaults (`config/types.rs:63-101`)
- Could benefit from config profiles (e.g., "fast", "thorough", "production")

### 12.4 Sync vs Async CLI
- `generate` command creates its own `Runtime` and calls async code
- Some commands (`init`, `status`, `config`) are sync
- `validate` and `sync` each create their own `Runtime` in `main.rs:194-196,234-241`
- Pattern: `let rt = Runtime::new()?; rt.block_on(...)` repeated

### 12.5 Atomic Write Safety
- `atomic_write()` uses temp file + rename pattern (`adaptive.rs:1613-1635`)
- Temp file uses UUID naming to avoid collisions
- Properly handles cleanup on failure

### 12.6 Budget Enforcement
- Pre-check is estimate-based (may over-reject) (`tracked.rs:41-43`)
- Post-check warns but continues (`tracked.rs:61-67`) - the request already happened
- This is a reasonable tradeoff for not blocking mid-generation

---

## 13. Key Type Relationships

```
QualityLoop
  -> AdaptivePipeline
    -> ProviderSet (fast/default/performance)
      -> TrackedProvider
        -> ProviderChain
          -> ChainedProvider[]
            -> CircuitBreaker (per provider)
            -> LlmProvider (trait object)
              -> ClaudeAgentProvider | OpenAiProvider
    -> Config
    -> VerifiedFileRegistry (OnceCell)
    -> EventStore (OnceCell)
    -> ClaudegenContext
  -> CheckpointManager
  -> DeepReviewEngine
  -> IterationState
```

```
AdaptivePipelineOutput
  |-- claude_md: ProjectMemory
  |-- skills: Vec<Skill>
  |-- agents: Vec<Agent>
  |-- rules: Vec<Rule>
  |-- detection: ProjectDetection
  |-- deep_analysis: Option<DeepAnalysisResult>
  |-- synthesis: Option<SynthesizedAnalysis>
  |-- output_plan: OutputPlan
  |-- validity_filter_result: ValidityFilterResult
  |-- consistency_result: ConsistencyResult
  |-- cross_validation_result: CrossValidationResult
  |-- quality_score: f32
  |-- refinement_iterations: usize
  |-- refinement_converged: bool
  |-- context: ClaudegenContext
  |-- module_map: Option<ModuleMap>
  |-- extracted_docs: Vec<(String, String)>
  |-- monorepo: Option<MonorepoAnalysis>
```

---

## 14. CLI Command Structure

Defined in `src/main.rs:24-82`:

| Command | Async? | Description |
|---------|--------|-------------|
| `init` | No | Initialize `.claudegen/` directory |
| `generate` | Yes | Full pipeline: analyze + generate + validate |
| `validate` | Yes | Validate generated artifacts |
| `status` | No | Show project status |
| `clean` | Yes | Clean up claudegen data |
| `config show/path/edit/init` | No | Configuration management |
| `sync` | Yes | Incremental update based on file changes |

The `sync` command (`src/cli/commands/sync.rs:22-107`) provides incremental updates:
1. Load `ProjectManifest` from `.claudegen/manifest.json`
2. `FileTracker::detect_changes()` - compare file hashes
3. `DependencyGraph::build() + affected_by()` - transitive dependency resolution
4. Mark affected artifacts as stale
5. Update manifest with new hashes

---

## 15. Summary

claudegen is a well-architected Rust CLI with:

- **9-phase adaptive pipeline** with graceful degradation at each phase
- **Tiered LLM routing** (fast/default/performance) for cost-optimized AI calls
- **Resilience patterns**: circuit breakers, retry with exponential backoff, rate limit awareness
- **Quality convergence loop**: multi-gate validation with automatic depth escalation
- **Durable execution**: checkpoint-based crash recovery, event sourcing for session resume
- **Layered configuration**: defaults -> global -> project -> env with full validation
- **Atomic file writes**: temp + rename for crash safety
- **100% file coverage analysis**: distributed map-reduce with hierarchical summarization
- **Anti-hallucination**: VerifiedFileRegistry validates all LLM-generated file references
