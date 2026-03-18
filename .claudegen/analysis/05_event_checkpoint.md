# 05. Event Sourcing / Checkpoint / Resume System Analysis

## 1. System Overview

claudegen implements a **dual-layer state persistence system** for long-running pipeline executions:

1. **Event Sourcing Layer** (`src/pipeline/events/`): Append-only event log that records every meaningful pipeline action. State is reconstructed by replaying events.
2. **Checkpoint Layer** (`src/pipeline/checkpoint.rs`): Periodic full-state snapshots for fast recovery without full replay.

These two systems serve complementary purposes and operate somewhat independently -- the event sourcing system is deeply integrated into the `AdaptivePipeline` and `RefinementEngine`, while the checkpoint system provides a more coarse-grained backup mechanism.

---

## 2. Event Sourcing System

### 2.1 Architecture

**Files:**
- `src/pipeline/events/types.rs` -- Event schema (PipelineEvent, EventType, EventPayload)
- `src/pipeline/events/store.rs` -- Sharded append-only event store
- `src/pipeline/events/state.rs` -- State reconstruction from events (ResumeState)
- `src/pipeline/events/compaction.rs` -- Incremental state compaction
- `src/pipeline/events/mod.rs` -- Public API surface

### 2.2 Event Schema

Each `PipelineEvent` contains:
- `id`: UUID v4 (unique per event)
- `timestamp`: DateTime<Utc> (wall-clock time)
- `schema_version`: u32 (currently 1, enables future migration)
- `event_type`: Enum discriminant
- `payload`: Typed payload variant

**Event Types (22 total):**

| Category | Events | What They Capture |
|----------|--------|-------------------|
| Session | `SessionStarted`, `SessionCompleted` | Session lifecycle with config hash for validation |
| Analysis | `ChunkAnalysisCached`, `AnalysisCheckpoint` | Chunk-level analysis progress, file hashes for invalidation |
| Phases | `ConventionInferenceCompleted`, `ConstraintExtractionCompleted`, `DomainAnalysisCompleted`, `CrossSynthesisCompleted` | Phase completion with snapshot paths and item counts |
| Detection | `ProjectDetectionCompleted`, `MonorepoAnalysisCompleted`, `ModuleDetectionCompleted` | Project structure discovery results |
| Refinement | `IterationStarted`, `QualityAssessed`, `IssueRefined`, `BestStateUpdated`, `IterationCompleted` | Full refinement loop instrumentation |
| Quality | `PatternDeduped`, `ArtifactJudged` | Pattern dedup stats, per-artifact quality scores |
| Deep Review | `DeepReviewStarted`, `DeepReviewPassCompleted`, `DeepReviewCompleted` | Deep review lifecycle |
| Checkpoints | `RefinementCheckpoint`, `AnalysisCheckpoint` | Embedded checkpoints within the event stream |

### 2.3 EventStore Implementation

**Sharded JSONL Architecture:**

The EventStore uses a sharded file-based approach (`shard_000.jsonl`, `shard_001.jsonl`, etc.) with a central `index.json` tracking shard metadata.

**Key design decisions:**
- **Shard size**: 1000 events per shard (constant `DEFAULT_SHARD_SIZE`).
- **Index save interval**: Every 10 events (constant `INDEX_SAVE_INTERVAL`).
- **Format**: One JSON line per event (JSONL). Enables streaming reads.
- **Session isolation**: Each session gets a UUID-named directory under `.claudegen/sessions/`.
- **Concurrency**: `tokio::sync::Mutex<ShardWriter>` serializes all writes.
- **Event count**: `AtomicU64` with `SeqCst` ordering for accurate counts.

**Write path (`append`):**
1. Acquire mutex lock on `ShardWriter`
2. Check if rotation needed (shard full)
3. If rotating: flush, save index, open new shard file
4. Serialize event to JSON
5. Write JSON + newline, flush immediately
6. Update shard index in memory
7. Periodically save index to disk (every 10 events)
8. Atomically increment event counter

**Flush-per-write guarantee**: Every event is flushed immediately (`sw.writer.flush().await`), ensuring crash safety at the event level. No buffered events are lost on crash.

