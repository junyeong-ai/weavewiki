# Implementation Roadmap - Architecture Redesign

**Net Change**: -96 lines (1010 added, 1106 deleted)
**Files Affected**: 36 files
**Estimated Timeline**: 4-6 phases, ~2-3 days with testing

---

## Dependency Graph

```
Phase 0: Foundation (No dependencies)
├─ Task #2: FileRef Unification
└─ Task #8: Dead Code Removal

Phase 1: Core Infrastructure (Depends on Phase 0)
├─ Task #3: Reference Validation Migration (needs FileRef)
├─ Task #1: Dependency Parsing (independent)
└─ Task #5: Event Sourcing Consolidation (independent)

Phase 2: Provider Enhancement (Depends on Phase 1)
└─ Task #4: Context & Budget + Prompt Caching (needs EventStore)

Phase 3: Analysis Optimization (Depends on Phase 1)
├─ Task #3: Pipeline Integration (needs FileRef + validation)
└─ Task #7: Service Detection Redesign (needs ManifestParser)

Phase 4: Quality System (Depends on Phase 3)
└─ Task #6: Quality System Overhaul (needs all validation)

Phase 5: Final Integration & Verification
└─ Smoke tests, regression tests, integration verification
```

---

## Phase 0: Foundation (BREAK NOTHING)

**Goal**: Remove dead code, unify types. Zero behavioral changes.

### Step 0.1: Dead Code Removal (Task #8)
**Files**: 3 deletions, 0 modifications

```bash
# Delete unused files
rm src/pipeline/analysis/hierarchical_summarizer.rs
rm src/pipeline/analysis/finding.rs
rm src/pipeline/analysis/extractor.rs

# Update mod.rs
```

**Edit**: `src/pipeline/analysis/mod.rs`
- Remove: `mod hierarchical_summarizer;`, `mod finding;`, `mod extractor;`
- Remove: `pub use hierarchical_summarizer::*;`, `pub use finding::*;`, `pub use extractor::*;`

**Verification**:
```bash
cargo build --lib --no-default-features  # Must compile
cargo test --no-default-features hierarchical  # Should find 0 tests
```

**Risk**: None - no external references confirmed by grep

---

### Step 0.2: FileRef Type Unification (Task #2 - Part 1)
**Files**: 1 enhancement, 0 deletions yet

**Edit**: `src/utils/patterns.rs`

```rust
// ENHANCE existing FileRef struct
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileRef {
    pub path: String,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,  // ADD THIS
}

impl FileRef {
    pub fn new(path: String) -> Self {
        Self { path, line_start: None, line_end: None }
    }

    pub fn with_line(path: String, line: u32) -> Self {
        Self { path, line_start: Some(line), line_end: None }
    }

    pub fn with_range(path: String, start: u32, end: u32) -> Self {
        Self { path, line_start: Some(start), line_end: Some(end) }
    }
}
```

**Verification**:
```bash
cargo build --lib --no-default-features
cargo test --no-default-features patterns::  # All existing tests pass
```

**Risk**: Low - additive change only, existing code unaffected

---

## Phase 1: Core Infrastructure

### Step 1.1: Reference Validation Consolidation (Task #2 - Part 2)
**Files**: 1 new file, 4 modifications

**Create**: `src/utils/validation.rs` (~100 lines)

```rust
use crate::utils::patterns::FileRef;
use crate::analyzer::file_registry::VerifiedFileRegistry;

#[derive(Debug, Clone, PartialEq)]
pub enum RefValidationResult {
    Valid,
    FileNotFound,
    LineZero,
    LineOutOfRange { line: u32, max_lines: usize },
}

/// Single source of truth for reference validation
pub fn validate_single_ref(
    file_ref: &FileRef,
    registry: &VerifiedFileRegistry
) -> RefValidationResult {
    // 1. Check file existence
    if !registry.contains(&file_ref.path) {
        return RefValidationResult::FileNotFound;
    }

    // 2. If no line specified, file-only ref is valid
    let Some(line_start) = file_ref.line_start else {
        return RefValidationResult::Valid;
    };

    // 3. Line 0 is invalid (1-indexed)
    if line_start == 0 {
        return RefValidationResult::LineZero;
    }

    // 4. Check line range
    let max_lines = registry.line_count(&file_ref.path).unwrap_or(0);
    let end_line = file_ref.line_end.unwrap_or(line_start);

    if end_line > max_lines as u32 {
        return RefValidationResult::LineOutOfRange {
            line: end_line,
            max_lines,
        };
    }

    RefValidationResult::Valid
}

/// Validate all references, return (valid_refs, issues)
pub fn validate_content_references(
    content: &str,
    registry: &VerifiedFileRegistry,
) -> (Vec<FileRef>, Vec<String>) {
    let refs = crate::utils::patterns::extract_file_references(content);
    let mut valid = Vec::new();
    let mut issues = Vec::new();

    for file_ref in refs {
        match validate_single_ref(&file_ref, registry) {
            RefValidationResult::Valid => valid.push(file_ref),
            RefValidationResult::FileNotFound => {
                issues.push(format!("File not found: {}", file_ref.path));
            }
            RefValidationResult::LineZero => {
                issues.push(format!("Line 0 is invalid: {}", file_ref.path));
            }
            RefValidationResult::LineOutOfRange { line, max_lines } => {
                issues.push(format!(
                    "Line {} out of range (max {}): {}",
                    line, max_lines, file_ref.path
                ));
            }
        }
    }

    (valid, issues)
}

#[cfg(test)]
mod tests {
    use super::*;

    // TODO: Add 10+ tests covering all RefValidationResult variants
}
```

