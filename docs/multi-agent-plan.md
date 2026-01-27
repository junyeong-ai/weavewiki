# Multi-Agent Orchestration Plan (Domain-Based + Consensus)

## 1. Executive Summary

This document defines the long-term architecture for domain-based multi-agent orchestration across `claudegen` and `claude-pilot`.

**Core Principle**: claudegen generates all project intelligence as Claude Code native artifacts (skills, agents, rules). claude-pilot orchestrates execution using only these generated artifacts with zero hard-coded project knowledge.

---

## 2. Goals

- Enable large-project handling via module/domain/service-specialized agents
- Add evidence-based consensus planning with tiered routing
- Persist all decisions and evidence with event sourcing for auditability
- Keep system universal across ALL languages/frameworks/architectures
- Integrate natively with Claude Code (skills, agents, rules, hooks)

---

## 3. Scope

| Component | Responsibility |
|-----------|----------------|
| **claudegen** | Analyzes projects, generates skills/agents/rules/module_map |
| **claude-pilot** | Loads plugin, routes requests, runs consensus, executes tasks |

**Supported Architectures**:
- Single module
- Multi-module (Maven/Gradle style)
- Hierarchical modules (nested packages)
- Monorepo (multiple independent projects)
- Monorepo + hierarchical (Turborepo style)
- Polyglot (mixed languages)
- Microservices

---

## 4. Non-Goals

- No project-specific heuristics that depend on language/framework
- No hard-coded directory semantics beyond universal signals
- No custom runtime - only Claude Code native features
- No implicit context inheritance between agents

---

## 4.1 Claude Code Spec Constraints

This section documents official Claude Code limitations that affect our design.

### Skill Frontmatter (Supported Fields)

| Field | Supported | Notes |
|-------|-----------|-------|
| `name` | ✅ | Required |
| `description` | ✅ | Required |
| `user-invocable` | ✅ | `true` = user can invoke via `/skillname` |
| `disable-model-invocation` | ✅ | `true` = Claude cannot invoke automatically |
| `context` | ✅ | `fork` = runs in subagent |
| `agent` | ✅ | Specifies which agent runs this skill |
| `allowed-tools` | ✅ | Tool permissions for this skill |
| `model` | ✅ | Model override |
| `argument-hint` | ✅ | Hint text for skill arguments |
| `hooks` | ✅ | **Limited to: PreToolUse, PostToolUse, Stop** |
| `skills` | ❌ | **NOT SUPPORTED in skills** |

### Agent Frontmatter (Supported Fields)

| Field | Supported | Notes |
|-------|-----------|-------|
| `name` | ✅ | Required |
| `description` | ✅ | Required |
| `tools` | ✅ | Tool whitelist |
| `disallowedTools` | ✅ | Tool blacklist |
| `model` | ✅ | Model selection |
| `permissionMode` | ✅ | `default`, `acceptEdits`, `dontAsk`, `plan`, `bypassPermissions` |
| `skills` | ✅ | Skills to load into agent context |
| `hooks` | ✅ | **Limited to: PreToolUse, PostToolUse, Stop** |

### Hook Event Availability

| Event | Settings/Plugin | Skills | Agents |
|-------|-----------------|--------|--------|
| `SessionStart` | ✅ | ❌ | ❌ |
| `SessionEnd` | ✅ | ❌ | ❌ |
| `UserPromptSubmit` | ✅ | ❌ | ❌ |
| `PreToolUse` | ✅ | ✅ | ✅ |
| `PostToolUse` | ✅ | ✅ | ✅ |
| `Stop` | ✅ | ✅ | ✅ |
| `SubagentStart` | ✅ | ❌ | ❌ |
| `SubagentStop` | ✅ | ❌ | ❌ |
| `PreCompact` | ✅ | ❌ | ❌ |

### Permission Rule Syntax

```
Tool                    # Match any use of Tool
Tool(exact-match)       # Exact argument match
Tool(prefix:*)          # Prefix match with word boundary
Tool(prefix*)           # Prefix match without word boundary
Tool(* suffix)          # Suffix match
Tool(* middle *)        # Contains match
```

**Examples:**
- `Bash(npm run test:*)` - matches `npm run test`, `npm run test:unit`
- `Bash(cargo *)` - matches `cargo build`, `cargo test`
- `Read(~/.ssh/**)` - matches files in home ssh directory
- `Edit(/src/**/*.ts)` - matches TypeScript files relative to settings

### Environment Variables in Hooks