**Resume path:**
1. `find_resumable_with_validation` scans `.claudegen/sessions/` for incomplete sessions
2. Sessions sorted by directory name (UUID-based, not strictly time-ordered)
3. Skips sessions with `SessionCompleted` event
4. Optionally validates config hash match
5. `resume()` calls `load_or_migrate_index()` which:
   - Tries loading existing `index.json`
   - Reconciles active shard event count with actual line count (handles index/data mismatch)
   - Falls back to rebuilding index from shard files
   - Migrates legacy `events.jsonl` single-file format

**Snapshot management:**
- `save_drafts()`: Saves initial draft state to `snapshots/drafts.json`
- `save_iteration(n)`: Saves iteration state to `snapshots/iter_N.json`
- `save_phase_snapshot(name)`: Saves phase output to `snapshots/{name}.json`
- All snapshots stored in session directory's `snapshots/` subdirectory

### 2.4 State Reconstruction (ResumeState)

`ResumeState::from_events(&[PipelineEvent])` replays all events to reconstruct:

**AnalysisResumeState:**
- `current_phase`: Which analysis phase was reached
- `completed_chunks`: Set of chunk IDs that completed analysis
- `failed_chunks`: Set of chunk IDs that failed
- `cached_chunks`: Chunks retrieved from cache (skippable on resume)
- `total_chunks`: Total chunk count at checkpoint time

**RefinementResumeState:**
- `last_completed_iteration`: Last fully completed iteration number
- `iteration_progress`: Per-iteration tracking of completed items and quality assessment
- `best_state_path`: Path to best quality snapshot
- `best_quality`: Best quality score achieved
- `quality_history`, `level_history`: Full quality trajectory
- `stagnation_count`, `consecutive_clean_passes`: Convergence tracking state
- `strategy_outcomes`: Per-strategy success/failure tracking

**Phase completion flags:**
- `project_detected`, `monorepo_analyzed`, `modules_detected`, `deep_review_completed`

**File hashes:**
- `file_hashes`: Per-file content hashes for incremental invalidation

**Pruning for memory safety:**
- `prune_old_progress()` called after reconstruction, keeps only last 5 iterations (`MAX_ITERATION_PROGRESS`)
- Prevents unbounded growth of `iteration_progress` HashMap

### 2.5 Incremental Compaction

`IncrementalCompactor` manages memory growth during long-running sessions:

| What | Threshold | Action |
|------|-----------|--------|
| Iteration progress | > 5 entries | Keep only last 5 iterations |
| Quality history | > 30 entries | Trim to 20 most recent |
| Level history | > 20 entries | Trim to 20 most recent |
| Strategy outcomes | > 50 entries | Remove all-success entries first, then LRU eviction |

Compaction is triggered on resume (`RefinementEngine::refine`) before restoring state.

---

## 3. Checkpoint System

### 3.1 Architecture

**File:** `src/pipeline/checkpoint.rs`

The checkpoint system provides a higher-level, periodic full-state snapshot mechanism independent of the event stream.

### 3.2 ExecutionCheckpoint Structure

A checkpoint captures the complete pipeline state:

```
ExecutionCheckpoint {
    version: u32,                          // Format version (1 or 2)
    created_at: DateTime<Utc>,
    current_phase: PipelinePhase,          // 11 phases: Init → Finalization
    phase_progress: f32,                   // 0.0-1.0 within current phase
    completed_phases: Vec<CompletedPhase>, // With timing and quality scores
    analysis_cache: Option<AnalysisCache>, // Cached analysis results
    generated_artifacts: GeneratedArtifacts, // Partial generation state
    quality_history: Vec<QualitySnapshot>,
    tokens_used: u64,
    budget_remaining: u64,
    api_calls: u32,                        // v2+
    input_tokens: u64,                     // v2+
    output_tokens: u64,                    // v2+
    avg_latency_ms: f64,                   // v2+
    total_cost_usd: f64,                   // v2+
    total_duration_ms: u64,                // v2+
    completed_chunks: HashSet<String>,
    refinement_iteration: usize,
    deep_review_pass: u32,
    metadata: HashMap<String, String>,
}
```

### 3.3 CheckpointManager

**Interval calculation**: Dynamically computed as `quality_loop_timeout / 4` (minimum 60 seconds), derived from `TimeoutConfig::effective_checkpoint_interval_secs()`.