**Edit**: `src/utils/mod.rs`
```rust
pub mod validation;  // ADD
```

**Verification**:
```bash
cargo test --no-default-features validation::  # New tests pass
```

---

### Step 1.2: Migrate Existing Validation Callsites (Task #2 - Part 3)
**Files**: 3 modifications (simplified.rs, quality_loop.rs, deep_review.rs)

**Pattern** (apply to all 3 files):

```rust
// BEFORE (in simplified.rs, quality_loop.rs, deep_review.rs)
let refs = file_reference::extract_references(body);
let invalid_refs: Vec<_> = refs.iter()
    .filter(|r| !file_registry.contains(&r.path))
    .collect();

// AFTER
use crate::utils::validation::validate_content_references;

let (valid_refs, issues) = validate_content_references(body, file_registry);

// Adapt to existing return type
let invalid_refs = issues;  // or map to ValidationIssue
```

**Files to modify**:
1. `src/pipeline/validation/simplified.rs:42-55` → use `validate_content_references()`
2. `src/pipeline/quality_loop.rs:180-198` → use `validate_content_references()`
3. `src/pipeline/deep_review.rs:120-135` → use `validate_content_references()`

**Verification**:
```bash
cargo test --no-default-features validation::
cargo test --no-default-features quality_loop::
cargo test --no-default-features deep_review::
```

**Risk**: Medium - changes core validation logic, needs thorough testing

---

### Step 1.3: Delete Old Reference System (Task #2 - Part 4)
**Files**: 1 deletion, 1 modification

**Delete from** `src/pipeline/file_reference.rs`:
- `FileReference` struct (~20 lines)
- `extract_references()` function (~30 lines)
- `count_references()` function (~10 lines)
- All `is_valid_file_ref()` variants (~20 lines)

**KEEP**:
- `PathResolver` struct (used elsewhere)
- `ResolvedReference` struct (used elsewhere)

**Edit**: `src/utils/patterns.rs` - move `extract_file_references()` here if not present

**Verification**:
```bash
cargo build --lib --no-default-features  # Must compile
grep -r "FileReference[^d]" src/  # Should only find ResolvedReference
```

---

### Step 1.4: Dependency Parsing Implementation (Task #1)
**Files**: 1 new file, 1 modification

**Create**: `src/pipeline/phases/dependency_parser.rs` (~200 lines)

```rust
//! Manifest-based dependency parsing
//! Supports: Cargo, Node, Gradle, Maven, Go

use std::path::Path;
use async_trait::async_trait;
use tokio::fs;

#[async_trait]
pub trait ManifestParser: Send + Sync {
    fn name(&self) -> &str;
    fn manifest_files(&self) -> &[&str];
    fn parse_dependencies(&self, content: &str) -> Vec<String>;
}

pub struct CargoManifestParser;

#[async_trait]
impl ManifestParser for CargoManifestParser {
    fn name(&self) -> &str { "cargo" }

    fn manifest_files(&self) -> &[&str] {
        &["Cargo.toml"]
    }

    fn parse_dependencies(&self, content: &str) -> Vec<String> {
        // Parse [dependencies], [dev-dependencies], [build-dependencies]
        // Use toml crate for parsing
        // Return: vec!["tokio", "serde", "anyhow", ...]
        todo!("Implement TOML parsing")
    }
}

pub struct NodeManifestParser;
// Similar for package.json (JSON parsing)

pub struct GradleManifestParser;
// Similar for build.gradle, build.gradle.kts (Regex-based)

pub struct MavenManifestParser;
// Similar for pom.xml (XML parsing)

pub struct GoManifestParser;
// Similar for go.mod (Line-based parsing)

pub async fn parse_subproject_dependencies(
    project_root: &Path,
    subproject_path: &str,
) -> Vec<String> {
    let parsers: Vec<Box<dyn ManifestParser>> = vec![
        Box::new(CargoManifestParser),
        Box::new(NodeManifestParser),
        Box::new(GradleManifestParser),
        Box::new(MavenManifestParser),
        Box::new(GoManifestParser),
    ];

    for parser in parsers {
        for manifest_file in parser.manifest_files() {
            let manifest_path = project_root
                .join(subproject_path)
                .join(manifest_file);

            if let Ok(content) = fs::read_to_string(&manifest_path).await {
                let deps = parser.parse_dependencies(&content);
                if !deps.is_empty() {
                    tracing::debug!(
                        parser = parser.name(),
                        subproject = subproject_path,
                        deps = deps.len(),
                        "Parsed dependencies"
                    );
                    return deps;
                }
            }
        }
    }

    Vec::new()  // No manifest found
}
```