| Variable | Availability | Description |
|----------|--------------|-------------|
| `CLAUDE_PROJECT_DIR` | All hooks | Absolute path to project root |
| `CLAUDE_PLUGIN_ROOT` | Plugin hooks | Absolute path to plugin directory |
| `CLAUDE_ENV_FILE` | SessionStart, Setup | File for persisting env vars |
| `CLAUDE_CODE_REMOTE` | All hooks | `true` if running remotely |

---

## 5. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         CLAUDEGEN (Knowledge Generation)                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  Analysis Pipeline:                                                          │
│  ┌─────────────┐ → ┌─────────────┐ → ┌─────────────┐ → ┌─────────────┐     │
│  │  Structure  │   │   Module    │   │ Convention  │   │  Artifact   │     │
│  │  Analyzer   │   │  Boundary   │   │  Extractor  │   │  Generator  │     │
│  └─────────────┘   └─────────────┘   └─────────────┘   └─────────────┘     │
│                                                                              │
│  Outputs:                                                                    │
│  ├── .claude-plugin/plugin.json      (manifest)                             │
│  ├── .claudegen/module_map.json      (module graph + scores)                │
│  ├── skills/                          (orchestration + module skills)       │
│  │   ├── orchestrate/SKILL.md                                               │
│  │   ├── consensus-planning/SKILL.md                                        │
│  │   └── module-{id}/SKILL.md                                               │
│  ├── agents/                          (module leaders + cross-cutting)      │
│  │   ├── project-orchestrator.md                                            │
│  │   ├── module-{id}.md                                                     │
│  │   ├── architect.md                                                       │
│  │   ├── qa-{lang}.md                                                       │
│  │   └── security-reviewer.md                                               │
│  ├── rules/                           (path-scoped conventions)             │
│  │   ├── global/                                                            │
│  │   └── modules/{id}/                                                      │
│  └── CLAUDE.md                        (project memory)                      │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    │ Plugin Load
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         CLAUDE-PILOT (Execution)                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────┐     │
│  │                      Plugin Loader                                  │     │
│  │  Load: manifest → module_map → agents → skills → rules → memory    │     │
│  └────────────────────────────────────────────────────────────────────┘     │
│                              │                                               │
│                              ▼                                               │
│  ┌────────────────────────────────────────────────────────────────────┐     │
│  │                      Request Router                                 │     │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌──────────────────────┐  │     │
│  │  │ Trivial │  │ Simple  │  │ Complex │  │ Cross-Module         │  │     │
│  │  │(Direct) │  │(Module) │  │(Multi)  │  │ (Full Consensus)     │  │     │
│  │  └─────────┘  └─────────┘  └─────────┘  └──────────────────────┘  │     │
│  └────────────────────────────────────────────────────────────────────┘     │
│                              │                                               │
│                              ▼                                               │
│  ┌────────────────────────────────────────────────────────────────────┐     │
│  │                   Consensus Engine                                  │     │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                │     │
│  │  │  Proposal   │  │   Voting    │  │  Conflict   │                │     │
│  │  │  Collector  │  │ (Evidence)  │  │ Resolution  │                │     │
│  │  └─────────────┘  └─────────────┘  └─────────────┘                │     │
│  └────────────────────────────────────────────────────────────────────┘     │
│                              │                                               │
│                              ▼                                               │
│  ┌────────────────────────────────────────────────────────────────────┐     │
│  │                   Agent Execution (from plugin)                     │     │
│  │  Module Agents: module-auth, module-api, module-core, ...          │     │
│  │  Cross-Cutting: architect, qa-rust, qa-ts, security                │     │
│  └────────────────────────────────────────────────────────────────────┘     │
│                              │                                               │
│                              ▼                                               │
│  ┌────────────────────────────────────────────────────────────────────┐     │
│  │                   Event Store + Shared Memory                       │     │
│  │  SQLite events → Snapshots → Namespace cache                       │     │
│  └────────────────────────────────────────────────────────────────────┘     │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 6. Claude Code Native Integration

### 6.1 Skill Architecture

Skills are the primary execution units. Each skill is a directory with `SKILL.md`:

```
skills/
├── orchestrate/                    # Main entry point (user-invocable)
│   └── SKILL.md
├── consensus-planning/             # Consensus coordination (model-invocable)
│   └── SKILL.md
├── module-{id}/                    # Per-module skills
│   ├── SKILL.md
│   └── context.md                  # Module-specific context
├── qa-review-{lang}/               # Language-specific QA
│   └── SKILL.md
└── architecture-review/            # Architecture validation
    └── SKILL.md
```