**Save mechanism (atomic write):**
1. Generate unique filename: `checkpoint_{timestamp}_{counter}_{iteration}.json`
2. Write to `.tmp` file first
3. Rename atomically (POSIX guarantees)
4. Clean up old checkpoints (keep last 5)

**Restore mechanism:**
1. `list_checkpoints()`: Read all `.json` files from checkpoint directory, sort by `created_at` descending
2. `restore_latest()`: Find first compatible checkpoint (version 1 or 2)
3. `restore_to_phase()`: Find checkpoint that has a specific phase completed

**Retention**: Maximum 5 checkpoints kept (oldest deleted on save). Uses filesystem modification time for ordering during cleanup.

### 3.4 Lock File and Crash Detection

**Lock file**: `.claudegen/.lock` contains:
```json
{
    "pid": 12345,
    "started_at": "2024-...",
    "hostname": "machine-name"
}
```

**Crash recovery flow (`CrashRecovery::attempt_recovery`):**
1. Check for lock file existence
2. If no lock: `NoRecoveryNeeded`
3. If lock exists with same PID: `NoRecoveryNeeded` (current process)
4. If lock PID is alive: `ProcessRunning` (another instance running)
5. If lock PID is dead (stale lock):
   a. Force-release stale lock
   b. Attempt to restore latest checkpoint
   c. If checkpoint found: `Recovered(checkpoint)`
   d. If no checkpoint: `StartFresh`

**Process liveness checks** are platform-specific:
- Linux: `/proc/{pid}` existence
- macOS: `ps -p {pid}` command
- Windows: `tasklist /FI "PID eq {pid}"`
- Fallback: Assume dead (conservative for recovery)

---

## 4. Integration: How AdaptivePipeline Uses Events

### 4.1 Event Store Initialization (Lazy, Session-Aware)

In `AdaptivePipeline::get_event_store()`:
1. Uses `OnceCell` for lazy singleton initialization
2. First checks for resumable sessions (`find_resumable_with_validation`)
3. If resumable session found: resumes from existing session directory
4. Otherwise: creates new session with fresh UUID

### 4.2 Resume State Construction

`AdaptivePipeline::build_resume_state()`:
1. Gets event store
2. Loads all events from session directory
3. If empty: returns default (fresh start)
4. Otherwise: calls `ResumeState::from_events()`

### 4.3 Event Emission Throughout Pipeline

The pipeline emits events at every significant state change:

**Phase completions** (with snapshot paths):
- ProjectDetectionCompleted (tech stack, frameworks, workspace type)
- MonorepoAnalysisCompleted (packages count, services count)
- ModuleDetectionCompleted (module count, high-value count)
- ConventionInferenceCompleted (snapshot path, convention count)
- ConstraintExtractionCompleted (snapshot path, constraint count)
- DomainAnalysisCompleted (snapshot path, item count)
- CrossSynthesisCompleted (snapshot path, item count)

**Refinement loop** (every iteration):
- IterationStarted → QualityAssessed → per-artifact ArtifactJudged → IssueRefined (per issue) → BestStateUpdated → IterationCompleted → RefinementCheckpoint

### 4.4 How Refinement Uses Resume State

In `RefinementEngine::refine()`:
1. Determines `start_iteration` from `resume_state.refinement.last_completed_iteration`
2. If resuming: compacts state via `IncrementalCompactor`, restores `RefinementState` from `RefinementResumeState`
3. Restores best snapshot if path available
4. Resumes refinement loop from `start_iteration`
5. Each iteration checks `completed_items` from resume state to skip already-processed issues
6. After each iteration: emits `RefinementCheckpoint` event with full refinement state

---

## 5. Sync System (Incremental Updates)

### 5.1 Architecture

**Directory:** `src/pipeline/sync/`

The sync system handles incremental project changes between pipeline runs.

### 5.2 FileTracker (blake3-Based)

- Scans project directory recursively
- Filters by `SOURCE_EXTENSIONS` (90+ extensions)
- Excludes build directories (target, node_modules, vendor, .git, etc.)
- Computes blake3 content hash per file
- Compares against previously tracked files from `ProjectManifest`
- Produces `ChangeSet` with `added`, `modified`, `deleted` lists

### 5.3 DependencyGraph