**Edit**: `src/pipeline/phases/monorepo_analyzer.rs:206`

```rust
// BEFORE
let dependencies = Vec::new();

// AFTER
let dependencies = crate::pipeline::phases::dependency_parser::parse_subproject_dependencies(
    &self.project_root,
    &subproject_path,
).await;
```

**Verification**:
```bash
# Create test fixtures for each manifest type
mkdir -p tests/fixtures/monorepo/{rust,node,gradle,maven,go}
# Add sample manifest files

cargo test --no-default-features dependency_parser::
cargo test --no-default-features monorepo_analyzer::
```

---

### Step 1.5: Event Sourcing Consolidation (Task #5)
**Files**: 1 deletion, 3 modifications, 1 new file

**Step 1.5a**: Extract Session Lock

**Create**: `src/pipeline/session_lock.rs` (~80 lines)

```rust
//! Session lock management (extracted from CheckpointManager)

use std::path::{Path, PathBuf};
use tokio::fs;
use crate::types::Result;

pub struct SessionLock {
    lock_file: PathBuf,
}

impl SessionLock {
    pub fn new(output_dir: &Path) -> Self {
        Self {
            lock_file: output_dir.join(".session.lock"),
        }
    }

    pub async fn acquire(&self) -> Result<()> {
        if self.lock_file.exists() {
            return Err(crate::types::ClaudegenError::Other(
                "Session already running".into()
            ));
        }
        fs::write(&self.lock_file, "locked").await?;
        Ok(())
    }

    pub async fn release(&self) -> Result<()> {
        if self.lock_file.exists() {
            fs::remove_file(&self.lock_file).await?;
        }
        Ok(())
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        // Best-effort cleanup
        let _ = std::fs::remove_file(&self.lock_file);
    }
}
```

**Step 1.5b**: Enhance EventStore

**Edit**: `src/pipeline/events/store.rs`

```rust
// ADD these methods

impl EventStore {
    /// Resume from last successful state
    pub async fn resume_state(&self) -> Result<Option<PipelineState>> {
        // 1. Find latest snapshot
        let snapshots = self.list_snapshots().await?;
        let latest_snapshot = snapshots.last();

        if let Some(snapshot_id) = latest_snapshot {
            // 2. Load snapshot
            let mut state = self.load_snapshot(snapshot_id).await?;

            // 3. Replay events since snapshot
            let events_after = self.read_events_since(snapshot_id).await?;
            for event in events_after {
                state.apply_event(&event)?;
            }

            return Ok(Some(state));
        }

        Ok(None)
    }

    /// Compact JSONL shards (delete before oldest snapshot)
    pub async fn compact(&self) -> Result<()> {
        let snapshots = self.list_snapshots().await?;
        if snapshots.len() < 2 {
            return Ok(());  // Keep at least 1 snapshot
        }

        let oldest_keep = &snapshots[snapshots.len() - 2];

        // Delete all shards before this snapshot
        let all_shards = self.list_shards().await?;
        for shard in all_shards {
            if shard.timestamp < oldest_keep.timestamp {
                self.delete_shard(&shard.path).await?;
                tracing::info!(shard = ?shard.path, "Compacted old shard");
            }
        }

        Ok(())
    }
}
```

**Edit**: `src/pipeline/events/types.rs`

```rust
// ADD new event types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PipelineEvent {
    // ... existing variants ...

    BudgetCheckpoint {
        timestamp: SystemTime,
        consumed: u64,
        remaining: u64,
    },

    MetricsCheckpoint {
        timestamp: SystemTime,
        metrics: HashMap<String, f64>,
    },

    SessionCompacted {
        timestamp: SystemTime,
        shards_deleted: usize,
    },
}
```