**Skill Frontmatter Patterns**:

```yaml
# Entry skill - user-invocable, forks to orchestrator agent
# NOTE: Orchestrator has READ-ONLY + Task permissions (no direct file modification)
---
name: orchestrate
description: Execute complex tasks with multi-agent consensus. Use for any non-trivial task requiring planning or multi-module changes.
context: fork
agent: project-orchestrator
disable-model-invocation: true
user-invocable: true
allowed-tools: Read, Grep, Glob, Task
---

# Consensus skill - model-invocable, runs in subagent
---
name: consensus-planning
description: Coordinate consensus among module agents for cross-cutting changes.
context: fork
agent: project-orchestrator
user-invocable: false
allowed-tools: Read, Grep, Glob, Task
---

# Module skill - model-invocable, provides module context
# NOTE: No 'skills:' field in skills (not supported). Content included directly.
---
name: module-auth
description: Work on authentication module. Use when task involves user login, sessions, tokens, JWT, OAuth, or auth middleware.
---

## Module Scope
Paths: src/auth/, tests/auth/
Key files: src/auth/mod.rs, src/auth/jwt.rs, src/auth/session.rs

## Conventions
- All auth errors use AuthError enum
- Session tokens use JWT with RS256
- Rate limiting on all auth endpoints

# QA skill - runs in subagent with restricted tools
---
name: qa-review-rust
description: Review Rust code for quality, safety, and best practices
context: fork
agent: qa-rust
allowed-tools: Read, Grep, Glob, Bash(cargo check:*), Bash(cargo clippy:*), Bash(cargo test:*)
---
```

### 6.2 Agent Architecture

Agents define specialized roles with custom prompts and tool access:

```
agents/
├── project-orchestrator.md         # Main coordinator
├── module-{id}.md                  # Per-module experts
├── architect.md                    # Architecture guardian
├── qa-{lang}.md                    # Language QA specialists
└── security-reviewer.md            # Security specialist
```

**Agent Definition Pattern**:

```yaml
# NOTE: Agent hooks only support PreToolUse, PostToolUse, Stop events
# For SubagentStart/SubagentStop, use plugin.json or settings.json
---
name: module-auth
description: Expert on authentication module. Handles all auth-related tasks including login, sessions, JWT, OAuth.
tools: Read, Grep, Glob, Edit, Write, Bash
model: sonnet
permissionMode: acceptEdits
skills:
  - module-auth-context
hooks:
  PreToolUse:
    - matcher: "Edit|Write"
      hooks:
        - type: command
          command: "${CLAUDE_PROJECT_DIR}/scripts/validate-module-scope.sh auth"
  PostToolUse:
    - matcher: "Edit|Write"
      hooks:
        - type: command
          command: "${CLAUDE_PROJECT_DIR}/scripts/run-module-tests.sh auth"
---

You are the expert for the authentication module.

## Module Scope
Paths: src/auth/, tests/auth/
Key files: src/auth/mod.rs, src/auth/jwt.rs, src/auth/session.rs

## Dependencies
- Depends on: core, config
- Dependents: api, web

## Conventions
- All auth errors use AuthError enum
- Session tokens use JWT with RS256
- Rate limiting on all auth endpoints

## Known Issues
- Session refresh has race condition (tracked in #123)
```

### 6.3 Rules Architecture

Rules provide path-scoped constraints:

```
rules/
├── global/
│   ├── security.md                 # Security policies
│   └── error-handling.md           # Error conventions
├── {lang}/
│   ├── naming.md                   # Language naming conventions
│   └── testing.md                  # Language test patterns
└── modules/
    └── {id}.md                     # Module-specific rules
```

**Rule File Pattern**:

```yaml
---
paths:
  - "src/auth/**/*.rs"
  - "tests/auth/**/*.rs"
---

# Authentication Module Rules

## Error Handling
- Always use `AuthError` enum, never `anyhow::Error`
- Include error codes for client-facing errors

## Testing
- Test both success and failure paths
- Use `test_user()` fixture for authenticated tests
```

### 6.4 Plugin Manifest

