# Analysis Pipeline Deep Dive: Map-Reduce / AST / Chunk Architecture

## 1. Map-Reduce Completeness

### Map Phase: Distributed Chunk Analysis

The Map phase is implemented in `src/pipeline/analysis/distributed.rs`. The entry point `DistributedAnalyzer::analyze_all_chunks_resumable` (line 599) orchestrates parallel chunk analysis with semaphore-bounded concurrency.

**Chunking Strategy** (`distributed.rs:237`):
- `ChunkingStrategy::create_chunks` builds chunks from `VerifiedFileRegistry` respecting token limits
- Files are grouped into chunks that fit within the model's context window
- Large files are split at AST-aware boundaries via `split_by_ast_boundaries` (line 418)
- Cross-chunk references are computed via `compute_cross_references` (line 460) using a `SymbolIndex`

**Per-chunk Analysis** (`distributed.rs:749`):
- `analyze_chunk` performs cache lookup first (`ChunkCache`), then falls back to LLM call
- Retry logic with exponential backoff: `MAX_RETRIES=3`, `INITIAL_BACKOFF_MS=1000`
- Context overflow prevention: `MAX_PROMPT_CHARS = 200_000` hard cap (line 908)
- Truncation uses AST-based boundaries first, then paragraph breaks, then char boundaries (`truncate_at_structure_boundary`, line 939)

**Concurrency Model**:
- Uses `tokio::sync::Semaphore` for bounded parallelism
- Checkpoint resumability: can resume from last completed chunk after failures
- Each chunk analyzed independently, enabling horizontal scaling

### Reduce Phase: Aggregation

The Reduce phase is in `src/pipeline/analysis/aggregator.rs`. `AnalysisAggregator::aggregate` (line 496) combines chunk results into `AggregatedAnalysis`.

**Pattern Merging** (`aggregator.rs:676`):
- Patterns merged by `category:name` composite key
- Multi-module tracking: patterns appearing in multiple modules get bonus scores
- Variant descriptions capped at `MAX_PATTERN_VARIANTS` to prevent unbounded growth

**Convention Voting** (`aggregator.rs:598`):
- `reduce_conventions`: voting-based reduction for naming, error handling, async patterns
- Weighted by `lines_analyzed`: `sum(confidence * lines_analyzed) / sum(lines_analyzed)`

**Constraint Combination** (`aggregator.rs:727`):
- Constraints merged by `kind:title` composite key
- Cross-module detection: constraints appearing across modules flagged

**Gotcha Deduplication** (`aggregator.rs:760`):
- Jaccard word-level similarity at `GOTCHA_DEDUP_SIMILARITY` threshold
- Prevents near-duplicate gotchas from multiple chunks

**Dependency Graph Construction** (`aggregator.rs:880`):
- Edge weight accumulation from chunk-level dependency data
- Hub module detection: threshold = `total_modules / 3` (minimum 2)

**Importance Scoring** (`aggregator.rs:368`):
- Formula: `frequency_ratio * (1.0 + multi_module_bonus(0.5) + evidence_bonus(0.3))`
- Multi-module bonus rewards patterns found across different modules
- Evidence bonus rewards patterns with validated file references

### Assessment

The Map-Reduce implementation is comprehensive. The Map phase handles all files via chunking, and the Reduce phase properly deduplicates and scores results. The key design decision is token-budget-aware chunking rather than fixed-size chunks, which adapts to different file sizes.

**Gap**: The `SymbolIndex` (aggregator.rs:438) filters common symbols (`new`, `get`, `set`, etc.) to reduce noise, but this filter list is hardcoded. Language-specific common symbols (e.g., Python's `__init__`, Go's `New`) would benefit from language-aware filtering.

---

## 2. AST Analysis Depth and Accuracy

### Tree-sitter Parser Infrastructure