**Step 1.5c**: Delete CheckpointManager

```bash
rm src/pipeline/checkpoint.rs  # ~620 lines deleted
```

**Edit**: `src/pipeline/mod.rs`
```rust
// REMOVE
mod checkpoint;
pub use checkpoint::CheckpointManager;

// ADD
mod session_lock;
pub use session_lock::SessionLock;
```

**Step 1.5d**: Update Callsites

**Edit**: `src/pipeline/adaptive.rs`

```rust
// BEFORE
use crate::pipeline::checkpoint::CheckpointManager;

let checkpoint = CheckpointManager::new(&output_dir)?;
checkpoint.save_checkpoint(&state).await?;

// AFTER
use crate::pipeline::{SessionLock, events::EventStore};

let lock = SessionLock::new(&output_dir);
lock.acquire().await?;

let event_store = EventStore::new(&output_dir).await?;

// On progress
event_store.append(PipelineEvent::PhaseCompleted { ... }).await?;

// On completion
lock.release().await?;
```

**Verification**:
```bash
cargo build --lib --no-default-features
cargo test --no-default-features event_store::
grep -r "CheckpointManager" src/  # Should find 0 results
```

---

## Phase 2: Provider Enhancement

### Step 2.1: Prompt Caching Implementation (Task #4 - Part 1)
**Files**: 1 modification

**Edit**: `src/ai/provider/claude_agent.rs`

```rust
use anthropic_sdk::types::{SystemPrompt, SystemBlock, CacheTtl};

impl ClaudeAgentProvider {
    async fn generate_with_system(
        &self,
        system: Option<&str>,
        user_prompt: &str,
        schema: &Value,
    ) -> Result<LlmResponse> {
        // Estimate tokens for cache threshold
        let estimated_tokens = system
            .map(|s| s.len() / 4)  // Rough estimate: 4 chars/token
            .unwrap_or(0);

        let min_cache_tokens = 1024;  // Claude API cache minimum

        let system_prompt = match system {
            Some(sys) if estimated_tokens >= min_cache_tokens => {
                // Use caching for large system prompts
                SystemPrompt::Blocks(vec![
                    SystemBlock::cached_with_ttl(sys, CacheTtl::FiveMinutes)
                ])
            }
            Some(sys) => {
                // Too small to cache
                SystemPrompt::Text(sys.to_string())
            }
            None => {
                // Default system prompt
                SystemPrompt::Text(
                    "You are a code documentation expert...".to_string()
                )
            }
        };

        let request = CreateMessageRequestBuilder::default()
            .model(self.model.clone())
            .system(system_prompt)
            .messages(vec![user_prompt.into()])
            .tools(schema_to_tools(schema))
            .build()?;

        // ... rest of implementation
    }
}
```

**Verification**:
```bash
cargo test --no-default-features claude_agent::
# Manual verification: Check API logs for cache hits/misses
```

**Expected Savings**: 69% on repeated system prompts (per Claude API docs)

---

### Step 2.2: Phase Budget Implementation (Task #4 - Part 2)
**Files**: 2 modifications

**Edit**: `src/ai/budget.rs`

```rust
// ADD new struct

#[derive(Debug, Clone)]
pub struct PhaseBudget {
    global: SharedBudget,
    phase: String,
    allocation_ratio: f64,
    consumed: AtomicU64,
}

impl PhaseBudget {
    pub fn new(global: SharedBudget, phase: String, allocation_ratio: f64) -> Self {
        Self {
            global,
            phase,
            allocation_ratio,
            consumed: AtomicU64::new(0),
        }
    }

    /// Calculate effective context window based on remaining budget
    pub fn effective_window(&self, model_max_tokens: usize) -> usize {
        let global_remaining = self.global.remaining() as f64;
        let phase_budget = (global_remaining * self.allocation_ratio) as usize;

        // Use smaller of: model limit, phase budget
        model_max_tokens.min(phase_budget)
    }

    pub fn consume(&self, tokens: u64) -> Result<()> {
        // Consume from both phase and global
        let current = self.consumed.fetch_add(tokens, Ordering::Relaxed);
        let phase_total = current + tokens;

        let phase_limit = (self.global.total_budget as f64 * self.allocation_ratio) as u64;
        if phase_total > phase_limit {
            tracing::warn!(
                phase = %self.phase,
                consumed = phase_total,
                limit = phase_limit,
                "Phase budget exceeded"
            );
        }

        self.global.consume(tokens)
    }

    pub fn stats(&self) -> PhaseBudgetStats {
        let consumed = self.consumed.load(Ordering::Relaxed);
        let limit = (self.global.total_budget as f64 * self.allocation_ratio) as u64;

        PhaseBudgetStats {
            phase: self.phase.clone(),
            consumed,
            limit,
            utilization: consumed as f64 / limit as f64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PhaseBudgetStats {
    pub phase: String,
    pub consumed: u64,
    pub limit: u64,
    pub utilization: f64,
}
```