```json
{
  "name": "my-project",
  "description": "Domain-specialized orchestration for my-project",
  "version": "1.0.0",
  "author": {
    "name": "claudegen",
    "version": "2.0.0"
  },
  "permissions": {
    "allowedTools": ["Read", "Grep", "Glob", "Edit", "Write", "Bash", "Task"]
  },
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Edit|Write",
        "hooks": [{ "type": "command", "command": "${CLAUDE_PLUGIN_ROOT}/scripts/pre-edit-hook.sh" }]
      }
    ],
    "SubagentStart": [
      {
        "matcher": "module-*",
        "hooks": [{
          "type": "command",
          "command": "${CLAUDE_PLUGIN_ROOT}/scripts/load-module-context.sh"
        }]
      }
    ],
    "SubagentStop": [
      {
        "matcher": "module-*",
        "hooks": [{
          "type": "prompt",
          "prompt": "Evaluate if this module agent completed its task. Input: $ARGUMENTS. Check if the assigned changes are complete and tests pass.",
          "timeout": 30
        }]
      }
    ]
  },
  "metadata": {
    "generatedAt": "2025-01-27T10:00:00Z",
    "projectType": "monorepo",
    "languages": ["rust", "typescript"],
    "moduleCount": 12
  }
}
```

---

## 7. Module Map Specification (claudegen output)

### 7.1 Location

- Output path: `plugin_root/.claudegen/module_map.json`
- Schema version: 1.0.0

### 7.2 JSON Schema

```json
{
  "version": "1.0.0",
  "project": {
    "name": "my-project",
    "type": "monorepo",
    "root": "/path/to/project"
  },
  "modules": [
    {
      "id": "auth",
      "name": "Authentication",
      "paths": ["src/auth/", "tests/auth/"],
      "key_files": [
        { "path": "src/auth/mod.rs", "role": "entry" },
        { "path": "src/auth/jwt.rs", "role": "core" }
      ],
      "public_apis": [
        { "name": "authenticate", "file": "src/auth/mod.rs", "line": 45 }
      ],
      "dependencies": {
        "internal": ["core", "config"],
        "external": ["jsonwebtoken", "bcrypt"]
      },
      "dependents": ["api", "web"],
      "languages": ["rust"],
      "scores": {
        "value": 0.85,
        "risk": 0.72,
        "coverage": 0.68,
        "complexity": 0.45
      },
      "conventions": [
        "Use AuthError for all errors",
        "JWT with RS256 signing"
      ],
      "recent_changes": [
        { "date": "2025-01-20", "summary": "Added OAuth2 support" }
      ],
      "known_issues": [
        { "id": "#123", "summary": "Session refresh race condition" }
      ]
    }
  ],
  "cross_cutting": {
    "architecture": {
      "pattern": "hexagonal",
      "layers": ["domain", "application", "infrastructure"]
    },
    "conventions": {
      "error_handling": "typed errors per module",
      "logging": "structured with tracing",
      "testing": "unit + integration + e2e"
    }
  },
  "dependency_graph": {
    "adjacency": {
      "auth": ["core", "config"],
      "api": ["auth", "core", "domain"]
    },
    "topological_order": ["core", "config", "domain", "auth", "api", "web"]
  }
}
```

### 7.3 Scoring Formulas

```
value_score = 0.30 * coverage_ratio
            + 0.30 * key_file_density
            + 0.20 * api_surface_ratio
            + 0.20 * dependent_count / max_dependents

risk_score  = 0.30 * dependency_fanout / max_fanout
            + 0.30 * change_frequency (if VCS available, else omit)
            + 0.20 * complexity_score
            + 0.20 * known_issue_count / max_issues
```

### 7.4 Module Boundary Detection

1. **Explicit modules**: Language-specific module systems (Rust mod, Go packages, Java packages)
2. **Directory boundaries**: Significant directory structure (min 3 files)
3. **Dependency clustering**: Files with high internal cohesion
4. **Fallback**: Top-level directories if synthesis is weak

---

## 8. Request Router (claude-pilot)

### 8.1 Tiered Routing Strategy

| Affected Modules | Confidence | Strategy | Participants |
|-----------------|------------|----------|--------------|
| 0-1 | ≥ 0.9 | Direct execution | None |
| 1 | < 0.9 | Module owner decides | module-leader |
| 2-3 | Any | Mini-consensus | module-leaders + architect |
| 4+ | Any | Full consensus | all affected + architect + QA |
| Architecture change | Any | Full + security | all + architect + security |

### 8.2 Impact Analysis

