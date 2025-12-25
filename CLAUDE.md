# WeaveWiki - AI Agent Developer Guide

Rust CLI for AI-driven codebase documentation. 6-phase multi-agent pipeline, SQLite persistence, tree-sitter parsing.

---

## Architecture

```
src/
├── main.rs                    # CLI entry, command dispatch
├── lib.rs                     # Module exports
├── constants.rs               # Centralized constants (budget, thresholds)
├── config/                    # AnalysisMode, ProjectScale, ModeConfig
├── ai/
│   ├── provider/
│   │   ├── mod.rs             # LlmProvider trait, LlmResponse
│   │   ├── claude_code.rs     # Default provider (subprocess)
│   │   ├── openai.rs          # HTTP API
│   │   ├── chain.rs           # Fallback chain with retry
│   │   └── circuit_breaker.rs # Circuit breaker pattern
│   ├── validation/            # Response validation
│   │   ├── json_repair.rs     # JSON repair attempts
│   │   ├── response.rs        # Schema validation
│   │   └── diagram.rs         # Mermaid validation
│   ├── budget.rs              # Token budget per phase (TALE)
│   ├── metrics.rs             # Usage metrics collection
│   ├── preflight.rs           # Pre-flight validation
│   ├── timeout.rs             # Timeout management
│   └── tokenizer.rs           # Token counting
├── wiki/exhaustive/
│   ├── mod.rs                 # MultiAgentPipeline orchestrator
│   ├── checkpoint.rs          # CheckpointManager, PipelinePhase
│   ├── session_context.rs     # Session context for prompts
│   ├── characterization/      # Phase 1: 7 agents (Turn 1-3)
│   ├── bottom_up/             # Phase 2-3: File analysis (Tier-based)
│   ├── top_down/              # Phase 4: Architecture agents
│   ├── consolidation/         # Phase 5: Domain grouping
│   ├── refinement/            # Phase 6: Quality iteration
│   ├── research/              # Deep Research implementation
│   ├── documentation/         # Doc blueprint generation
│   ├── patterns.rs            # Code pattern extraction
│   ├── mermaid.rs             # Mermaid diagram utilities
│   └── llms_txt.rs            # llms.txt generation
├── analyzer/
│   ├── parser/                # Tree-sitter (11 languages)
│   ├── scanner/               # File scanning with gitignore
│   └── structure.rs           # Code structure analysis
├── storage/database.rs        # SQLite WAL, r2d2 pool
├── types/
│   ├── error.rs               # WeaveError, ErrorCategory
│   └── ...                    # Domain types
├── cli/                       # CLI commands (init, generate, query, etc.)
└── verifier/                  # Knowledge base verification
```

---

## Pipeline Flow

```
Characterization (Turn 1-3)
  │ Turn 1: Structure, Dependency, EntryPoint (parallel)
  │ Turn 2: Purpose, Technical, Terminology (parallel)
  │ Turn 3: SectionDiscovery
  ↓
ProjectProfile → Bottom-Up (Leaf→Standard→Important→Core)
  ↓
FileInsight[] → Top-Down (Architecture, Flow, Risk, Domain)
  ↓
ProjectInsight[] → Consolidation (SemanticDomainGrouper)
  ↓
DomainInsight[] → Refinement (QualityScorer → DocGenerator)
  ↓
Wiki Output (.weavewiki/wiki/)
```

---

## Key Patterns

### Adding Characterization Agent

```rust
// characterization/agents/my_agent.rs
#[async_trait]
impl CharacterizationAgent for MyAgent {
    fn name(&self) -> &str { "my_agent" }
    fn turn(&self) -> u8 { 1 }  // 1, 2, or 3

    async fn run(&self, ctx: &CharacterizationContext) -> Result<AgentOutput> {
        let prompt = PromptBuilder::new()
            .role("analyst", "your domain")
            .objectives(vec!["Find X", "Identify Y"])
            .build();

        let response = ctx.provider.generate(&prompt, &schema).await?;
        Ok(AgentOutput { agent_name: self.name().into(), turn: self.turn(), .. })
    }
}
// Register in characterization/mod.rs agents vec
```

### Adding LLM Provider

```rust
// ai/provider/my_provider.rs
#[async_trait]
impl LlmProvider for MyProvider {
    async fn generate(&self, prompt: &str, schema: &Value) -> Result<LlmResponse>;
    fn name(&self) -> &str;
    fn model(&self) -> &str;
}
// Add to create_provider() in mod.rs
```

### Adding Language Parser

```rust
// analyzer/parser/my_lang.rs
impl Parser for MyLangParser {
    fn parse(&self, path: &str, content: &str) -> Result<ParseResult>;
    fn language(&self) -> Language;
}
// Requires tree-sitter grammar in Cargo.toml
// Add to Language enum in language.rs
```

---

## Processing Tiers (Bottom-Up)

```rust
enum ProcessingTier {
    Leaf = 0,      // Utilities, minimal analysis
    Standard = 1,  // Normal depth
    Important = 2, // Deep Research (3 turns + child context)
    Core = 3,      // Deep Research (4 turns + child context)
}
// Deep Research: Plan → Update1 → Update2 (Core only) → Synthesis
```

---

## Token Budget Allocation

```rust
// constants.rs - TALE algorithm (single source of truth)
PhaseAllocations {
    characterization: 5%,   // 7 agents project profiling
    bottom_up: 50%,         // All file analysis (largest portion)
    top_down: 10%,          // 4 agents project-level analysis
    consolidation: 20%,     // Per-domain AI synthesis
    refinement: 15%,        // Quality improvement passes
}
// Dynamic reallocation when phase completes early
```

---

## Database Schema

```sql
-- storage/schema.sql (key tables)
doc_sessions (id, project_path, status, current_phase, checkpoint_data, project_profile)
file_checkpoints (session_id, file_path, insight_json)
agent_insights (session_id, agent_name, turn, output_json)
```

---

## Error Categories

```rust
// types/error.rs - ErrorCategory enum
RateLimit    → Retry with exponential backoff
TokenLimit   → Reduce context or fallback provider
Auth         → Fail fast
Network      → Retry with backoff
ParseError   → JSON repair attempt (ai/validation/)
```

---

## Constants

| Location | Constant | Value |
|----------|----------|-------|
| `constants.rs` | DEFAULT_BUDGET | 1,000,000 tokens |
| `constants.rs` | WARNING_THRESHOLD | 75% |
| `constants.rs` | CRITICAL_THRESHOLD | 90% |
| `constants.rs` | MAX_CHILD_CONTEXT_TOKENS | 2000 |
| `constants.rs` | FAILURE_THRESHOLD | 5 |

---

## Common Tasks

### Resume Interrupted Session
```rust
let checkpoint = pipeline.load_checkpoint()?;
pipeline.resume(checkpoint).await?;
```

### Debug LLM Calls
```bash
RUST_LOG=debug cargo run -- generate
```

### Inspect Checkpoints
```bash
sqlite3 .weavewiki/weavewiki.db "SELECT * FROM doc_sessions"
```

---

## Test Commands

```bash
cargo test                    # 267 tests
cargo clippy -- -D warnings   # Lint
cargo fmt --check             # Format check
cargo build --release         # Release build
```