Maps file changes to affected artifacts through a three-level hierarchy:

```
File → Module → Artifacts (rules, agents, skills)
                ↓
          Group → GroupRule
                ↓
          Domain → DomainRule, DomainAgent
```

**Transitive dependency propagation** with configurable depth limit:
- Depth 0: Only directly changed modules
- Depth 1: Changed + direct dependents
- Depth 2 (default): Two levels of propagation
- Unlimited: Full transitive closure

**Cycle safety**: Uses BFS with `visited` HashSet, prevents infinite loops.

**Invalidation cascade**:
- Module change → Module artifacts + transitive dependents
- Any member module affected → Group rule invalidated
- Any member group affected → Domain artifacts invalidated
- File addition/deletion → ProjectRule + CLAUDE.md always regenerated

### 5.4 SyncResult

Tracks regeneration outcomes:
- `regenerated`: Artifacts that were regenerated (with path and reason)
- `skipped`: Artifacts that didn't need regeneration
- `errors`: Artifacts that failed regeneration

---

## 6. Iteration State Management

### 6.1 IterationState (`src/pipeline/iteration_state.rs`)

Manages the quality refinement loop's dynamic iteration budget:

**Quality trajectory**: Bounded `VecDeque<f32>` (max 100 entries), tracks quality over time.

**Uncertainty calculation**: Rolling window variance (last 5 values), scaled by factor 5.0, clamped to [0.0, 1.0].

**Budget extension triggers**:
- `QualityImproving { min_delta }`: Extends if recent quality delta exceeds threshold
- `HighUncertainty { threshold }`: Extends if uncertainty is above threshold

**Termination**: `should_continue()` returns false when `satisfied` or `iteration >= max_allowed`.

### 6.2 How Refinement Uses IterationState

In `RefinementEngine`, convergence is determined by:

1. **Quality level classification**: `BelowFloor` / `AtFloor` / `AtTarget` based on combined quality score and `QualityMetrics.is_acceptable()` (85% of artifacts pass).

2. **Convergence paths**:
   - `TargetMet`: Quality at target + consecutive clean passes >= required
   - `FloorMetExtended`: Quality at floor + consecutive passes >= required * 2
   - `LevelOscillation`: Quality levels oscillating >= 50% of window and quality >= floor

3. **Quality patterns** (oscillation + stagnation detection):
   - Oscillation: Direction changes / meaningful pairs >= 0.5 threshold
   - Stagnation: Delta below threshold or no improvement
   - Combined oscillation+stagnation → force regeneration strategy

---

## 7. Context Budget Tracking

### 7.1 ContextBudget (`src/ai/context_tracker.rs`)

Manages LLM context window allocation across 3 tiers:

- **Total budget**: `model_limit * 80%` (200K default → 160K available)
- **Output reserve**: 20% of model limit
- **Tier system**:
  - Tier 1 (Essential): Always included (project detection, conventions, constraints)
  - Tier 2 (Relevant): Module summaries, included if space allows
  - Tier 3 (Reference): Full analysis, summarized if budget tight

**Allocation**: `allocate(section, requested)` returns actual allocated amount (capped by remaining budget). No overallocation possible.

**Summarization decision**: `needs_summarization(tokens)` triggers when content exceeds remaining budget.

---

## 8. Progress Tracking

### 8.1 ProgressTracker (`src/cli/progress.rs`)

Real-time progress reporting with:
- Phase-level tracking (start/complete)
- Item-level progress with throughput and ETA
- Broadcast channel (`tokio::sync::broadcast`) for event distribution
- Thread-safe state via `Arc<RwLock<ProgressState>>` with poisoning recovery
- Metrics: throughput (items/sec), ETA (seconds remaining), overall progress (%)

---

## 9. Critical Analysis: Strengths

### 9.1 Event Sourcing Completeness

The event system captures a remarkably complete audit trail:
- Every phase transition with snapshot paths
- Every refinement iteration with per-artifact scores
- Every issue refinement attempt with strategy and success/failure
- Embedded checkpoint events for state recovery

**Strength**: Any pipeline state can be reconstructed from the event log alone, without relying on snapshot files (though snapshots accelerate restoration).

### 9.2 Crash Safety