**Parser Trait** (`src/analyzer/parser/traits.rs:288`):
- Unified `Parser` trait: `parse(&self, path: &str, content: &str) -> Result<ParseResult>`
- `ParseResult` contains `Vec<Node>` + `Vec<Edge>` -- a code graph
- Helper functions: `create_ts_parser`, `evidence_from_node`, `execute_query`, `query_captures`
- Evidence tracking: every node/edge gets `EvidenceLocation` with file:line:column

**Language Support** (`src/analyzer/parser/language.rs:678`):
- 45+ languages in the `Language` enum with metadata (display_name, extensions, aliases)
- Tree-sitter parser support for 7 languages: Rust, Go, Python, TypeScript, JavaScript, TSX, JSX, Bash
- `Language::from_path` auto-detects from file extension
- `has_parser_support()` distinguishes parseable vs. extension-only languages

### Per-Language Parser Analysis

**Rust Parser** (`src/analyzer/parser/rust_lang.rs:466`):
- Extracts: use statements, mod declarations, structs, enums, traits, functions, impl blocks
- Visibility detection via string matching on parent node text (line 419) -- documented limitation
- Handles `pub`, `pub(crate)`, `pub(super)` visibility
- Function signature extraction: parameters parsed from text, async detection from parent node
- Impl blocks create `EdgeType::Implements` edges between structs and traits

**TypeScript Parser** (`src/analyzer/parser/typescript.rs:397`):
- Extracts: imports, exports, classes, functions, interfaces
- Import resolution: `resolve_import` converts relative paths to absolute (line 382)
- Only tracks relative imports (starting with `.` or `/`)
- Export edge type: `EdgeType::Exposes` -- tracks public API surface
- Async detection: checks if "async" precedes function keyword in source bytes

**Python Parser** (`src/analyzer/parser/python.rs:250`):
- Extracts: imports (relative only), classes, functions
- PEP 8 visibility convention: `_` prefix = Private, else Public (line 214)
- Uses shared helper functions (`create_code_node`, `create_code_edge`, `evidence_from_node`)
- Only tracks relative imports (starting with `.`) -- absolute imports skipped

**Go Parser** (`src/analyzer/parser/go.rs:334`):
- Extracts: imports, structs, interfaces, functions, methods (with receiver type)
- Go visibility: uppercase first letter = Public, lowercase = Private (line 292)
- Methods create `EdgeType::Owns` edges from struct to method
- Method receiver type extracted via tree-sitter query on parameter list

**Bash Parser** (`src/analyzer/parser/bash.rs:103`):
- Minimal extraction: functions only
- All functions treated as Public visibility (no private concept in Bash)
- No import/dependency extraction -- Bash lacks formal module system
- No parameter extraction (Bash uses positional args, not formal parameters)

### AST Enrichment & Validation

**AstEnricher** (`src/pipeline/analysis/ast_enrichment.rs:581`):
- `extract_facts` (line 253): ground-truth extraction via tree-sitter
- `AstFacts` stores: functions, types, traits, imports per file
- Validation levels (`AstValidation` enum):
  - `Exact`: function found at exact line
  - `Close`: found within `LINE_PROXIMITY_TOLERANCE = 5` lines
  - `WrongLine`: found in correct file but wrong line
  - `WrongFile`: function exists but in different file
  - `NotFound`: no matching function found
- Used to validate LLM claims against actual code -- prevents hallucination

### Assessment

AST depth is solid for the supported languages. Each parser extracts structural elements (types, functions, imports) with evidence locations. The key strength is the `AstValidation` enum that provides graduated trust levels for LLM claim validation.

**Gaps**:
1. Return type extraction is `None` for all parsers -- not yet implemented
2. Bash parser is minimal (functions only, no sourcing/dependency tracking)
3. TypeScript parser uses `LANGUAGE_TYPESCRIPT` for all JS/TS/TSX/JSX variants -- may miss JSX-specific constructs
4. No C/C++, Java, Kotlin, Ruby, or PHP parsers despite those languages being detected

---

## 3. Chunk Splitting Strategy

### AST-Aware Chunking (`src/pipeline/analysis/ast_chunking.rs:312`)