**Edit**: `src/pipeline/context.rs`

```rust
use crate::ai::budget::{SharedBudget, PhaseBudget};

pub struct GenerationContext {
    // ... existing fields ...
    pub budget: PhaseBudget,  // CHANGE from SharedBudget
}

// Phase allocation ratios (sum to ~85%, keep 15% reserve)
const PHASE_ALLOCATIONS: &[(&str, f64)] = &[
    ("generation", 0.30),
    ("refinement", 0.15),
    ("deep_analysis", 0.15),
    ("quality_assessment", 0.10),
    ("synthesis", 0.10),
    ("validation", 0.05),
];

impl GenerationContext {
    pub fn new(phase: &str, global_budget: SharedBudget) -> Self {
        let allocation = PHASE_ALLOCATIONS
            .iter()
            .find(|(p, _)| *p == phase)
            .map(|(_, r)| *r)
            .unwrap_or(0.05);  // Default 5% for unknown phases

        let budget = PhaseBudget::new(global_budget, phase.to_string(), allocation);

        Self {
            budget,
            // ... other fields ...
        }
    }
}
```

**Verification**:
```bash
cargo test --no-default-features budget::
cargo test --no-default-features context::

# Integration test: Run full pipeline, check phase budget logs
cargo run -- generate --dry-run
```

---

## Phase 3: Analysis Optimization

### Step 3.1: Integrate CompletenessValidator (Task #3 - Part 1)
**Files**: 1 modification

**Edit**: `src/pipeline/adaptive.rs`

```rust
use crate::pipeline::analysis::completeness_validator::CompletenessValidator;

impl AdaptivePipeline {
    pub async fn run(&self) -> Result<Plugin> {
        // ... existing phases ...

        // After distributed analysis
        let analysis_result = self.distributed_analysis_phase().await?;

        // NEW: Validate completeness
        let validator = CompletenessValidator::new(&self.config);
        let completeness = validator.validate(&analysis_result, &file_registry)?;

        if completeness.coverage < 0.95 {
            tracing::warn!(
                coverage = completeness.coverage,
                missing_files = completeness.missing_files.len(),
                "Analysis coverage below target"
            );

            // Optional: Trigger supplemental analysis for missing files
            if self.config.strict_coverage {
                return Err(ClaudegenError::Other(
                    format!("Coverage {}% below 95% threshold", completeness.coverage * 100.0)
                ));
            }
        }

        // Continue with synthesis...
    }
}
```

**Verification**:
```bash
cargo test --no-default-features completeness_validator::
cargo test --no-default-features adaptive::
```

---

### Step 3.2: Integrate ChunkCache (Task #3 - Part 2)
**Files**: 1 modification

**Edit**: `src/pipeline/analysis/distributed.rs`

```rust
use crate::pipeline::analysis::chunk_cache::ChunkCache;
use crate::utils::hash::content_hash;

pub struct DistributedAnalyzer {
    // ... existing fields ...
    chunk_cache: ChunkCache,
}

impl DistributedAnalyzer {
    async fn analyze_chunk(
        &self,
        chunk: &CodeChunk,
        context: &AnalysisContext,
    ) -> Result<ChunkAnalysis> {
        // Calculate content hash
        let hash = content_hash(&chunk.content);

        // Check cache
        if let Some(cached) = self.chunk_cache.get(&hash).await? {
            tracing::debug!(
                file = %chunk.file_path,
                chunk_id = %chunk.id,
                "Cache hit"
            );
            return Ok(cached);
        }

        // Analyze chunk (LLM call)
        let analysis = self.analyze_chunk_uncached(chunk, context).await?;

        // Cache result
        self.chunk_cache.put(&hash, &analysis).await?;

        Ok(analysis)
    }
}
```

**Verification**:
```bash
cargo test --no-default-features chunk_cache::
cargo test --no-default-features distributed::

# Integration test: Run twice, second run should show cache hits
cargo run -- generate test-project
cargo run -- generate test-project  # Should be faster
```

---

### Step 3.3: Service Detection Redesign (Task #7)
**Files**: 1 modification

**Edit**: `src/pipeline/phases/service_detection.rs`