```rust
pub struct ImpactAnalysis {
    pub affected_modules: Vec<ModuleId>,
    pub affected_paths: Vec<PathBuf>,
    pub confidence: f64,
    pub has_api_changes: bool,
    pub has_security_implications: bool,
}

impl RequestRouter {
    fn analyze_impact(&self, request: &Request) -> ImpactAnalysis {
        // 1. Extract mentioned files/paths from request
        let mentioned_paths = extract_paths(request);

        // 2. Map paths to modules via module_map
        let affected_modules = self.module_map
            .modules_containing(&mentioned_paths);

        // 3. Calculate confidence based on explicitness
        let confidence = if mentioned_paths.is_empty() {
            0.5  // Need LLM to determine
        } else {
            0.9  // Explicit paths mentioned
        };

        ImpactAnalysis {
            affected_modules,
            affected_paths: mentioned_paths,
            confidence,
            has_api_changes: self.detect_api_impact(&affected_modules),
            has_security_implications: self.detect_security_impact(&affected_modules),
        }
    }
}
```

### 8.3 Task Type Routing

| Task Type | Required Agents |
|-----------|-----------------|
| Bug fix | orchestrator + module-leader + coder + qa-reviewer |
| Feature | orchestrator + architect + module-leader + coder + qa-reviewer |
| Refactor | orchestrator + architect + module-leader + reviewer |
| Performance | orchestrator + perf-reviewer + module-leader |
| Security | orchestrator + security-reviewer + module-leader |

---

## 9. Consensus Model (claude-pilot)

### 9.1 Consensus Flow

```
1. PROPOSAL PHASE
   └─ Orchestrator analyzes request
   └─ Identifies affected modules via module_map
   └─ Generates initial proposal with evidence

2. VOTING PHASE
   └─ Each relevant agent evaluates proposal
   └─ Returns: approve/reject + evidence + concerns + suggestions
   └─ Votes weighted by evidence quality

3. RESOLUTION PHASE
   └─ If weighted approval >= 0.67: ACCEPT
   └─ If weighted approval < 0.67:
      └─ Merge suggestions into revised proposal
      └─ Re-run voting (max 3 rounds)
   └─ If still rejected: ESCALATE to user

4. EXECUTION PHASE
   └─ Tasks assigned to module agents
   └─ Independent tasks run in parallel
   └─ Dependent tasks run sequentially
```

### 9.2 Vote Structure

```rust
pub struct ConsensusVote {
    pub agent_id: String,
    pub module_id: Option<String>,
    pub decision: VoteDecision,
    pub confidence: f64,
    pub evidence: Vec<EvidenceRef>,
    pub concerns: Vec<Concern>,
    pub suggested_changes: Vec<PlanChange>,
    pub reasoning: String,
}

pub enum VoteDecision {
    Approve,
    ApproveWithChanges,
    RequestMoreInfo,
    Reject,
}
```

### 9.3 Evidence-Weighted Voting

```rust
fn calculate_consensus(votes: &[ConsensusVote]) -> ConsensusResult {
    let mut weighted_approve = 0.0;
    let mut total_weight = 0.0;

    for vote in votes {
        // Weight = evidence quality * agent confidence
        let weight = vote.evidence_quality() * vote.confidence;
        total_weight += weight;

        match vote.decision {
            VoteDecision::Approve => weighted_approve += weight,
            VoteDecision::ApproveWithChanges => weighted_approve += weight * 0.8,
            VoteDecision::RequestMoreInfo => {} // No weight contribution
            VoteDecision::Reject => {} // No weight contribution
        }
    }

    let approval_ratio = weighted_approve / total_weight;

    if approval_ratio >= 0.67 {
        ConsensusResult::Accepted(merge_suggestions(votes))
    } else if has_blocking_concerns(votes) {
        ConsensusResult::Blocked(collect_blockers(votes))
    } else {
        ConsensusResult::NeedsRevision(collect_suggestions(votes))
    }
}
```

### 9.4 Consensus Configuration

```toml
[consensus]
max_rounds = 3
approval_threshold = 0.67
vote_timeout_secs = 60
max_participants = 8
evidence_threshold = 0.70
confidence_threshold = 0.65
```

### 9.5 Escalation Triggers

- Evidence score < threshold
- Consensus fails 3 rounds
- QA gate rejects plan
- Blocking concerns unresolved

**Action**: Escalate to user with full context + recommendations

---

## 10. Event Sourcing Model (claude-pilot)

### 10.1 Event Envelope

```rust
pub struct DomainEvent {
    pub id: Ulid,
    pub event_type: EventType,
    pub timestamp: DateTime<Utc>,

    // Correlation
    pub project_id: String,
    pub session_id: String,
    pub correlation_id: String,
    pub causation_id: Option<String>,

    // Aggregate
    pub aggregate_id: String,
    pub aggregate_type: AggregateType,
    pub aggregate_version: u32,

    // Actor
    pub actor: Actor,

    // Scope
    pub scope: EventScope,

    // Quality metadata
    pub evidence_refs: Vec<String>,
    pub confidence: f64,

    // Payload
    pub payload: serde_json::Value,
}
```