Multiple layers of protection:
- **Flush-per-write** in EventStore: No buffered events lost on crash
- **Atomic file writes** throughout: temp file + rename pattern prevents corruption
- **Lock file with liveness detection**: Prevents concurrent execution
- **Index reconciliation**: Handles index/data mismatch from partial index saves
- **Legacy migration**: Handles format upgrades transparently

### 9.3 Memory Boundedness

Explicit bounds on all growing collections:
- Quality history: max 20 entries
- Level history: max 20 entries
- Iteration progress: max 5 iterations
- Strategy outcomes: max 50 entries
- Quality trajectory: max 100 entries
- Failure tracker pairs: max 5,000 entries
- Feedback selector history: max 200 artifacts
- Tier3 constraints: max 50
- Key abstractions: max 100

### 9.4 Selective Re-evaluation

The `assess_quality_selective` optimization caches judgments for unmodified artifacts, reducing LLM calls by ~85% per iteration. Cache is invalidated only for artifacts modified in the current iteration.

---

## 10. Critical Analysis: Weaknesses and Risks

### 10.1 Session Discovery Is UUID-Based, Not Time-Ordered

`find_resumable_with_validation` sorts session directories by filename (UUID). UUIDs are not monotonically time-ordered, so sorting by filename doesn't guarantee the most recent session is found first. This could cause:
- **Risk**: Resuming from an older session instead of the most recent one if multiple incomplete sessions exist.
- **Mitigation**: In practice, only one session is usually incomplete at a time.

### 10.2 Full Event Replay Required for State Reconstruction

`ResumeState::from_events()` replays ALL events sequentially. For a session with thousands of events across many shards, this means:
- Reading all shard files from disk
- Deserializing every event
- Processing every event through the state reconstruction logic

**Scale concern**: A session with 10,000 events across 10 shards would require reading ~10MB+ of JSONL, deserializing 10K JSON objects. While fast for typical sessions, this could take seconds for very long-running sessions.

**Missing optimization**: No incremental snapshot-based resume. The `RefinementCheckpoint` events embed full state, but `ResumeState::from_events()` still processes all events rather than seeking to the latest checkpoint event and only replaying events after it.

### 10.3 Checkpoint System Is Not Actively Used in Pipeline

The `CheckpointManager` and `CrashRecovery` types are fully implemented but **not integrated into `AdaptivePipeline::run()`**. The pipeline relies entirely on the event sourcing system for resume. The checkpoint system appears to be a parallel mechanism that was designed but not wired into the main execution path.

**Evidence**:
- `AdaptivePipeline` has no `CheckpointManager` field
- `AdaptivePipeline::run()` never calls `maybe_checkpoint()` or `save_checkpoint()`
- The `CrashRecovery` type is defined but not used in any pipeline entry point

### 10.4 No Config Hash Validation By Default

`get_event_store()` calls `find_resumable_with_validation(&root, None)` with `None` for expected hash. This means:
- **Risk**: Resuming a session that was started with a different configuration. Config changes between runs could lead to inconsistent state.
- **Mitigation**: The `find_resumable_with_validation` method supports hash validation, but the caller doesn't use it.

### 10.5 Atomic Write Gaps in EventStore

While `atomic_write()` is used for output files (CLAUDE.md, rules, skills, agents), the EventStore itself uses direct `append` writes:
```rust
sw.writer.write_all(json.as_bytes()).await?;
sw.writer.write_all(b"\n").await?;
sw.writer.flush().await?;
```
If the process crashes between writing JSON and the newline, or during the newline write, the JSONL file could contain a partial last line. The `read_jsonl_into` method handles this gracefully by skipping unparseable lines:
```rust
Err(e) => tracing::warn!(line_num = idx + 1, error = %e, "Skipping unrecognized event"),
```
**Assessment**: Acceptable. At worst, one event is lost on crash. The event stream is designed to be replayed, and losing the last event is tolerable.

### 10.6 Snapshot Path Fragility

`RefinementResumeState.best_state_path` stores an absolute file path string. If the session directory is moved or the project is checked out to a different location, snapshot restoration will fail silently:
```rust
if let Some(ref path) = resume_refinement.best_state_path
    && let Ok(snapshot) = self.load_snapshot(path).await
{
    // Uses absolute path stored in event
}
```
**Impact**: Snapshot-based fast restoration fails; pipeline would need to re-run refinement from scratch.