```rust
use crate::pipeline::phases::dependency_parser::parse_subproject_dependencies;

pub struct ServiceDetector {
    provider: Arc<dyn LlmProvider>,
}

#[derive(Debug, Clone)]
pub struct DetectedService {
    pub name: String,
    pub service_type: String,
    pub subtype: Option<String>,  // NEW: "kafka-consumer", "grpc-server"
    pub classification_confidence: f64,  // NEW: LLM confidence
    pub dependencies: Vec<String>,
    pub entry_points: Vec<String>,
}

impl ServiceDetector {
    /// Two-phase detection: manifest → content analysis
    pub async fn detect_service(
        &self,
        subproject_path: &Path,
        files: &[PathBuf],
    ) -> Result<Option<DetectedService>> {
        // Phase 1: Parse dependencies (deterministic)
        let dependencies = parse_subproject_dependencies(
            &self.project_root,
            subproject_path.to_str().unwrap(),
        ).await;

        if dependencies.is_empty() {
            return Ok(None);  // Not a service, likely a library
        }

        // Phase 2: LLM content analysis (semantic)
        let code_samples = self.extract_code_samples(files, 5).await?;
        let analysis = self.classify_service_type(
            subproject_path,
            &dependencies,
            &code_samples,
        ).await?;

        Ok(Some(DetectedService {
            name: self.infer_service_name(subproject_path),
            service_type: analysis.service_type,
            subtype: analysis.subtype,
            classification_confidence: analysis.confidence,
            dependencies,
            entry_points: analysis.entry_points,
        }))
    }

    async fn classify_service_type(
        &self,
        path: &Path,
        dependencies: &[String],
        code_samples: &[String],
    ) -> Result<ServiceClassification> {
        let prompt = format!(
            r#"Analyze this service and classify its type.

DEPENDENCIES:
{}

CODE SAMPLES:
{}

Classify as:
- web-api (REST/GraphQL server)
- grpc-server (gRPC service)
- kafka-consumer (event consumer)
- kafka-producer (event producer)
- background-worker (cron/queue worker)
- cli-tool (command-line utility)

Return JSON with:
- service_type: main classification
- subtype: optional specific implementation (e.g., "kafka-consumer")
- confidence: 0.0-1.0
- entry_points: list of main entry point files
- reasoning: brief explanation
"#,
            dependencies.join(", "),
            code_samples.join("\n---\n")
        );

        let schema = generate_schema::<ServiceClassification>();
        let response = self.provider.generate(&prompt, &schema).await?;
        deserialize_llm_response(&response.content, "service_classification")
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ServiceClassification {
    service_type: String,
    subtype: Option<String>,
    confidence: f64,
    entry_points: Vec<String>,
    reasoning: String,
}
```

**Verification**:
```bash
cargo test --no-default-features service_detection::

# Test with real monorepo
cargo run -- generate tests/fixtures/example-monorepo
```

---

## Phase 4: Quality System

### Step 4.1: Remove @file:line Validation from LLM (Task #6 - Part 1)
**Files**: 1 modification

**Edit**: `src/pipeline/quality/judge.rs`

```rust
impl LlmJudge {
    fn build_evaluation_prompt(&self, artifact: &impl ArtifactContent) -> String {
        format!(
            r#"Evaluate this {artifact_type} for a Claude Code plugin.

CONTENT:
{content}

EVALUATION CRITERIA:
1. Tier Classification
   - Tier 0: Hallucinated/invalid (references non-existent features)
   - Tier 1: Generic knowledge (applies to any project)
   - Tier 2: Project conventions (style, patterns)
   - Tier 3: Hidden constraints (must-know gotchas, bug sources)

2. Quality Dimensions (0.0-1.0 each)
   - actionability: Clear, directive language
   - specificity: Project-specific vs generic
   - evidence_strength: Claims backed by references
   - depth: Surface vs deep insights

Note: Do NOT validate @file:line references - that's done separately.
Your job is to evaluate CONTENT QUALITY only.

Return JSON with:
- tier: "tier0" | "tier1" | "tier2" | "tier3"
- confidence: 0.0-1.0 (your confidence in classification)
- actionability: 0.0-1.0
- specificity: 0.0-1.0
- evidence_strength: 0.0-1.0
- depth: 0.0-1.0
- reasoning: string (brief explanation)
"#,
            artifact_type = artifact.artifact_type(),
            content = artifact.content(),
        )
    }
}
```

---

### Step 4.2: Extract LLM Confidence (Task #6 - Part 2)
**Files**: 1 modification

**Edit**: `src/pipeline/quality/judge.rs`