### 10.2 Aggregate Boundaries

| Aggregate | Events |
|-----------|--------|
| `mission:<id>` | RequestReceived, EvidenceCollected, MissionCompleted/Failed |
| `plan:<id>` | Consensus*, PlanGenerated, QAReview* |
| `module:<id>` | TaskAssigned/Started/Completed/Failed |

### 10.3 Core Event Types

```rust
pub enum EventType {
    // Request lifecycle
    RequestReceived,
    RequestAnalyzed,
    ModulesIdentified,

    // Consensus
    ConsensusStarted,
    ProposalGenerated,
    VoteReceived,
    ConsensusAccepted,
    ConsensusRejected,
    ConsensusEscalated,

    // Planning
    PlanGenerated,
    PlanValidated,
    TasksCreated,

    // Execution
    TaskAssigned,
    TaskStarted,
    TaskCompleted,
    TaskFailed,

    // Verification
    VerificationRoundStarted,
    IssueDetected,
    FixAttempted,
    VerificationPassed,
    ConvergenceAchieved,

    // Mission
    MissionCompleted,
    MissionFailed,
    MissionEscalated,
}
```

### 10.4 Snapshot Strategy

Snapshots created at:
- `ConsensusAccepted`
- `PlanGenerated`
- `TaskCompleted`
- `VerificationPassed`

```rust
pub struct Snapshot {
    pub id: Ulid,
    pub aggregate_id: String,
    pub version: u32,
    pub trigger: SnapshotTrigger,
    pub state: SnapshotState,
}

pub struct SnapshotState {
    pub active_plan: Option<Plan>,
    pub accepted_consensus: Option<ConsensusResult>,
    pub key_findings: Vec<Finding>,
    pub module_states: HashMap<String, ModuleState>,
}
```

### 10.5 Event Store Implementation

```rust
// SQLite-based event store
pub struct EventStore {
    db: SqlitePool,
}

impl EventStore {
    pub async fn append(&self, event: DomainEvent) -> Result<()> {
        // Optimistic locking via aggregate_version
        sqlx::query!(
            r#"INSERT INTO events
               (id, event_type, aggregate_id, aggregate_version, ...)
               VALUES (?, ?, ?, ?, ...)"#,
            event.id.to_string(),
            event.event_type.as_str(),
            event.aggregate_id,
            event.aggregate_version,
            // ...
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn replay(&self, aggregate_id: &str) -> Result<Vec<DomainEvent>> {
        // Replay all events for aggregate
    }
}
```

---

## 11. Shared Memory (claude-pilot)

### 11.1 Namespace Structure

```
project/{project_id}/
├── architecture/           # Global architecture decisions
├── conventions/            # Project-wide conventions
└── findings/               # Cross-cutting discoveries

module/{module_id}/
├── context/                # Module-specific context
├── issues/                 # Known issues
└── patterns/               # Learned patterns

consensus/{consensus_id}/
├── proposal/               # Current proposal
├── votes/                  # Collected votes
└── resolution/             # Final decision

session/{session_id}/
├── plan/                   # Current execution plan
├── progress/               # Task progress
└── blockers/               # Active blockers
```

### 11.2 Access Control

| Role | project/ | module/{id}/ | consensus/{id}/ | session/ |
|------|----------|--------------|-----------------|----------|
| Orchestrator | R/W | R/W | R/W | R/W |
| Module Leader | R | R/W (own) | R/W (participating) | R |
| QA Agent | R | R | R | R |
| Architect | R/W | R | R/W | R |

### 11.3 TTL Policy

| Namespace | TTL | Purpose |
|-----------|-----|---------|
| project/ | 30 days | Long-term decisions |
| module/ | 7 days | Module context |
| consensus/ | 24 hours | Active consensus |
| session/ | Session | Ephemeral state |

### 11.4 Source of Truth

- Event store is the system of record
- Shared memory is a cache for coordination
- Conflicts resolved in favor of event log

---

## 12. Plugin Load Contract (claude-pilot)

### 12.1 Required Layout

```
plugin_root/
├── .claude-plugin/
│   └── plugin.json              # REQUIRED: manifest
├── .claudegen/
│   └── module_map.json          # REQUIRED: module graph
├── skills/
│   └── orchestrate/
│       └── SKILL.md             # REQUIRED: entry skill
├── agents/
│   ├── project-orchestrator.md  # REQUIRED: main coordinator
│   └── qa-reviewer.md           # REQUIRED: QA agent
├── rules/                       # OPTIONAL: path rules
└── CLAUDE.md                    # OPTIONAL: project memory
```