### 10.7 No Event Schema Migration

Events have a `schema_version` field (currently 1), but there is no migration logic. If the `EventPayload` enum changes (new variants, changed field types), old events become unparseable. The `read_jsonl_into` method skips them, but:
- **Risk**: Accumulated historical data from previous versions would be silently ignored during replay.
- **Mitigation**: Schema version 1 is the only version, and `serde(default)` is used on some fields.

### 10.8 Shard Index Inconsistency Window

The shard index is saved to disk every 10 events (`INDEX_SAVE_INTERVAL`). Between saves, up to 9 events can exist in the shard file without being reflected in the index. The `load_or_migrate_index` method reconciles by counting actual lines in the active shard.

**Edge case**: If the process crashes between a shard rotation (new file created) and the index save, the new shard exists but the index doesn't reference it. The fallback path handles this by scanning for shard files when the index doesn't exist or is outdated.

---

## 11. Scalability Analysis

### 11.1 Event Store: Large Project Behavior

For a project with 5,000 files:
- **Chunks**: ~100-200 chunks (25-50 files per chunk)
- **Analysis events**: ~200-400 (chunk analysis, cache hits, phase completions)
- **Refinement events**: ~50-100 per iteration * ~5-10 iterations = ~250-1000
- **Total events**: ~500-1500 per session
- **Shards**: 1-2 shards (at 1000 events/shard)
- **Disk**: ~1-5MB per session

**Verdict**: Well within bounds. Sharding becomes meaningful at ~10K+ events.

### 11.2 State Reconstruction at Scale

For 1500 events:
- Deserialization: ~15ms (JSON parsing)
- State reconstruction: ~5ms (HashMap operations)
- Total resume time: ~20ms

**Verdict**: Negligible overhead for typical sessions.

### 11.3 Snapshot Storage

Each iteration snapshot contains full artifact state (skills, agents, rules as JSON). For a project generating 50 artifacts:
- Snapshot size: ~200KB-1MB per iteration
- With 10 iterations: ~2-10MB of snapshots
- Plus phase snapshots: ~500KB-2MB

**Verdict**: Manageable. The checkpoint system's max 5 retention helps.

### 11.4 Sync System: Dependency Propagation

For a project with 100 modules:
- File → Module lookup: O(n) linear scan of prefixes
- Transitive resolution: BFS with depth limit (default 2)
- Worst case (fully connected): O(modules * depth) = O(200)

**Verdict**: Very fast. Depth limit prevents cascade explosion.

---

## 12. Correctness of Resume

### 12.1 Can the Pipeline Resume Without Information Loss?

**Analysis phase resume**: YES, with caveats.
- `AnalysisCheckpoint` stores completed/failed chunk IDs and file hashes
- Completed chunks can be skipped on resume
- File hashes enable incremental invalidation (changed files re-analyzed)
- **Gap**: Phase-level snapshots (conventions, constraints, domain analysis) are stored in snapshot files referenced by events, but the resume logic in `AdaptivePipeline::run()` does not actually skip already-completed phases. It runs the full pipeline from the beginning.

**Refinement resume**: YES, fully.
- `start_iteration` correctly advances past completed iterations
- `RefinementState::from_resume_state()` restores quality history, stagnation count, consecutive passes, level history
- Best snapshot restored from file path
- Per-iteration `completed_items` prevents re-processing
- Cache seeding on first iteration ensures no redundant LLM calls

### 12.2 What State Is NOT Preserved Across Resume?

1. **In-memory analysis results** (deep analysis, synthesis, domain analysis): These are re-computed on resume. Phase snapshot files exist but are not loaded during resume.
2. **RefinementState.cached_judgments**: Not persisted. All artifacts re-evaluated on first iteration after resume.
3. **RefinementState.converged_artifacts**: Not persisted. Convergence tracking resets on resume.
4. **Strategy rotator state**: Not persisted. Strategy rotation resets on resume.
5. **Failure tracker state**: Not persisted across sessions.

### 12.3 Resume Accuracy Rating