**Entry Point**: `analyze_file_ast` (line 57)
- Uses tree-sitter to find structural boundaries (function/class/impl boundaries)
- Supports Rust, Python, TypeScript/JS, Go via `get_ts_language` (line 45)

**Block Splitting Threshold**: `MAX_BLOCK_LINES_BEFORE_SPLIT = 80` (line 8)
- Impl/class blocks exceeding 80 lines are split at method-level boundaries
- `collect_method_boundaries` (line 156) finds method start lines within large blocks

**ExportedSymbol Tracking**:
- `SymbolKind` enum: Function, Struct, Enum, Trait, Class, Impl, Type, Const, Module
- Each symbol tracked with start/end line positions for splitting decisions

### Chunking in Distributed Analysis (`distributed.rs`)

**Token-Budget Chunking** (`create_chunks`, line 237):
- Files grouped into chunks that respect token limits
- Large files split at AST boundaries within budget
- Cross-chunk references computed post-chunking via symbol index

**Cross-chunk Reference Computation** (`compute_cross_references`, line 460):
- Builds symbol index from all chunks
- Filters common symbols to reduce noise
- Scans for references between chunks to track hidden dependencies

**Truncation Fallback** (`truncate_at_structure_boundary`, line 939):
- Priority: AST boundary > paragraph break (`\n\n`) > char boundary
- Ensures content never exceeds `MAX_PROMPT_CHARS = 200_000`

### Assessment

The chunking strategy is well-designed with three levels of sophistication:
1. **File-level grouping**: Respects token budget
2. **AST-aware splitting**: Large files split at structural boundaries
3. **Cross-chunk reference tracking**: Symbol index maintains semantic connections

**Strength**: The 80-line threshold for method-level splitting is reasonable for most languages. Files with very large impl blocks get granular splitting.

**Gap**: The chunking strategy doesn't account for semantic cohesion beyond structural boundaries. Two related functions might be split into different chunks if they're separated by a struct definition.

---

## 4. Hierarchical Summarization

### Implementation (`src/pipeline/analysis/hierarchical_summarizer.rs:289`)

**Three-level hierarchy**: `HierarchicalSummarizer::summarize` (line 42)

1. **Chunk level**: Raw analysis results from each chunk
2. **Module level** (`build_module_summary`, line 109):
   - Single-chunk modules: promoted directly (no summarization overhead)
   - Multi-chunk modules: patterns/constraints/gotchas deduplicated across chunks
   - Module-level key files and dependencies aggregated
3. **Project level** (`build_architecture_overview`, line 149):
   - Top 5 modules by importance selected
   - Cross-cutting patterns identified (appearing in 3+ modules)
   - Key patterns highlighted (appearing in 2+ modules)
   - Architecture overview derived from module interactions

### Information Flow

```
Chunk Results (N chunks)
  --> Module Summaries (M modules, M << N)
    --> Project Summary (1 project overview)
```

### Assessment

The hierarchical summarization is effective but conservative:
- Single-chunk modules skip summarization entirely (good optimization)
- Multi-chunk deduplication uses module boundaries (correct)
- Project overview limits to top 5 modules -- may miss important smaller modules

**Gap**: No configurable threshold for the "top 5 modules" limit. In large codebases with 20+ modules, this could lose important context.

---

## 5. Context Window Overflow Prevention

### ContextBudget (`src/ai/context_tracker.rs:132`)

**Architecture**:
- 80/20 split: 80% for input, 20% reserved for output generation
- `MODEL_CONTEXT_LIMIT` from constants module
- Token estimation: `chars / CHARS_PER_TOKEN` (approx 4 chars/token)

**3-tier Progressive Loading**:
- **Tier 1 (Essential)**: Project detection, conventions, constraints -- always included
- **Tier 2 (Relevant)**: Module summaries for current artifact -- included if space allows
- **Tier 3 (Reference)**: Full analysis, cross-synthesis -- summarized if budget tight

**Budget Allocation API** (`ContextBudget::allocate`, line 51):
- Per-section allocation with overflow protection
- Requested amount capped to remaining budget
- `needs_summarization` (line 77): returns true if content doesn't fit remaining budget
- `utilization` (line 71): tracks usage ratio for monitoring