### 12.2 Validation

```rust
impl PluginLoader {
    pub async fn load(&self) -> Result<LoadedPlugin> {
        // 1. Load and validate manifest
        let manifest = self.load_manifest()?;
        self.validate_schema_version(&manifest)?;

        // 2. Load module map (REQUIRED)
        let module_map = self.load_module_map()
            .context("module_map.json is required")?;

        // 3. Load required skills
        let orchestrate_skill = self.load_skill("orchestrate")
            .context("orchestrate skill is required")?;

        // 4. Load required agents
        let orchestrator_agent = self.load_agent("project-orchestrator")
            .context("project-orchestrator agent is required")?;
        let qa_agent = self.load_agent("qa-reviewer")
            .context("qa-reviewer agent is required")?;

        // 5. Load optional components
        let rules = self.load_rules().ok();
        let memory = self.load_claude_md().ok();

        Ok(LoadedPlugin { ... })
    }
}
```

### 12.3 Failure Handling

| Missing | Action |
|---------|--------|
| plugin.json | Fail fast |
| module_map.json | Fail fast |
| orchestrate skill | Fail fast |
| project-orchestrator agent | Fail fast |
| qa-reviewer agent | Fail fast |
| Other skills/agents | Continue with warning |
| rules/ | Continue |
| CLAUDE.md | Continue |

---

## 13. Quality Gates

### 13.1 Evidence Thresholds

| Tier | Criteria | Action |
|------|----------|--------|
| RED | evidence < 0.50 OR confidence < 0.40 | Stop + escalate |
| YELLOW | evidence < 0.70 OR confidence < 0.65 | Warn + continue |
| GREEN | evidence >= 0.70 AND confidence >= 0.65 | Proceed |

### 13.2 Convergence Rule

```
QA Convergence:
- Emit `VerificationRound(passed=true)` each clean review
- Emit `ConvergenceAchieved` after 2 consecutive clean rounds
- Emit `MissionCompleted` after final persistence
```

### 13.3 Quality Gate Events

| Gate | Pass Event | Fail Event |
|------|------------|------------|
| Evidence | - | `EscalationTriggered` |
| Consensus | `ConsensusAccepted` | `ConsensusRejected` |
| QA Round | `VerificationPassed` | `IssueDetected` |
| Convergence | `ConvergenceAchieved` | - |
| Mission | `MissionCompleted` | `MissionFailed` |

---

## 14. Implementation Phases

### Phase 1: Foundation (Weeks 1-4)

**claudegen:**
- [ ] Add module boundary detection phase
- [ ] Implement module_map.json generation
- [ ] Generate per-module agents
- [ ] Generate orchestrate skill
- [ ] Generate qa-reviewer agent

**claude-pilot:**
- [ ] Implement plugin loader
- [ ] Implement module map parser
- [ ] Implement request router
- [ ] Add tiered routing logic

**Deliverable:** Single-module tasks work with generated agents

### Phase 2: Consensus (Weeks 5-8)

**claudegen:**
- [ ] Generate cross-cutting agents (architect, security)
- [ ] Generate consensus-planning skill
- [ ] Add evidence references to agents
- [ ] Generate language-specific QA agents

**claude-pilot:**
- [ ] Implement consensus engine
- [ ] Add evidence-weighted voting
- [ ] Implement shared memory namespaces
- [ ] Add vote collection and resolution

**Deliverable:** Multi-module tasks use consensus planning

### Phase 3: Event Sourcing (Weeks 9-12)

**claude-pilot:**
- [ ] Implement event store (SQLite)
- [ ] Add all event types
- [ ] Implement snapshot strategy
- [ ] Add replay capability
- [ ] Implement optimistic locking

**Deliverable:** Full audit trail and state reconstruction

### Phase 4: Quality Convergence (Weeks 13-16)

**claude-pilot:**
- [ ] Integrate consensus with verification
- [ ] Add multi-turn QA loops
- [ ] Implement convergence tracking
- [ ] Add escalation handling

**claudegen:**
- [ ] Add hooks for pre/post verification
- [ ] Generate verification skills

**Deliverable:** 2-round clean convergence with consensus

### Phase 5: Learning & Optimization (Weeks 17-20)