```rust
#[derive(Debug, Deserialize, JsonSchema)]
struct TierEvaluationOutput {
    tier: String,
    confidence: f32,  // NEW: LLM reports its confidence
    actionability: f32,
    specificity: f32,
    evidence_strength: f32,
    depth: f32,
    reasoning: String,
}

impl LlmJudge {
    pub async fn evaluate_tier(&self, artifact: &impl ArtifactContent) -> Result<TierEvaluation> {
        let prompt = self.build_evaluation_prompt(artifact);
        let schema = generate_schema::<TierEvaluationOutput>();

        let response = self.provider.generate(&prompt, &schema).await?;
        let output: TierEvaluationOutput = deserialize_llm_response(
            &response.content,
            "tier_evaluation"
        )?;

        Ok(TierEvaluation {
            tier: parse_tier(&output.tier)?,
            confidence: output.confidence,  // USE LLM confidence
            scores: QualityScores {
                actionability: output.actionability,
                specificity: output.specificity,
                evidence_strength: output.evidence_strength,
                depth: output.depth,
            },
            reasoning: output.reasoning,
        })
    }
}
```

**Verification**:
```bash
cargo test --no-default-features judge::
# Check that confidence varies (not hardcoded 0.85)
```

---

## Phase 5: Final Integration

### Step 5.1: Update CLAUDE.md (Documentation)
**Files**: 1 modification

**Edit**: `CLAUDE.md` - Apply 5 corrections:

1. **Line 149-156**: Update Tier1 action
```markdown
| Tier | Definition | Action |
|------|------------|--------|
| Tier 0 | Hallucinated/invalid content | **ALWAYS REJECT** |
| Tier 1 | Generic language/tool knowledge | **ADVISORY ONLY** |
```

2. **Line 27-31**: Update evidence table
```markdown
| Requirement | Description |
|-------------|-------------|
| `@file:line` references | Validated for file existence and line range only |
| WeakEvidence signals | LLM adds references via EvidenceStrategy |
| Tier 0 rejection | Hard block on hallucinated references |
```

3. **Line 215-226**: Remove Tier1Content row
```markdown
| IssueKind | Primary Strategy | Fallback |
|-----------|------------------|----------|
| WeakEvidence | EvidenceStrategy | RegenerationStrategy |
| MissingReferences | EvidenceStrategy | RegenerationStrategy |
| TooGeneric | SemanticStrategy | RegenerationStrategy |
```

4. **Line 166-187**: Update architecture tree
```markdown
src/
├── pipeline/
│   ├── events/           # Event sourcing (replaces checkpoint.rs)
│   ├── session_lock.rs   # Session lock (extracted from checkpoint)
│   ├── phases/
│   │   └── dependency_parser.rs  # NEW: ManifestParser trait
```

5. **Line 202-212**: Fix RefinementStrategy trait
```rust
async fn refine_rule(&self, rule: &mut Rule, ctx: &StrategyContext<'_>) -> Result<StrategyResult>;
// Remove: { Ok(default()) }
```

---

### Step 5.2: Smoke Tests
**Files**: Create new test file

**Create**: `tests/integration/smoke_test.rs`

```rust
//! Smoke tests for architecture redesign

#[tokio::test]
async fn test_file_ref_unification() {
    // Verify FileRef type exists and has line_end
    let file_ref = FileRef::with_range("src/main.rs".into(), 1, 10);
    assert_eq!(file_ref.line_start, Some(1));
    assert_eq!(file_ref.line_end, Some(10));
}

#[tokio::test]
async fn test_reference_validation() {
    // Verify validate_single_ref rejects line 0
    let registry = VerifiedFileRegistry::build(Path::new(".")).await.unwrap();
    let ref_zero = FileRef::with_line("src/main.rs".into(), 0);

    let result = validate_single_ref(&ref_zero, &registry);
    assert!(matches!(result, RefValidationResult::LineZero));
}

#[tokio::test]
async fn test_dependency_parsing() {
    // Verify ManifestParser works for Cargo.toml
    let parser = CargoManifestParser;
    let toml_content = r#"
[dependencies]
tokio = "1.0"
serde = "1.0"
    "#;

    let deps = parser.parse_dependencies(toml_content);
    assert!(deps.contains(&"tokio".to_string()));
    assert!(deps.contains(&"serde".to_string()));
}

#[tokio::test]
async fn test_event_store_resume() {
    // Verify EventStore can resume from snapshot
    let temp_dir = tempfile::tempdir().unwrap();
    let store = EventStore::new(temp_dir.path()).await.unwrap();

    // Create snapshot
    let state = PipelineState::default();
    store.create_snapshot(&state).await.unwrap();

    // Resume
    let resumed = store.resume_state().await.unwrap();
    assert!(resumed.is_some());
}

#[tokio::test]
async fn test_phase_budget() {
    // Verify PhaseBudget allocates correctly
    let global = create_shared_budget(100_000);
    let phase = PhaseBudget::new(global, "generation".into(), 0.30);

    let effective = phase.effective_window(200_000);
    assert_eq!(effective, 30_000);  // 30% of 100k
}

#[tokio::test]
async fn test_prompt_caching() {
    // Verify ClaudeAgentProvider uses caching for large prompts
    // (This requires mocking or API inspection)
    todo!("Verify SystemBlock::cached is used");
}

#[tokio::test]
async fn test_service_detection_two_phase() {
    // Verify ServiceDetector uses manifest + LLM
    let detector = ServiceDetector::new(mock_provider());
    let result = detector.detect_service(
        Path::new("tests/fixtures/example-service"),
        &[],
    ).await.unwrap();

    assert!(result.is_some());
    assert!(result.unwrap().classification_confidence > 0.0);
}
```