### Overflow Protection in Distributed Analysis

- `MAX_PROMPT_CHARS = 200_000` hard cap (`distributed.rs:908`)
- `truncate_at_structure_boundary` (line 939): graceful truncation at AST/paragraph boundaries
- Prompt construction checks character count before sending to LLM

### Assessment

The context budget system is well-designed with enforced limits (not just guidance). The 3-tier model ensures critical information always fits.

**Strength**: The `allocate` method caps to remaining budget rather than failing -- graceful degradation.

**Gap**: Token estimation uses a fixed `CHARS_PER_TOKEN` constant (approximately 4). This is a rough approximation that varies by language (code-heavy content has different tokenization than prose).

---

## 6. Distributed Analysis Effectiveness

### Parallel Execution Model (`distributed.rs`)

**Concurrency Control**:
- Semaphore-bounded parallelism: configurable concurrent chunk limit
- Exponential backoff retry: 3 retries with doubling delay from 1000ms
- Checkpoint resumability: saves progress for long-running analyses

**Cache System** (`src/pipeline/analysis/chunk_cache.rs:274`):
- Blake3 hashing for cache keys
- Import-path-aware invalidation: cache invalidated when imports change
- `PROMPT_VERSION = "v1"`: version tracking for cache staleness
- Time-based eviction via `evict_older_than`
- Path traversal protection via `safe_join`

**Completeness Validation** (`src/pipeline/analysis/completeness_validator.rs:333`):
- 3-step validation process (`validate`, line 65):
  1. Find truncated files: coverage ratio < `MIN_FILE_COVERAGE_RATIO`
  2. Retry failed chunks
  3. Detect referenced-but-unanalyzed modules
- Ensures 100% file coverage after distributed analysis

### Assessment

The distributed analysis is production-ready with proper concurrency control, caching, and completeness validation.

**Strength**: Completeness validation catches both truncated files and missing modules -- no silent gaps.

**Gap**: The retry mechanism uses fixed `MAX_RETRIES=3`. For very large codebases where LLM rate limiting is common, adaptive retry counts based on error type would be more robust.

---

## 7. Cross-Synthesis Integration

### Implementation (`src/pipeline/analysis/cross_synthesis.rs:1095`)

**CrossSynthesizer::synthesize** (line 379) performs multi-dimensional cross-analysis:

1. **Hidden Dependencies** (`find_hidden_dependencies`, line 413):
   - Extracted from cross-module constraints in aggregated analysis
   - Types: SharedState, InitializationOrder, ConfigurationCoupling, RuntimeOnly

2. **Architecture Violations** (`detect_architecture_violations`, line 510):
   - Circular dependency detection
   - Layer bypass detection (skipping architectural layers)
   - Wrong direction detection (data layer depending on presentation)
   - Missing abstraction detection: `MIN_FAN_IN_FOR_ABSTRACTION = 3` (line 22)

3. **3-Layer Architecture Classification**:
   - Presentation(0) -> Service(1) -> Data(2)
   - Keyword-based classification for layer assignment
   - `is_implementation_detail` (line 361): keyword detection (internal, impl, raw, private)

4. **Insight Tiers**:
   - Tier 2: Convention-level insights from cross-module patterns
   - Tier 3: Essential constraints that must be respected

### Integration with Other Phases

The cross-synthesis receives inputs from:
- `AggregatedAnalysis` (from Reduce phase)
- `DomainAnalysisResult` (from domain analyzer)
- Module detection results

And feeds into:
- `OutputRouter` for output planning
- Constraint extraction for hidden dependency enrichment
- Agent planning for specialized agent creation

### Assessment

Cross-synthesis is the most architecturally significant phase -- it connects bottom-up (chunk analysis) with top-down (architecture detection) perspectives.

**Strength**: Architecture violation detection is comprehensive with 4 violation types.

**Gap**: Layer classification uses keyword matching only. Projects with non-standard naming (e.g., "gateway" instead of "controller") may be misclassified.