**claude-pilot:**
- [ ] Track agent success rates
- [ ] Build pattern bank from events
- [ ] Implement adaptive routing
- [ ] Add decision reuse

**claudegen:**
- [ ] Use learned patterns for generation
- [ ] Improve module scoring based on history

**Deliverable:** System improves over time

---

## 15. Risk Mitigation

| Risk | Mitigation |
|------|------------|
| **Cost explosion** | Cap 8 agents per round, Haiku for voting, Sonnet for implementation, cache contexts |
| **Consensus deadlock** | Max 3 rounds, architect tiebreaker, user escalation |
| **Module boundary errors** | LLM-assisted detection, user confirmation, manual override |
| **Stale context** | 7-day TTL, event-driven updates, lazy loading |
| **Plugin incompatibility** | Strict schema versioning, fail-fast validation |

---

## 16. Configuration Reference

### claudegen config.toml

```toml
[module_detection]
min_files = 3
max_depth = 5

[module_scoring]
value_weight_coverage = 0.30
value_weight_key_files = 0.30
value_weight_api_surface = 0.20
value_weight_dependents = 0.20

[agent_generation]
module_agent_model = "sonnet"
qa_agent_model = "haiku"
orchestrator_model = "sonnet"

[output]
generate_module_map = true
generate_module_agents = true
generate_architect_agent = true
generate_qa_agents = true
generate_security_agent = true
```

### claude-pilot config.toml

```toml
[plugin]
search_paths = [".claude-plugin", "~/.claude/plugins"]
strict_version_check = true

[routing]
direct_confidence = 0.9
mini_consensus_max = 3
architect_on_api_changes = true

[consensus]
max_rounds = 3
approval_threshold = 0.67
vote_timeout_secs = 60
max_participants = 8
evidence_threshold = 0.70
confidence_threshold = 0.65

[event_store]
database_path = ".claude/events.db"
snapshot_interval = 100
retention_days = 30

[shared_memory]
project_ttl_days = 30
module_ttl_days = 7
consensus_ttl_hours = 24
```

---

## 17. Work Checklist

### A. claudegen (Knowledge Generation)

**Module Detection:**
- [ ] Add module boundary detection phase to pipeline
- [ ] Implement dependency graph builder
- [ ] Implement module scoring (value, risk, coverage)
- [ ] Generate module_map.json artifact

**Agent Generation:**
- [ ] Generate project-orchestrator.md
- [ ] Generate module-{id}.md for each module
- [ ] Generate architect.md
- [ ] Generate qa-{lang}.md for each language
- [ ] Generate security-reviewer.md

**Skill Generation:**
- [ ] Generate orchestrate/SKILL.md (entry point)
- [ ] Generate consensus-planning/SKILL.md
- [ ] Generate module-{id}/SKILL.md for each module
- [ ] Generate qa-review-{lang}/SKILL.md for each language

**Rules Generation:**
- [ ] Generate global rules (security, error-handling)
- [ ] Generate language-specific rules
- [ ] Generate module-specific rules with paths frontmatter

**Plugin Output:**
- [ ] Generate plugin.json manifest
- [ ] Include schema version for compatibility
- [ ] Add hooks configuration

### B. claude-pilot (Execution)

**Plugin Loading:**
- [ ] Implement plugin loader
- [ ] Implement module_map.json parser
- [ ] Validate required skills/agents
- [ ] Load rules and memory

**Request Routing:**
- [ ] Implement impact analysis
- [ ] Implement tiered routing (Direct/Module/Mini/Full)
- [ ] Implement task type detection

**Consensus Engine:**
- [ ] Implement proposal generation
- [ ] Implement vote collection
- [ ] Implement evidence-weighted voting
- [ ] Implement conflict resolution
- [ ] Implement escalation handling

**Event Store:**
- [ ] Implement SQLite-based event store
- [ ] Implement all event types
- [ ] Implement snapshot strategy
- [ ] Implement replay capability
- [ ] Implement optimistic locking

**Shared Memory:**
- [ ] Implement namespace structure
- [ ] Implement access control
- [ ] Implement TTL enforcement

**Quality Convergence:**
- [ ] Integrate consensus with verification
- [ ] Implement 2-round convergence
- [ ] Track convergence events

### C. Integration

- [ ] End-to-end test with single-module project
- [ ] End-to-end test with multi-module project
- [ ] End-to-end test with monorepo
- [ ] End-to-end test with polyglot project
- [ ] Performance benchmarks
- [ ] Documentation

---

*Document Version: 2.0.0*
*Last Updated: 2025-01-27*