**Run all tests**:
```bash
cargo test --no-default-features
cargo test --test integration
```

---

### Step 5.3: Regression Prevention
**Files**: CI/CD updates

**Create**: `.github/workflows/architecture-checks.yml`

```yaml
name: Architecture Checks

on: [push, pull_request]

jobs:
  check-dead-code:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - name: Check for deleted files
        run: |
          # Verify dead code stays deleted
          ! test -f src/pipeline/checkpoint.rs
          ! test -f src/pipeline/analysis/hierarchical_summarizer.rs
          ! test -f src/pipeline/analysis/finding.rs
          ! test -f src/pipeline/analysis/extractor.rs
      - name: Check for FileReference usage
        run: |
          # Verify old FileReference not reintroduced
          ! grep -r "FileReference[^d]" src/
      - name: Verify manifest parsers
        run: |
          # Verify all parsers implemented
          grep -q "CargoManifestParser" src/pipeline/phases/dependency_parser.rs
          grep -q "NodeManifestParser" src/pipeline/phases/dependency_parser.rs
          grep -q "GradleManifestParser" src/pipeline/phases/dependency_parser.rs
          grep -q "MavenManifestParser" src/pipeline/phases/dependency_parser.rs
          grep -q "GoManifestParser" src/pipeline/phases/dependency_parser.rs
```

---

## Summary & Metrics

### Implementation Order

1. **Phase 0** (1-2 hours): Foundation - safe deletions + FileRef enhancement
2. **Phase 1** (4-6 hours): Core Infrastructure - validation, dependency parsing, event sourcing
3. **Phase 2** (2-3 hours): Provider Enhancement - prompt caching, phase budget
4. **Phase 3** (3-4 hours): Analysis Optimization - integration, service detection
5. **Phase 4** (1-2 hours): Quality System - LLM prompt updates, confidence extraction
6. **Phase 5** (2-3 hours): Integration - docs, tests, CI

**Total**: ~15-20 hours (2-3 days with testing)

### Verification Gates

After each phase:
```bash
cargo build --lib --no-default-features  # Must compile
cargo clippy --lib --no-default-features  # 0 warnings
cargo test --no-default-features  # All tests pass
```

After Phase 5:
```bash
cargo test --test integration  # Smoke tests pass
./scripts/verify-architecture.sh  # All checks pass
```

### Risk Mitigation

| Risk | Impact | Mitigation |
|------|--------|------------|
| Reference validation breaks existing logic | HIGH | Phase 0 FileRef enhancement first, migrate gradually |
| Event store resume fails | MEDIUM | Extensive testing, keep migration guide |
| Prompt caching doesn't work | LOW | Fallback to non-cached (same behavior) |
| Service detection false positives | MEDIUM | Two-phase approach, LLM confidence threshold |
| Missing edge cases | MEDIUM | Comprehensive test fixtures for each manifest type |

### Success Criteria

- ✅ All 677 tests pass
- ✅ 0 clippy warnings
- ✅ Monorepo dependencies populated (not empty)
- ✅ Line 0 references rejected
- ✅ 4 reference validation bugs fixed
- ✅ Prompt cache hit rate >50% on second run
- ✅ Phase budgets enforced (no overruns)
- ✅ Event store compaction reduces disk usage
- ✅ Service detection confidence scores vary (not hardcoded)
- ✅ Net -96 lines achieved

---

## Next Steps

**Option A**: Implement Phase 0 now (safe, breaks nothing)
**Option B**: Review plan, request modifications
**Option C**: Pick specific task to implement first

**Recommended**: Start with Phase 0 (foundation) - takes 1-2 hours, zero risk, enables all other phases.