---

## 8. Language and Framework Support

### Language Detection (`src/analyzer/parser/language.rs:678`)

**45+ languages detected** from file extensions, organized in a metadata table:
- Each language has: display_name, highlight_str, extensions (1-4), aliases, has_parser flag
- Detection via `Language::from_path` -- purely extension-based

**Tree-sitter Parser Support (7 languages)**:
| Language | Parser | Extraction Depth |
|----------|--------|------------------|
| Rust | `RustParser` | Structs, enums, traits, functions, impls, use statements, mod declarations |
| TypeScript/JS/TSX/JSX | `TypeScriptParser` | Classes, functions, interfaces, imports, exports |
| Python | `PythonParser` | Classes, functions, imports (relative only) |
| Go | `GoParser` | Structs, interfaces, functions, methods (with receivers) |
| Bash | `BashParser` | Functions only |

### Framework Detection (`src/pipeline/phases/project_detection.rs:1557`)

**Comprehensive framework detection across ecosystems**:
- **Rust**: clap, structopt, argh (CLI); actix-web, axum, rocket, warp, tide (backend)
- **Node.js**: express, fastify, koa, hono, nestjs (backend); react, vue, angular, svelte, next, nuxt (frontend)
- **Python**: fastapi, django, flask, starlette (backend)
- **JVM**: spring-boot, ktor, micronaut, quarkus (backend)
- **Go**: gin, echo, fiber, chi, mux, gorm (backend)

### Workspace Detection (`project_detection.rs`)

**10 workspace types supported**:
1. CargoWorkspace (Rust)
2. PnpmWorkspace
3. NpmWorkspace
4. YarnWorkspace
5. TurboRepo
6. NxWorkspace
7. LernaMonorepo
8. GradleMultiProject (JVM)
9. MavenMultiModule (JVM)
10. GoWorkspace

Each with dedicated parsing logic and member type inference.

### Assessment

Language detection is broad (45+) but parser depth is narrow (7 languages). Framework detection is practical and covers major ecosystems.

**Gap**: No parser support for Java, Kotlin, C/C++, Ruby, PHP despite having language detection. These languages would benefit from tree-sitter parsers for deeper analysis.

---

## 9. Monorepo and Microservice Scalability

### Monorepo Analysis (`src/pipeline/phases/monorepo_analyzer.rs:572`)

**MonorepoAnalyzer::analyze** (line 105):
- Detects subprojects from workspace member declarations
- Analyzes per-subproject: type, language, dependencies, entry points
- Finds shared packages: libraries consumed by multiple subprojects
- Detects cross-dependencies between subprojects

**Output Strategy Selection** (`determine_output_strategy`, line 488):
- Unified: single-project or small monorepo
- SplitByLanguage: multi-language monorepo
- SplitByProject: >3 projects with different types
- Hierarchical: >5 subprojects

**Rules Grouping** (`create_rules_grouping`, line 513):
- Groups subprojects by `(ProjectType, Language)` tuple
- Each group gets dedicated rule generation scope

### Service Detection (`src/pipeline/phases/service_detection.rs:619`)

**ServiceDetector::detect_from_modules** (line 181):
- Identifies services from detected modules
- Service type classification via `ServiceIndicators`:
  - Api, Worker, Gateway, Library, Cli, Web
- Interface detection: HTTP routes, gRPC services, GraphQL, WebSocket, Queue
- Service dependency graph construction

**Service Graph** (`build_service_graph`, line 499):
- Nodes: services with type
- Edges: dependencies with type (HTTP, gRPC, Queue, Database, Cache, Internal)

### Scalability Assessment

**Strengths**:
- Workspace detection supports 10 workspace types across ecosystems
- Output strategy adapts to monorepo structure
- Service detection extracts actual HTTP routes and gRPC service names from code

**Gaps**:
1. `SERVICE_CANDIDATE_RISK_THRESHOLD = 0.5` (service_detection.rs:166) is a single threshold for all project types
2. Cross-dependency detection uses string matching of service names in file content -- may produce false positives
3. No DAG validation on the service graph -- circular service dependencies are detected only at the module level