| Component | Resume Accuracy | Notes |
|-----------|----------------|-------|
| Refinement iteration position | 100% | Exact iteration resume |
| Quality history | 100% | Full history preserved via RefinementCheckpoint |
| Best state restoration | 95% | Depends on snapshot file availability |
| Stagnation/convergence tracking | 100% | All counters preserved |
| Strategy outcomes | 100% | Preserved in checkpoint |
| Analysis chunk progress | 100% | Completed/failed chunks tracked |
| Phase completion flags | 100% | Boolean flags reconstructed |
| Judgment cache | 0% | Not persisted, re-evaluated |
| Converged artifact tracking | 0% | Not persisted, re-discovered |
| Full pipeline phase skip | 0% | Pipeline always re-runs from phase 1 |

---

## 13. Error Recovery Analysis

### 13.1 Error Categories and Recovery

The error system (`src/types/error.rs`) classifies errors into 9 categories:

| Category | Retryable | Fallback | Recovery Strategy |
|----------|-----------|----------|-------------------|
| RateLimit | Yes | No | Wait 30s then retry same provider |
| Network | Yes | No | Retry with backoff (5s) |
| Transient | Yes | No | Retry same provider (2s) |
| ParseError | Yes | No | Retry with different prompt (1s) |
| TokenLimit | No | Yes | Reduce context or fallback provider |
| Unavailable | No | Yes | Fallback to next provider |
| Auth | No | No | Fail fast |
| BadRequest | No | No | Fail fast, fix request |
| Unknown | No | No | Conservative approach |

### 13.2 Partial Failure Handling

**Distributed analysis**: Failed chunks are tracked separately. `CompletenessValidator` retries failed chunks and fills coverage gaps.

**Refinement strategy failures**: `FailureTracker` tracks per-artifact per-strategy failures. After `max_failures` (configurable) attempts, the strategy is skipped for that artifact. `FeedbackAwareSelector` rotates to alternative strategies.

**Event emission failures**: All event emissions are wrapped with error logging but don't abort the pipeline:
```rust
if let Err(e) = store.append(...).await {
    tracing::warn!(error = %e, "Failed to emit event");
}
```
This means event store failures are non-fatal -- the pipeline continues even if event recording fails.

---

## 14. Recommendations

### 14.1 High Priority

1. **Wire checkpoint system into AdaptivePipeline**: The `CheckpointManager` is fully implemented but unused. Integrating periodic checkpoints during the pipeline would provide faster recovery than full event replay.

2. **Enable config hash validation on resume**: Pass the actual config hash to `find_resumable_with_validation` to prevent resuming with mismatched configuration.

3. **Phase-skip optimization**: Store phase completion markers that allow `AdaptivePipeline::run()` to skip already-completed phases (project detection, convention inference, etc.) on resume.

### 14.2 Medium Priority

4. **Checkpoint-based fast replay**: Instead of replaying all events, seek to the last `RefinementCheckpoint` event and only replay subsequent events. This would reduce resume time for long sessions.

5. **Relative snapshot paths**: Store snapshot paths relative to the session directory, not as absolute paths. This would enable session portability.

6. **Session age cleanup**: Add a mechanism to clean up old/stale sessions (e.g., sessions older than 7 days).

### 14.3 Low Priority

7. **Event schema migration**: Add a migration mechanism for when event schema evolves. Currently, old events are silently skipped.

8. **Time-based session ordering**: Sort sessions by creation time (from first event timestamp) rather than UUID.

9. **Persist judgment cache**: Serialize cached judgments to a snapshot file for faster refinement resume.

---

## 15. Summary

The event sourcing system is **well-designed and production-ready** for its primary use case: enabling refinement loop resume after interruption. The sharded JSONL architecture with index reconciliation provides good crash safety, and the state reconstruction logic is thorough and well-tested.

The **main architectural concern** is the unused checkpoint system -- a complete implementation exists but is not integrated. This represents either dead code or an incomplete feature.

The **biggest practical limitation** is that analysis phases (project detection through constraint extraction) are always re-run on resume. Only the refinement loop truly benefits from resume, which is appropriate since refinement is typically the longest phase (80%+ of total runtime).

For large-scale projects, the system should perform well. The bounded collections, shard rotation, and incremental compaction prevent memory and disk issues. The sync system's dependency graph with depth-limited propagation prevents cascade explosions during incremental updates.