---

## 10. Information Loss Prevention

### Verified Reference Pool (`src/pipeline/analysis/reference_pool.rs:415`)

**VerifiedReferencePool::build** (line 97):
- Collects references from: patterns, abstractions, violations, insights, dependencies
- Each reference validated:
  - File existence check against `VerifiedFileRegistry`
  - Line validation: line=0 accepted as file-level, line>0 validated against file line count
- Invalid references logged and excluded
- `SkillCategory` filtering: Review, Implement, Debug, Plan, Refactor

### Evidence Validation (`src/pipeline/analysis/synthesis.rs:971`)

**AnalysisSynthesizer::synthesize** (line 279):
- `validate_all_references` (line 313): every file reference checked
- `cross_validate` (line 396): pattern/constraint evidence cross-validated between deep and structural analysis
- `resolve_conflicts` (line 62): category-specific conflict resolution
- `enhance_with_ast` (line 805): AST ground-truth validates LLM claims, adjusts confidence

### Finding Pipeline (`src/pipeline/analysis/finding.rs:221` + `extractor.rs:242`)

**RawFinding** with UUID tracking:
- 10 `FindingKind` variants for categorization
- 5 `AnalysisSource` variants for provenance tracking
- Confidence clamping to [0.0, 1.0]
- `ChunkResultExtractor::extract_all`: normalizes chunk results into standard `RawFinding` format

### Analysis Archive (`src/pipeline/analysis/archive.rs:350`)

**AnalysisArchive**:
- JSON persistence of all chunk results
- File-to-chunk reverse index for lookup
- Preserves full analysis data for incremental re-analysis

### Assessment

Information loss prevention is thorough:
1. **Input validation**: File registry verifies all paths exist
2. **Reference validation**: Every `@file:line` reference validated against actual files
3. **Cross-validation**: LLM claims validated against AST ground truth
4. **Provenance tracking**: UUID-based finding pipeline tracks origin of every insight
5. **Archival**: Full results persisted for incremental analysis

**Strength**: The `AstValidation` enum provides graduated trust levels (Exact/Close/WrongLine/WrongFile/NotFound) rather than binary accept/reject.

**Gap**: Archive format is JSON-based with no schema versioning. Schema changes in `ChunkAnalysisResult` could break deserialization of archived results.

---

## Summary of Key Metrics

| Dimension | Status | Key Reference |
|-----------|--------|---------------|
| Map-Reduce completeness | Complete | `distributed.rs:599` (Map), `aggregator.rs:496` (Reduce) |
| AST depth | 7 languages | `parser/mod.rs:30` (factory) |
| Chunk splitting | AST-aware + token-budget | `ast_chunking.rs:57`, `distributed.rs:237` |
| Hierarchical summarization | 3-level | `hierarchical_summarizer.rs:42` |
| Context overflow prevention | Enforced (not advisory) | `context_tracker.rs:39` |
| Distributed analysis | Production-ready | `distributed.rs:599`, `chunk_cache.rs:1` |
| Cross-synthesis | 4 violation types | `cross_synthesis.rs:379` |
| Language support | 45+ detected, 7 parsed | `language.rs:1`, `mod.rs:30` |
| Monorepo scalability | 10 workspace types | `project_detection.rs:573`, `monorepo_analyzer.rs:105` |
| Information loss prevention | 5-layer validation | `reference_pool.rs:97`, `synthesis.rs:313` |

## Top Issues

1. **Return type extraction missing**: All parsers set `return_type: None` -- limits function signature analysis
2. **Token estimation approximation**: Fixed `CHARS_PER_TOKEN` doesn't account for language-specific tokenization
3. **Archive schema versioning**: No migration path for serialized chunk results when types change
4. **Language parser coverage gap**: 45 languages detected but only 7 have tree-sitter support
5. **Service dependency false positives**: String matching for cross-service dependencies may match unrelated identifiers
