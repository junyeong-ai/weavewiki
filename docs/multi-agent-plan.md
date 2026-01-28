# Multi-Agent Orchestration Plan (Domain-Based + Consensus)

**Version**: 3.10.0
**Status**: Final
**Last Updated**: 2025-01-27

## 1. Executive Summary

This document defines the **definitive long-term architecture** for domain-based multi-agent orchestration across `claudegen` and `claude-pilot`.

**Core Principle**: claudegen generates all project intelligence as Claude Code native artifacts (skills, agents, rules). claude-pilot orchestrates execution using only these generated artifacts with zero hard-coded project knowledge.

**Design Innovations**:
1. **Module-Specialized Agents**: Dynamic generation of project-specific experts from codebase analysis
2. **Evidence-Weighted Consensus**: Multi-agent agreement with quality-weighted voting
3. **Tiered Routing**: Complexity-appropriate agent allocation (Direct → Module → Mini → Full)
4. **Convergent Verification**: 2 consecutive clean rounds (NON-NEGOTIABLE)
5. **Event Sourcing**: Full audit trail and state reconstruction

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

- No **hard-coded** project-specific heuristics in claude-pilot (all project intelligence from generated artifacts)
- No hard-coded directory semantics beyond universal signals (`.git`, `node_modules`)
- No custom runtime for core orchestration - only Claude Code native features (skills, agents, rules, hooks)
- No implicit context inheritance between agents (explicit skill injection required)

**Clarifications:**
- **Language-aware analysis**: claudegen may use language-specific parsing (Tree-Sitter) for analysis, but generated artifacts are language-agnostic
- **Hooks with scripts**: Plugin hooks executing shell scripts ARE Claude Code native features (see [hooks reference](https://code.claude.com/docs/en/hooks)), not custom runtime
- **Module detection**: Uses LLM judgment for semantic boundaries, with programmatic fallbacks for universal signals only

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

## 4.2 claude-pilot Implementation Model

**Critical Question**: What IS claude-pilot and how does it work?

**Answer**: claude-pilot is a **CLI wrapper** that orchestrates Claude Code sessions.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     CLAUDE-PILOT IMPLEMENTATION MODEL                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  claude-pilot IS:                                                            │
│  ├── A Rust CLI binary that wraps `claude` (Claude Code CLI)                │
│  ├── Manages state OUTSIDE of Claude Code (SQLite, SharedMemory)            │
│  ├── Configures hooks in settings.json before launching Claude Code         │
│  └── Parses transcript AFTER Claude Code session ends                       │
│                                                                              │
│  claude-pilot IS NOT:                                                        │
│  ├── A daemon running alongside Claude Code                                  │
│  ├── Something that intercepts tool calls in real-time                      │
│  └── Magic runtime that can modify LLM behavior mid-conversation            │
│                                                                              │
│  EXECUTION FLOW:                                                             │
│  ┌────────────────┐     ┌────────────────┐     ┌────────────────┐          │
│  │  claude-pilot  │────▶│  claude        │────▶│  claude-pilot  │          │
│  │  (pre-session) │     │  (Claude Code) │     │  (post-session)│          │
│  │                │     │                │     │                │          │
│  │  - Load plugin │     │  - Run agents  │     │  - Parse output│          │
│  │  - Setup hooks │     │  - Execute     │     │  - Store events│          │
│  │  - Prep context│     │  - Hooks fire  │     │  - Update state│          │
│  └────────────────┘     └────────────────┘     └────────────────┘          │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Key Insight**: claude-pilot operates in **two phases**:

| Phase | Timing | What claude-pilot Does |
|-------|--------|------------------------|
| **Pre-Session** | Before `claude` runs | Load plugin, configure hooks, write initial context |
| **Post-Session** | After `claude` exits | Parse transcript, extract `<task-result>` tags, store events, update SharedMemory |

**Hooks are the Bridge**: Claude Code hooks (SubagentStop, Stop, etc.) can write to files. claude-pilot reads these files post-session OR in real-time via file watching.

```
┌────────────────────────────────────────────────────────────────────────────┐
│  REAL-TIME vs POST-SESSION STATE CAPTURE                                    │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Option A: Post-Session (Simpler)                                          │
│  ├── Hooks write to .claudegen/events/*.jsonl during session               │
│  ├── claude-pilot reads all event files after session ends                 │
│  └── Batch processing of events into SQLite                                │
│                                                                             │
│  Option B: Real-Time (More Complex)                                        │
│  ├── claude-pilot watches .claudegen/events/ directory                     │
│  ├── Processes events as they're written by hooks                          │
│  └── Enables live dashboards, early intervention                           │
│                                                                             │
│  RECOMMENDED: Start with Option A, evolve to Option B                      │
│                                                                             │
└────────────────────────────────────────────────────────────────────────────┘
```

**What This Means for the Architecture**:

1. **"claude-pilot intercepts outputs"** = claude-pilot parses transcript/hook files AFTER session
2. **"claude-pilot routes requests"** = claude-pilot configures which skill to invoke BEFORE session
3. **SharedMemory** = Files that both hooks and claude-pilot can read/write
4. **Event Store** = SQLite database managed by claude-pilot, hooks append via JSONL files

**Code Clarification**: Rust code in this document is for claude-pilot CLI, not for execution inside LLM agents. Agents are LLMs that output text; claude-pilot processes that text.

**Pre-Session Context Injection**: claude-pilot launches Claude Code with initial context:
- Builds initial prompt with session state (pending consensus, active plan, etc.)
- Passes via `claude --prompt "..."` or equivalent CLI mechanism
- SessionStart hook can set environment variables for hooks to use

**Multi-Session Resume**: Complex tasks may span multiple Claude Code sessions:
- Post-session: claude-pilot persists all state to SQLite
- Pre-session: claude-pilot detects pending work (unfinished consensus, incomplete tasks)
- Injects resume context into initial prompt: "Resuming consensus {id}, round {n}, state: ..."
- Session continuity tracked via `correlation_id` in events

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
│  │   ├── claude-pilot/SKILL.md                                               │
│  │   ├── consensus-planning/SKILL.md                                        │
│  │   └── module-{id}/SKILL.md                                               │
│  ├── agents/                          (module leaders + cross-cutting)      │
│  │   ├── architect.md                                            │
│  │   ├── module-{id}.md                                                     │
│  │   ├── architect.md                                                       │
│  │   ├── qa-reviewer.md                 (unified, language-agnostic)        │
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
│  │  Cross-Cutting: architect, qa-reviewer, security                   │     │
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

### 5.1 High-Level Flow

```
User Request
     │
     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                            CLAUDE-PILOT FLOW                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  1. LOAD PLUGIN                                                             │
│     ├── Load .claude-plugin/plugin.json                                     │
│     ├── Load .claudegen/module_map.json                                     │
│     ├── Load skills/, agents/, rules/                                       │
│     └── Validate required components                                        │
│                                                                              │
│  2. ANALYZE REQUEST                                                         │
│     ├── Extract mentioned paths/files                                       │
│     ├── Map paths to modules via module_map                                 │
│     ├── Detect task type (bug/feature/refactor/security)                    │
│     └── Calculate confidence score                                          │
│                                                                              │
│  3. ROUTE TO TIER                                                           │
│     ┌─────────────────────────────────────────────────────────────────┐    │
│     │  Modules    │ Confidence │ Tier      │ Participants             │    │
│     ├─────────────┼────────────┼───────────┼──────────────────────────┤    │
│     │  0-1        │ ≥ 0.9      │ DIRECT    │ None (direct exec)       │    │
│     │  1          │ < 0.9      │ MODULE    │ module-{id} only       │    │
│     │  2-3        │ Any        │ MINI      │ leaders + architect      │    │
│     │  4+         │ Any        │ FULL      │ all + architect + QA     │    │
│     │  Security   │ Any        │ FULL+SEC  │ all + security           │    │
│     └─────────────────────────────────────────────────────────────────┘    │
│                                                                              │
│  4. EXECUTE CONSENSUS (if not DIRECT)                                       │
│     ├── Generate proposal with evidence                                     │
│     ├── Collect votes from relevant agents                                  │
│     ├── Weight votes by evidence quality                                    │
│     ├── Resolve conflicts (architect defers on modules)                     │
│     └── Accept (≥67%), Revise, or Escalate                                 │
│                                                                              │
│  5. EXECUTE TASKS (orchestrator delegates via Task tool)                    │
│     ├── Decompose plan into module-scoped tasks                            │
│     ├── Spawn module agents via Task tool                                   │
│     ├── Execute parallel (independent) / sequential (dependent)             │
│     └── Emit events to event store                                          │
│                                                                              │
│  6. VERIFY CONVERGENT                                                       │
│     ├── Run verification round                                              │
│     ├── Extract issues (LLM-validated)                                      │
│     ├── Apply fix strategies (pattern bank)                                 │
│     └── Repeat until 2 consecutive clean rounds                             │
│                                                                              │
│  7. COMPLETE                                                                │
│     ├── Emit MissionCompleted event                                         │
│     ├── Extract learnings                                                   │
│     └── Update pattern bank                                                 │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 6. Claude Code Native Integration

### 6.1 Skill Architecture

Skills are the primary execution units. Each skill is a directory with `SKILL.md`:

```
skills/
├── claude-pilot/                   # Main entry point (user-invocable)
│   └── SKILL.md
├── consensus-planning/             # Consensus coordination (model-invocable)
│   └── SKILL.md
├── module-{id}/                    # Per-module skills
│   ├── SKILL.md
│   └── context.md                  # Module-specific context
├── qa-review/                      # Unified QA (language-agnostic logic)
│   └── SKILL.md
├── qa-static-analysis/             # Language-specific static analysis
│   └── SKILL.md                    # Dispatches to appropriate tools
├── qa-test-runner/                 # Language-specific test execution
│   └── SKILL.md
└── architecture-review/            # Architecture validation
    └── SKILL.md
```

**QA Structure Rationale**:

Instead of generating N agents for N languages (cost explosion), use hierarchical QA:

```
┌─────────────────────────────────────────────────────────────────┐
│  qa-reviewer.md (agent)                                         │
│  ├── 90% logic is language-agnostic:                           │
│  │   - Code complexity analysis                                 │
│  │   - Test coverage assessment                                 │
│  │   - Documentation completeness                               │
│  │   - Security vulnerability patterns                          │
│  │                                                              │
│  └── Dispatches to skills for language-specific tools:          │
│      ├── qa-static-analysis/ → Bash(cargo clippy), Bash(pylint) │
│      └── qa-test-runner/     → Bash(cargo test), Bash(pytest)   │
└─────────────────────────────────────────────────────────────────┘
```

This reduces agent count from O(languages) to O(1) while maintaining language-specific tooling.

**Skill Frontmatter Patterns**:

```yaml
# Entry skill - user-invocable, forks to orchestrator agent
# CRITICAL: Orchestrator has READ-ONLY permissions + Task tool for delegation
# Orchestrator analyzes and delegates; module agents do the actual file modifications
---
name: claude-pilot
description: Execute complex tasks with multi-agent consensus. Use for any non-trivial task requiring planning or multi-module changes.
context: fork
agent: architect
disable-model-invocation: false
user-invocable: true
allowed-tools: Read, Grep, Glob, Task
---

# Alias skill - user-invocable shortcut
---
name: claude-pilot
description: Alias for /claude-pilot. Launch multi-agent consensus-based execution.
context: fork
agent: architect
disable-model-invocation: true
user-invocable: true
allowed-tools: Read, Grep, Glob, Task
---

# Consensus skill - model-invocable, runs in subagent
---
name: consensus-planning
description: Coordinate consensus among module agents for cross-cutting changes.
context: fork
agent: architect
user-invocable: false
allowed-tools: Read, Grep, Glob, Task
---

# Module skill - model-invocable, provides module context
# CRITICAL: No 'skills:' field in skills (NOT SUPPORTED)
# Module context is included directly in skill body
---
name: module-auth
description: Work on authentication module. Use when task involves user login, sessions, tokens, JWT, OAuth, or auth middleware.
---

## Module Scope
Paths: src/auth/, tests/auth/
Key files: src/auth/mod.rs, src/auth/jwt.rs, src/auth/session.rs

## Conventions
- All auth errors use AuthError enum @src/auth/error.rs:1
- Session tokens use JWT with RS256 @src/auth/jwt.rs:23
- Rate limiting on all auth endpoints @src/auth/middleware.rs:45

## Known Issues
- Session refresh has race condition @src/auth/session.rs:89 (tracked: #123)

# QA skill - unified reviewer with language-aware tool dispatch
---
name: qa-review
description: Review code for quality, safety, and best practices. Works with any language.
context: fork
agent: qa-reviewer
allowed-tools: Read, Grep, Glob, Task
---

# QA static analysis - language-specific tool dispatch
---
name: qa-static-analysis
description: Run static analysis tools for the detected language(s).
allowed-tools: Read, Grep, Glob, Bash(cargo clippy:*), Bash(cargo check:*), Bash(pylint:*), Bash(eslint:*), Bash(golangci-lint:*)
---

## Supported Languages and Tools

| Language | Static Analysis | Test Runner |
|----------|-----------------|-------------|
| Rust | `cargo clippy`, `cargo check` | `cargo test` |
| Python | `pylint`, `mypy` | `pytest` |
| TypeScript | `eslint`, `tsc --noEmit` | `jest`, `vitest` |
| Go | `golangci-lint` | `go test` |
| Java | `checkstyle`, `spotbugs` | `mvn test`, `gradle test` |

$ARGUMENTS
```

**IMPORTANT: Orchestrator is READ-ONLY**

The architect agent uses the Task tool to spawn module agents that have Edit/Write permissions. This separation ensures:
1. Orchestrator cannot accidentally modify files outside module scope
2. Module agents have full authority within their scope
3. Clear audit trail of which agent made which changes

**CRITICAL: Event Storage is claude-pilot's Responsibility**

The orchestrator agent does NOT write to Event Store or Shared Memory directly. Instead:
1. Orchestrator makes decisions and outputs structured JSON
2. **claude-pilot** parses agent outputs from transcript **post-session**
3. claude-pilot persists events to SQLite and updates Shared Memory
4. This resolves the "READ-ONLY agent needs to write" paradox

```
┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐
│   Orchestrator   │────▶│   claude-pilot   │────▶│   Event Store    │
│   (READ-ONLY)    │     │   (Runtime)      │     │   (SQLite)       │
│   outputs JSON   │     │   parses & saves │     │                  │
└──────────────────┘     └──────────────────┘     └──────────────────┘
```

### 6.2 Agent Architecture

Agents define specialized roles with custom prompts and tool access:

```
agents/
├── architect.md         # Main coordinator
├── module-{id}.md                  # Per-module experts
├── architect.md                    # Architecture guardian
├── qa-reviewer.md                  # Unified QA (language-agnostic)
└── security-reviewer.md            # Security specialist
```

**Agent Definition Patterns**:

```yaml
# Project Orchestrator - READ-ONLY + Task delegation
# CRITICAL: Orchestrator does NOT have Edit/Write tools
# It analyzes requests and delegates to module agents via Task tool
---
name: architect
description: Main coordinator for all complex tasks. Manages consensus, delegates to module agents, ensures convergent quality.
tools: Read, Grep, Glob, Task
model: sonnet
permissionMode: default
skills:
  - consensus-planning
---

You are the project orchestrator.

## Your Role
1. Analyze incoming requests for scope and impact
2. Identify which modules are affected (via module_map.json)
3. For multi-module changes, run consensus protocol
4. Decompose plans into module-scoped tasks
5. Delegate tasks to module agents via Task tool
6. Monitor execution and ensure 2-round convergent verification

## Decision Authority
- Single module changes: delegate to module leader
- Multi-module changes: run consensus protocol
- Architecture changes: require architect approval
- Security implications: require security review

## Module Map
@.claudegen/module_map.json

# Module Agent - Full permissions within scope
# NOTE: Agent hooks only support PreToolUse, PostToolUse, Stop events
# For SubagentStart/SubagentStop, use plugin.json or settings.json
---
name: module-auth
description: Expert on authentication module. Handles all auth-related tasks including login, sessions, JWT, OAuth.
tools: Read, Grep, Glob, Edit, Write, Bash
model: sonnet
permissionMode: acceptEdits
skills:
  - module-auth
hooks:
  PreToolUse:
    - matcher: "Edit|Write"
      hooks:
        - type: command
          command: "${CLAUDE_PROJECT_DIR}/.claudegen/hooks/validate-module-scope.sh auth"
  PostToolUse:
    - matcher: "Edit|Write"
      hooks:
        - type: command
          command: "${CLAUDE_PROJECT_DIR}/.claudegen/hooks/run-module-tests.sh auth"
---

You are the expert for the authentication module.

## Module Scope
Paths: src/auth/, tests/auth/
Key files: src/auth/mod.rs, src/auth/jwt.rs, src/auth/session.rs

## Dependencies
- Depends on: core, config
- Dependents: api, web

## Conventions
- All auth errors use AuthError enum @src/auth/error.rs:1
- Session tokens use JWT with RS256 @src/auth/jwt.rs:23
- Rate limiting on all auth endpoints @src/auth/middleware.rs:45

## Known Issues
- Session refresh has race condition @src/auth/session.rs:89 (tracked: #123)

## Your Authority
- Full authority over files in your module scope
- Must coordinate with other modules for API changes
- Must get architect approval for pattern changes
```

### 6.3 Context Propagation Protocol

**Problem**: Sub-agents spawned via Task tool do NOT inherit parent context.

**CRITICAL CONSTRAINTS**:
1. Claude Code does NOT allow intercepting Task tool calls before execution
2. **Orchestrator is READ-ONLY** (no Edit/Write tools) - cannot write context files
3. SubagentStart/Stop hooks fire AFTER subagent has started

**Solution**: Prompt-embedded context (no file I/O required).

**Key Insight**: The Task tool's `prompt` parameter IS the context delivery mechanism. Orchestrator embeds context directly in the Task prompt string - no file writes needed.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     CONTEXT PROPAGATION FLOW                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  1. Orchestrator embeds context DIRECTLY in Task prompt (NO file write)     │
│     ┌─────────────────────────────────────────────────────────────────┐    │
│     │  TASK PROMPT STRUCTURE:                                          │    │
│     │  <task-context task-id="{ulid}">                                 │    │
│     │  {                                                               │    │
│     │    "task_id": "{ulid}",                                         │    │
│     │    "current_plan": {...},                                       │    │
│     │    "module_subset": {...},                                      │    │
│     │    "dependencies": [...],                                       │    │
│     │    "completed_tasks": [...],                                    │    │
│     │    "blocking_issues": [...]                                     │    │
│     │  }                                                               │    │
│     │  </task-context>                                                 │    │
│     │  {task_description}                                              │    │
│     └─────────────────────────────────────────────────────────────────┘    │
│                                                                              │
│  2. Task tool spawns sub-agent with embedded context in prompt              │
│                                                                              │
│  3. SubagentStart hook fires (optional: write to .claudegen/events/)        │
│                                                                              │
│  4. Sub-agent parses <task-context> from its prompt - NO file read needed   │
│                                                                              │
│  5. Sub-agent outputs structured result:                                    │
│     <task-result task-id="{ulid}">                                         │
│     {"status": "completed|failed", "files_modified": [...], ...}           │
│     </task-result>                                                          │
│                                                                              │
│  6. SubagentStop hook writes event to .claudegen/events/{task_id}.jsonl    │
│                                                                              │
│  7. Result capture (TWO PATHS):                                             │
│     ┌─────────────────────────────────────────────────────────────────┐    │
│     │  Path A: Orchestrator (LLM) reads <task-result> from Task output│    │
│     │          and incorporates result into ongoing execution          │    │
│     │          (immediate, within same session)                        │    │
│     │                                                                  │    │
│     │  Path B: claude-pilot parses transcript post-session            │    │
│     │          (batch processing after Claude Code exits)              │    │
│     │          - Reads .claudegen/events/*.jsonl                      │    │
│     │          - Extracts <task-result> from conversation transcript  │    │
│     │          - Stores in SQLite event store                         │    │
│     └─────────────────────────────────────────────────────────────────┘    │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Why Prompt-Embedded Context**:

| Approach | Feasibility | Reason |
|----------|-------------|--------|
| File-based (orchestrator writes) | ❌ VIOLATES READ-ONLY | Orchestrator has no Write tool |
| Hook on `Task` tool | ❌ IMPOSSIBLE | No hook can modify Task parameters |
| SubagentStart writes file | ❌ TOO LATE | Subagent already started, can't modify its initial prompt |
| **Prompt-embedded context** | ✅ WORKS | Context included in Task `prompt` parameter |

**Implementation**:

**IMPORTANT**: The Rust code below runs in **claude-pilot CLI** (pre/post session), NOT inside LLM agents.
- `TaskPromptBuilder`: Conceptual - orchestrator LLM generates this text naturally
- `ResultCapture`: Runs in claude-pilot post-session to process transcript

```rust
/// CONCEPTUAL: This represents what the orchestrator LLM does when building Task prompts.
/// The orchestrator (an LLM) generates this text structure naturally, not by executing Rust.
/// This code documents the EXPECTED FORMAT, not runtime execution.
impl TaskPromptBuilder {
    pub fn build_task_prompt(
        &self,
        task_id: &Ulid,
        agent_id: &str,
        task_description: &str,
    ) -> String {
        let context = ContextPayload {
            task_id: task_id.to_string(),
            current_plan: self.plan.clone(),
            module_subset: self.module_map.subset_for(agent_id),
            dependencies: self.dependency_graph.deps_for(agent_id),
            completed_tasks: self.completed_results.clone(),
            blocking_issues: self.known_blockers.clone(),
        };

        format!(r#"
<task-context task-id="{task_id}">
{context_json}
</task-context>

{task_description}

When complete, output your result:
<task-result task-id="{task_id}">
{{"status": "completed|failed|blocked", "files_modified": [...], "summary": "..."}}
</task-result>
"#,
            task_id = task_id,
            context_json = serde_json::to_string_pretty(&context).unwrap(),
            task_description = task_description,
        )
    }
}

/// ACTUAL RUNTIME CODE: Runs in claude-pilot CLI post-session.
/// claude-pilot reads the conversation transcript and extracts <task-result> tags.
impl ResultCapture {
    /// Called by claude-pilot after Claude Code session ends.
    /// Input: Full conversation transcript (from Claude Code's output or .claude/transcript)
    pub fn extract_task_result(&self, agent_output: &str) -> Option<TaskResult> {
        let re = Regex::new(r"<task-result task-id=\"([^\"]+)\">\s*(\{[\s\S]*?\})\s*</task-result>").unwrap();
        re.captures(agent_output).and_then(|caps| {
            serde_json::from_str(caps.get(2)?.as_str()).ok()
        })
    }
}
```

**Settings.json Hook Configuration** (event capture):

```json
{
  "hooks": {
    "SubagentStart": [
      {
        "type": "command",
        "command": "echo '{\"event\":\"subagent_start\",\"ts\":\"'$(date -Iseconds)'\"}' >> ${CLAUDE_PROJECT_DIR}/.claudegen/events/session.jsonl"
      }
    ],
    "SubagentStop": [
      {
        "type": "command",
        "command": "echo '{\"event\":\"subagent_stop\",\"ts\":\"'$(date -Iseconds)'\"}' >> ${CLAUDE_PROJECT_DIR}/.claudegen/events/session.jsonl"
      }
    ]
  }
}
```

**Note**: Hooks capture timing events. The actual `<task-result>` content is extracted by claude-pilot from the conversation transcript post-session.

**Parallel Execution**: Each Task has unique task_id embedded in prompt - no file contention.

**Context Size Limits**:

| Context Type | Limit Strategy |
|--------------|----------------|
| current_plan | Latest 10 tasks only |
| module_subset | Essential fields only (id, paths, dependencies) |
| completed_tasks | Most recent 5 results |
| consensus_state | Current round only |

**Enforcement**: Context size limits are enforced by **orchestrator heuristics**, not strict byte counting:
- Orchestrator prompt instructs: "Include only the 10 most recent tasks"
- LLM naturally truncates based on these instructions
- NOT programmatic validation (LLM cannot measure bytes accurately)
- If context is too large, Claude Code may hit token limits → graceful failure with retry

**Note**: Specific byte/KB limits are NOT enforced - these are rough guidelines. LLM judges "enough context" semantically.

**Agent Prompt Template** (generated by orchestrator, included in Task prompt):

```markdown
## Task Execution Protocol

Your task context is embedded above in <task-context>.
1. Parse the JSON context to understand your assigned work
2. Execute the task following the plan
3. Output your result in <task-result> format when complete
```

### 6.4 Rules Architecture

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

### 6.5 Plugin Manifest

**Schema-compliant example** (per `docs/schemas/plugin.json.schema.json`):

```json
{
  "schema_version": "1.0.0",
  "generator": "claudegen@2.0.0",
  "project_name": "my-project",
  "description": "Domain-specialized orchestration for my-project",
  "required_skills": ["claude-pilot", "consensus-planning"],
  "required_agents": ["architect", "qa-reviewer"],
  "skills": ["claude-pilot", "consensus-planning", "module-auth", "module-api", "qa-review"],
  "agents": ["architect", "qa-reviewer", "module-auth", "module-api", "architect"]
}
```

**Schema Required Fields:**
| Field | Type | Description |
|-------|------|-------------|
| `schema_version` | string | Semver for manifest compatibility |
| `generator` | string | claudegen version (format: `name@version`) |
| `project_name` | string | Project identifier |

**Schema Optional Fields:**
| Field | Type | Description |
|-------|------|-------------|
| `description` | string | Project description |
| `required_skills` | string[] | Skills required for orchestration |
| `required_agents` | string[] | Agents required for orchestration |
| `skills` | string[] | List of all skill names (for validation) |
| `agents` | string[] | List of all agent names (for validation) |

**Note:** Plugin hooks (SubagentStart/Stop, etc.) should be configured in `.claude/settings.json` rather than plugin.json, as they are Claude Code runtime features not part of the claudegen schema.

---

## 7. Module Map Specification (claudegen output)

### 7.1 Location

- Output path: `plugin_root/.claudegen/module_map.json`
- Schema version: 1.0.0

### 7.2 JSON Schema

**Schema-compliant example** (per `docs/schemas/module_map.json.schema.json`):

```json
{
  "module_map_version": "1.0.0",
  "modules": [
    {
      "module_id": "auth",
      "paths": ["src/auth/", "tests/auth/"],
      "coverage_ratio": 0.68,
      "key_files": [
        "src/auth/mod.rs",
        "src/auth/jwt.rs",
        "src/auth/session.rs"
      ],
      "dependencies": ["core", "config"],
      "estimated_value_score": 0.85,
      "risk_score": 0.72
    },
    {
      "module_id": "api",
      "paths": ["src/api/", "tests/api/"],
      "coverage_ratio": 0.45,
      "key_files": [
        "src/api/routes.rs",
        "src/api/handlers.rs"
      ],
      "dependencies": ["auth", "core", "domain"],
      "estimated_value_score": 0.75,
      "risk_score": 0.60
    }
  ]
}
```

**Schema Required Fields (Top-level):**
| Field | Type | Description |
|-------|------|-------------|
| `module_map_version` | string | Semver for module map compatibility |
| `modules` | array | Array of module definitions |

**Schema Required Fields (Per Module):**
| Field | Type | Description |
|-------|------|-------------|
| `module_id` | string | Stable ID for event correlation |
| `paths` | string[] | File paths belonging to module |
| `coverage_ratio` | number (0-1) | Test coverage ratio |
| `key_files` | string[] | Important files in module |
| `dependencies` | string[] | Other module IDs this depends on |

**Schema Optional Fields (Per Module):**
| Field | Type | Description |
|-------|------|-------------|
| `estimated_value_score` | number (0-1) | Business value score |
| `risk_score` | number (0-1) | Risk/complexity score |

**Note:** The schema is intentionally minimal. Extended metadata (conventions, known_issues, public_apis, etc.) should be embedded in the skill/agent markdown files rather than the module_map.json.

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

Module detection uses **LLM judgment** for semantic boundaries with **programmatic fallbacks** for universal signals only.

**Detection Strategy with Confidence Thresholds:**

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     MODULE DETECTION DECISION TREE                           │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  LLM Semantic Analysis                                                       │
│       │                                                                      │
│       ├── confidence ≥ 0.7 ──────────────▶ USE LLM RESULT                   │
│       │                                                                      │
│       ├── 0.5 ≤ confidence < 0.7 ────────▶ HYBRID APPROACH                  │
│       │   │                                │ (LLM + Structural signals)     │
│       │   │                                │ Merge overlapping modules      │
│       │   │                                │ User confirmation recommended   │
│       │   │                                                                  │
│       └── confidence < 0.5 ──────────────▶ STRUCTURAL FALLBACK              │
│           │                                                                  │
│           ├── Has clear structural signals ──▶ Use structural boundaries   │
│           │   (min 3 files, distinct deps)                                  │
│           │                                                                  │
│           └── No clear signals ──────────────▶ TOP-LEVEL DIRECTORIES       │
│               │                                + User confirmation REQUIRED │
│               │                                                              │
│               └── User overrides available via .claudegen/modules.toml      │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Confidence Thresholds** (configurable):

| Threshold | Default | Description |
|-----------|---------|-------------|
| `llm_high_confidence` | 0.7 | Above this: trust LLM fully |
| `llm_low_confidence` | 0.5 | Below this: use fallback |
| `structural_min_files` | 3 | Minimum files for structural module |
| `structural_min_distinct_deps` | 2 | Minimum unique dependencies |

**Configuration**:

```toml
# .claudegen/config.toml
[module_detection]
llm_high_confidence = 0.7
llm_low_confidence = 0.5
structural_min_files = 3
require_user_confirmation_below = 0.5
```

**Manual Override**:

```toml
# .claudegen/modules.toml (user-defined)
[[modules]]
id = "auth"
paths = ["src/auth/", "src/security/"]
reason = "Combined for business domain coherence"

[[modules]]
id = "legacy"
paths = ["src/old/"]
skip_agent_generation = true
reason = "Deprecated code, no agent needed"
```

**Detection Strategy Details:**

1. **LLM Semantic Analysis** (Primary, confidence ≥ 0.7)
   - claudegen uses LLM to identify logical module boundaries from codebase structure
   - Language-agnostic: works for any language without hard-coded rules

2. **Hybrid Approach** (0.5 ≤ confidence < 0.7)
   - Combine LLM suggestions with structural signals
   - Merge overlapping boundaries
   - Flag for user review

3. **Structural Signals** (Fallback, confidence < 0.5)
   - Significant directory boundaries (min 3 files, distinct from siblings)
   - Import/dependency clustering (file adjacency matrix)

4. **Top-level directories** (Final Fallback)
   - If all else fails, use top-level directories as modules
   - **REQUIRES user confirmation** at this tier

**Marker File Detection (Clarification):**

Marker files (`Cargo.toml`, `package.json`, `go.mod`, `pom.xml`) are used for **presence detection only** as hints to LLM, NOT for hard-coded module boundary logic:

| Approach | Status | Rationale |
|----------|--------|-----------|
| "If `Cargo.toml` exists → this is a Rust crate module" | ❌ WRONG | Hard-coded language heuristic |
| "Pass marker file presence as context to LLM" | ✅ OK | LLM decides significance |
| "Parse `Cargo.toml` for workspace members" | ❌ WRONG | Language-specific parsing |

This is consistent with Non-Goals: "No hard-coded project-specific heuristics" - marker files are universal indicators that LLM interprets semantically.

**NOT Used:**
- ❌ Language-specific parsing rules (no "Rust mod", "Go packages" hard-coded logic)
- ❌ Framework-specific patterns
- ❌ File extension-based assumptions
- ❌ Parsing marker file contents (only presence/absence detection)

**Rationale:** Follows CLAUDE.md principle "If you need domain knowledge to interpret it → use LLM"

### 7.5 Bootstrap Protocol

**Problem**: First-time claudegen run has no existing module_map, agents, or skills.

**Solution**: Staged bootstrap with sensible defaults.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         BOOTSTRAP FLOW                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  Stage 1: Minimal Scan (Always succeeds)                                    │
│  ├── Detect project root (git root or cwd)                                  │
│  ├── Find marker files (Cargo.toml, package.json, etc.)                     │
│  ├── Build initial file registry                                            │
│  └── Output: .claudegen/bootstrap_state.json                                │
│                                                                              │
│  Stage 2: LLM Analysis (May fail gracefully)                                │
│  ├── Attempt module boundary detection                                      │
│  ├── On success: Use detected modules                                       │
│  ├── On failure: Fallback to top-level directories                         │
│  └── Output: .claudegen/module_map.json (draft)                             │
│                                                                              │
│  Stage 3: Artifact Generation                                               │
│  ├── Generate claude-pilot/SKILL.md                                          │
│  ├── Generate architect.md                                       │
│  ├── Generate qa-reviewer.md                                                │
│  ├── Generate module-{id}.md for each detected module                       │
│  └── Output: skills/, agents/, plugin.json                                  │
│                                                                              │
│  Stage 4: Validation                                                        │
│  ├── Validate all required files exist                                      │
│  ├── Validate schema compliance                                             │
│  └── Output: Success or error report                                        │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Bootstrap Defaults** (when LLM analysis fails):

| Component | Default Value |
|-----------|---------------|
| Module boundaries | Top-level directories (excl. common ignores) |
| Module count | 1-5 depending on project size |
| Module agent model | sonnet |
| QA agent model | haiku |
| Coverage ratio | 0.0 (unknown) |

**Common Ignores** (always excluded from module detection):

```
.git/, node_modules/, target/, build/, dist/,
__pycache__/, .venv/, venv/, .idea/, .vscode/,
*.log, *.tmp, *.bak
```

**Re-bootstrap Behavior**:

| Scenario | Action |
|----------|--------|
| No existing plugin | Full bootstrap |
| Existing plugin, no `--force` | Skip, use existing |
| Existing plugin, `--force` | Backup existing, full re-bootstrap |
| Corrupted plugin | Warn, offer re-bootstrap |

**Implementation**:

```rust
impl BootstrapPhase {
    pub async fn run(&self, opts: &BootstrapOptions) -> Result<BootstrapResult> {
        // Stage 1: Always succeeds
        let file_registry = self.scan_project(&opts.project_root).await?;

        // Stage 2: May use fallback
        let modules = match self.detect_modules(&file_registry).await {
            Ok(detected) if detected.confidence >= 0.5 => detected.modules,
            Ok(detected) => {
                tracing::warn!(
                    confidence = %detected.confidence,
                    "Low confidence module detection, using fallback"
                );
                self.fallback_modules(&file_registry)
            }
            Err(e) => {
                tracing::warn!(error = %e, "Module detection failed, using fallback");
                self.fallback_modules(&file_registry)
            }
        };

        // Stage 3: Generate artifacts
        let artifacts = self.generate_artifacts(&modules, &file_registry).await?;

        // Stage 4: Validate
        self.validate_artifacts(&artifacts)?;

        Ok(BootstrapResult {
            modules,
            artifacts,
            used_fallback: modules.iter().any(|m| m.is_fallback),
        })
    }

    fn fallback_modules(&self, registry: &FileRegistry) -> Vec<Module> {
        registry
            .top_level_directories()
            .into_iter()
            .filter(|d| !COMMON_IGNORES.contains(&d.as_str()))
            .take(5)
            .map(|dir| Module {
                module_id: dir.clone(),
                paths: vec![format!("{}/", dir)],
                coverage_ratio: 0.0,
                key_files: vec![],
                dependencies: vec![],
                is_fallback: true,
            })
            .collect()
    }
}
```

---

## 8. Request Router (claude-pilot)

### 8.1 Tiered Routing Strategy

| Affected Modules | Confidence | Strategy | Participants |
|-----------------|------------|----------|--------------|
| 0-1 | ≥ 0.9 | Direct execution | None |
| 1 | < 0.9 | Module owner decides | module-{id} |
| 2-3 | Any | Mini-consensus | module-{id}s + architect |
| 4+ | Any | Full consensus | all affected + architect + QA |
| Architecture change | Any | Full + security | all + architect + security |

### 8.2 Impact Analysis

**CONCEPTUAL**: The code below documents what the **orchestrator LLM** does semantically when analyzing a request. This is NOT claude-pilot Rust code — the orchestrator LLM performs this reasoning naturally. claude-pilot only provides the module_map as input context.

```rust
pub struct ImpactAnalysis {
    pub affected_modules: Vec<ModuleId>,
    pub affected_paths: Vec<PathBuf>,
    pub confidence: f64,
    pub has_api_changes: bool,
    pub has_security_implications: bool,
}

// CONCEPTUAL: Orchestrator LLM reasoning process (not executable code)
// Orchestrator reads module_map.json and reasons about request impact
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
| Bug fix | orchestrator + module-{id} + qa-reviewer |
| Feature | orchestrator + architect + module-{id} + qa-reviewer |
| Refactor | orchestrator + architect + module-{id} + qa-reviewer |
| Performance | orchestrator + module-{id} + qa-reviewer |
| Security | orchestrator + security-reviewer + module-{id} + qa-reviewer |

**Task Type Detection (Orchestrator Responsibility)**:

Task type is determined by the **orchestrator LLM**, NOT by keyword matching:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         TASK TYPE DETECTION FLOW                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  1. User request received by orchestrator                                    │
│  2. Orchestrator LLM semantically analyzes request intent                   │
│  3. Task type included in consensus proposal                                │
│  4. Task type MAY change after evidence gathering reveals new scope         │
│     (e.g., "fix bug" reveals it needs architectural change → Refactor)      │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Why LLM Detection**: Following CLAUDE.md principle "If you need domain knowledge to interpret it → use LLM". Keyword matching fails for:
- "Make it faster" → could be Performance or Refactor
- "Update the login" → could be Bug fix, Feature, or Security
- Multi-language requests (Korean/Japanese/etc.)

**Note**: Module leaders (module-{id}.md agents) handle implementation work within their scope. There is no separate "coder" agent - module expertise includes implementation capability.

---

## 9. Consensus Model (claude-pilot)

**Code Clarification**: Consensus logic operates at TWO levels:
1. **Within-session**: Orchestrator LLM performs voting by calling Task tool for each agent, collecting responses, and evaluating results naturally
2. **Cross-session**: claude-pilot persists consensus state to SQLite and injects resume context into the next session

The Rust code below documents claude-pilot's **cross-session state management** and **post-session processing**, not in-session LLM behavior. The orchestrator LLM implements the voting flow by making Task calls and reasoning about responses.

### 9.1 Consensus Flow

**Single-Participant Exception (MODULE Tier)**:
When only one module leader participates (1 affected module, confidence < 0.9), consensus is skipped:
- Module leader's proposal becomes the accepted plan directly
- No voting phase required
- Proceed directly to EXECUTION PHASE

**Multi-Participant Flow (MINI/FULL Tier)**:

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

**Evidence Quality Calculation**:

```rust
/// Calculate evidence quality score (0.0 - 1.0)
/// Prevents circular reference by using only verifiable metrics
fn evidence_quality(refs: &[EvidenceRef], file_registry: &FileRegistry) -> f64 {
    if refs.is_empty() {
        return 0.0;
    }

    // Factor 1: Valid file references (files exist in registry)
    let valid_refs = refs.iter()
        .filter(|r| file_registry.contains(&r.path))
        .count();
    let validity_ratio = valid_refs as f64 / refs.len() as f64;

    // Factor 2: Line specificity (references with line numbers)
    let line_specific = refs.iter()
        .filter(|r| r.line.is_some())
        .count();
    let specificity_ratio = line_specific as f64 / refs.len() as f64;

    // Factor 3: Reference diversity (unique files referenced)
    let unique_files: HashSet<_> = refs.iter().map(|r| &r.path).collect();
    let diversity_ratio = (unique_files.len() as f64 / refs.len() as f64).min(1.0);

    // Weighted combination (no circular dependencies)
    validity_ratio * 0.50      // Most important: refs must be real
    + specificity_ratio * 0.30 // Line-level refs are more valuable
    + diversity_ratio * 0.20   // Diverse evidence is stronger
}
```

**Voting Calculation**:

```rust
fn calculate_consensus(&self, votes: &[ConsensusVote]) -> ConsensusResult {
    let mut weighted_approve = 0.0;
    let mut total_weight = 0.0;

    for vote in votes {
        // Weight = evidence quality * agent confidence
        // Both factors are independently calculated (no circular reference)
        let evidence_score = evidence_quality(&vote.evidence, &self.file_registry);
        let weight = evidence_score * vote.confidence;
        total_weight += weight;

        match vote.decision {
            VoteDecision::Approve => weighted_approve += weight,
            VoteDecision::ApproveWithChanges => weighted_approve += weight * 0.8,
            VoteDecision::RequestMoreInfo => {} // No weight contribution
            VoteDecision::Reject => {} // No weight contribution
        }
    }

    // Prevent division by zero
    let approval_ratio = if total_weight > 0.0 {
        weighted_approve / total_weight
    } else {
        0.0  // No valid votes = no approval
    };

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
- **Oscillation detected** (same proposal hash appears twice)

**Action**: Escalate to user with full context + recommendations

### 9.6 Oscillation Detection

**Problem**: Consensus can enter infinite loop if revised proposals cycle back to previous states.

```
Round 1: Proposal A → Rejected → Suggestions lead to Proposal B
Round 2: Proposal B → Rejected → Suggestions lead to Proposal A  ← OSCILLATION!
Round 3: Proposal A → Rejected → ...
```

**Solution**: Track proposal hashes and detect cycles.

```rust
pub struct ConsensusState {
    pub current_round: u32,
    pub proposal_history: Vec<ProposalHash>,
    pub oscillation_count: u32,
}

impl ConsensusEngine {
    fn check_oscillation(&self, new_proposal: &Proposal) -> OscillationCheck {
        let hash = self.hash_proposal(new_proposal);

        if self.state.proposal_history.contains(&hash) {
            let cycle_start = self.state.proposal_history
                .iter()
                .position(|h| h == &hash)
                .unwrap();

            return OscillationCheck::Detected {
                cycle_length: self.state.proposal_history.len() - cycle_start,
                repeated_hash: hash,
            };
        }

        OscillationCheck::Clear
    }

    fn hash_proposal(&self, proposal: &Proposal) -> ProposalHash {
        // Hash key decision points, not entire proposal
        let mut hasher = blake3::Hasher::new();

        // Hash affected modules
        for module in &proposal.affected_modules {
            hasher.update(module.as_bytes());
        }

        // Hash task decomposition structure
        for task in &proposal.tasks {
            hasher.update(task.module_id.as_bytes());
            hasher.update(task.action_type.as_bytes());
        }

        // Hash key constraints
        for constraint in &proposal.constraints {
            hasher.update(constraint.as_bytes());
        }

        ProposalHash(hasher.finalize().to_hex().to_string())
    }
}
```

**Oscillation Handling**:

| Oscillation Count | Action |
|-------------------|--------|
| 1 | Log warning, try architect tiebreaker |
| 2 | Force accept highest-evidence proposal |
| 3+ | Escalate to user immediately |

**Architect Tiebreaker Protocol**:

When architect acts as tiebreaker, they do NOT re-vote. Instead:

```rust
pub enum TiebreakerDecision {
    /// Pick one of the conflicting proposals as winner
    PickWinner { selected_proposal: ProposalId, rationale: String },

    /// Force split: decompose into separate non-conflicting tasks
    ForceSplit { decomposition: Vec<TaskId>, rationale: String },

    /// Cannot decide: escalate immediately
    CannotDecide { reason: String },
}

impl ConsensusEngine {
    fn architect_tiebreaker(&self, conflict: &Conflict) -> TiebreakerDecision {
        // Architect evaluates ONLY the conflicting proposals
        // Does NOT re-run full voting
        // Returns a decision that breaks the deadlock
    }
}
```

**Tiebreaker Rules**:
- Architect's previous vote (if any) is ignored during tiebreaker
- Tiebreaker decision is final for that oscillation cycle
- If PickWinner, that proposal proceeds without re-voting

**Event Emission**:

```rust
// On oscillation detection
self.event_store.append(Event::OscillationDetected {
    proposal_hash: hash,
    cycle_length: cycle_len,
    round: current_round,
    previous_occurrence: first_occurrence_round,
}).await?;
```

### 9.7 Partial Consensus Handling

**Problem**: When approval is between 50-67%, suggestions may conflict.

```
Scenario:
- Agent A: ApproveWithChanges → "Use async/await"
- Agent B: ApproveWithChanges → "Use callbacks for compatibility"
- Agent C: Reject → "Need more evidence"
- Weighted approval: 62% (below 67% threshold)
```

**Solution**: Conflict-aware suggestion merging with escalation.

```rust
pub struct SuggestionMerger {
    conflict_detector: ConflictDetector,
}

impl SuggestionMerger {
    fn merge_suggestions(&self, votes: &[ConsensusVote]) -> MergeResult {
        let suggestions: Vec<_> = votes.iter()
            .filter(|v| matches!(v.decision, VoteDecision::ApproveWithChanges))
            .flat_map(|v| &v.suggested_changes)
            .collect();

        // Detect conflicts
        let conflicts = self.conflict_detector.detect(&suggestions);

        if conflicts.is_empty() {
            // No conflicts: merge all suggestions
            MergeResult::Merged(self.combine_suggestions(&suggestions))
        } else if suggestions.is_empty() {
            // All reject with no suggestions: immediate escalation
            MergeResult::Escalate(EscalationReason::NoActionableFeedback)
        } else {
            // Conflicts exist: use evidence-weighted resolution
            MergeResult::ConflictResolution(self.resolve_by_evidence(&conflicts, votes))
        }
    }

    fn resolve_by_evidence(
        &self,
        conflicts: &[Conflict],
        votes: &[ConsensusVote],
    ) -> Vec<PlanChange> {
        conflicts.iter().map(|conflict| {
            // Pick suggestion with highest evidence quality
            let winner = conflict.options.iter()
                .max_by(|a, b| {
                    let a_evidence = self.evidence_for_suggestion(a, votes);
                    let b_evidence = self.evidence_for_suggestion(b, votes);
                    a_evidence.partial_cmp(&b_evidence).unwrap()
                })
                .unwrap();

            winner.clone()
        }).collect()
    }
}

pub enum MergeResult {
    Merged(Vec<PlanChange>),
    ConflictResolution(Vec<PlanChange>),
    Escalate(EscalationReason),
}

pub enum EscalationReason {
    NoActionableFeedback,
    UnresolvableConflict,
    InsufficientEvidence,
}
```

**Conflict Detection Rules**:

| Conflict Type | Detection | Resolution |
|---------------|-----------|------------|
| Same file, different changes | Path overlap | Higher evidence wins |
| Contradictory approaches | Keyword analysis | Architect tiebreaker |
| Missing dependencies | Dependency graph | Add all dependencies |
| Scope disagreement | Module boundary check | Smaller scope wins |

**Event Emission**:

```rust
self.event_store.append(Event::SuggestionConflictDetected {
    conflict_type: conflict.type_name(),
    options: conflict.options.len(),
    resolution_method: resolution.method(),
}).await?;
```

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

    // Agent Failure & Recovery (see Section 14.6)
    AgentFailed,
    RecoveryActionTaken,
    RollbackCompleted,
    CheckpointCreated,

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

**IMPORTANT**: Orchestrator is READ-ONLY (no Write tool). SharedMemory writes are handled by:
1. **Module agents**: Write directly to their namespace via Write tool
2. **claude-pilot**: Writes on behalf of orchestrator post-session

| Role | project/ | module/{id}/ | consensus/{id}/ | session/ |
|------|----------|--------------|-----------------|----------|
| Orchestrator | R (Read tool) | R (Read tool) | R (Read tool) | R (Read tool) |
| claude-pilot (runtime) | R/W | R/W | R/W | R/W |
| Module Leader | R | R/W (own only) | R/W (participating) | R |
| QA Agent | R | R | R | R |
| Architect | R | R | R/W | R |

**How Agents Access SharedMemory**:

SharedMemory is implemented as **JSON files** at `.claudegen/shared/`:

```
.claudegen/shared/
├── project/
│   └── architecture.json
├── module/
│   ├── auth/context.json
│   └── api/context.json
├── consensus/
│   └── {consensus_id}/state.json
└── session/
    └── current.json
```

**Agent Access Pattern**:
- **Read**: Agents use `Read` tool to read `.claudegen/shared/{namespace}/{key}.json`
- **Write**: Agents output structured commands that hooks/claude-pilot process:
  ```
  <shared-memory-write namespace="module/auth" key="context">
  {"last_updated": "...", "key_findings": [...]}
  </shared-memory-write>
  ```
- **Enforcement**: Hooks can validate writes, claude-pilot processes post-session

**Race Condition Prevention**:

Multiple agents writing simultaneously can cause data loss. Mitigation strategies:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     SHARED MEMORY WRITE STRATEGIES                           │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  Strategy 1: Agent-Scoped Files (RECOMMENDED)                               │
│  ├── Each agent writes to own file: module/{id}/own.json                    │
│  ├── No contention between agents                                           │
│  └── claude-pilot merges at read time                                       │
│                                                                              │
│  Strategy 2: Append-Only JSONL for Session                                  │
│  ├── session/events.jsonl (append-only)                                     │
│  ├── Each write appends a new line                                          │
│  └── claude-pilot consolidates entries post-session                         │
│                                                                              │
│  Strategy 3: Tag-Based Output (No Files)                                    │
│  ├── Agents output <shared-memory-write> tags                               │
│  ├── claude-pilot parses transcript post-session                            │
│  └── Single writer (claude-pilot) prevents race                             │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

**File Structure with Race Prevention**:

```
.claudegen/shared/
├── project/
│   └── architecture.json          # Written by claude-pilot only
├── module/
│   ├── auth/
│   │   └── own.json               # Written by module-auth agent only
│   └── api/
│       └── own.json               # Written by module-api agent only
├── consensus/
│   └── {consensus_id}/state.json  # Written by claude-pilot only
└── session/
    └── events.jsonl               # Append-only, all agents
```

### 11.3 TTL Policy

| Namespace | TTL | Purpose |
|-----------|-----|---------|
| project/ | 30 days | Long-term decisions |
| module/ | 7 days | Module context |
| consensus/ | 24 hours | Active consensus |
| session/ | Session | Ephemeral state |

### 11.4 Source of Truth & Reconstruction

- **Event store is the system of record** (30-day retention)
- Shared memory is a **cache** for fast coordination
- Conflicts resolved in favor of event log

**TTL vs Event Store Mismatch Resolution**:

Shared memory entries may expire before event store retention:
- consensus/ expires in 24h but events retained 30 days
- Solution: **Lazy reconstruction from events**

```rust
impl SharedMemory {
    pub async fn get(&self, namespace: &str, key: &str) -> Option<Value> {
        // 1. Try cache first
        if let Some(cached) = self.cache.get(namespace, key).await {
            if !cached.is_expired() {
                return Some(cached.value);
            }
        }

        // 2. Cache miss or expired → reconstruct from events
        if namespace.starts_with("consensus/") {
            return self.reconstruct_consensus(key).await.ok();
        }
        if namespace.starts_with("module/") {
            return self.reconstruct_module_context(key).await.ok();
        }

        None
    }

    async fn reconstruct_consensus(&self, consensus_id: &str) -> Result<Value> {
        // Replay events to rebuild consensus state
        let events = self.event_store
            .query_by_aggregate(&format!("consensus:{}", consensus_id))
            .await?;

        let mut state = ConsensusState::default();
        for event in events {
            state.apply(event)?;
        }

        // Re-cache for future access
        let value = serde_json::to_value(&state)?;
        self.cache.set(&format!("consensus/{}", consensus_id), value.clone()).await?;

        Ok(value)
    }
}
```

**Reconstruction Guarantees**:

| Namespace | Reconstruct From | Latency |
|-----------|-----------------|---------|
| consensus/ | ConsensusStarted, VoteReceived, ConsensusAccepted events | ~100ms |
| module/ | TaskCompleted events for that module | ~50ms |
| project/ | Architecture decisions, convention changes | ~200ms |
| session/ | **Not reconstructable** (ephemeral by design) | N/A |

### 11.5 Pattern Bank Persistence Strategy

**Problem**: Event store has 30-day retention, but learned patterns should persist longer.

**Solution**: Pattern Bank stores self-contained evidence, independent of event store.

```rust
pub struct FixPattern {
    pub id: Ulid,
    pub signature: ErrorSignature,
    pub strategy: FixStrategy,

    // Self-contained evidence (not event_ids)
    pub evidence: PatternEvidence,

    // Statistics
    pub success_count: u32,
    pub failure_count: u32,
    pub last_used: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

pub struct PatternEvidence {
    /// Snapshot of the error that led to this pattern
    pub original_error: ErrorSnapshot,

    /// Snapshot of the successful fix
    pub fix_snapshot: FixSnapshot,

    /// File patterns where this applies (not specific paths)
    pub applicable_patterns: Vec<String>,  // e.g., "src/**/*.rs", "tests/**/*_test.go"

    /// Keywords that indicate applicability
    pub keywords: Vec<String>,
}

pub struct ErrorSnapshot {
    pub category: IssueCategory,
    pub message_template: String,  // With placeholders: "cannot find type `{}`"
    pub error_code: Option<String>,
}

pub struct FixSnapshot {
    pub strategy_type: String,
    pub description: String,
    pub example_diff: Option<String>,  // Anonymized example
}
```

**Retention Policy**:

| Data Type | Retention | Reason |
|-----------|-----------|--------|
| Event Store | 30 days | Audit trail, state reconstruction |
| Pattern Bank | **Indefinite** | Learned knowledge, high value |
| Pattern Evidence | With pattern | Self-contained, no external refs |

**Pattern Graduation**:

Patterns graduate from event-dependent to self-contained:

```rust
impl PatternBank {
    /// Called when event store is about to expire relevant events
    pub async fn graduate_patterns(&mut self, expiring_events: &[EventId]) -> Result<()> {
        for pattern in &mut self.patterns {
            if pattern.references_events(expiring_events) {
                // Upgrade to self-contained evidence
                pattern.evidence = self.snapshot_evidence(pattern).await?;
                pattern.event_refs.clear();
            }
        }
        Ok(())
    }

    fn snapshot_evidence(&self, pattern: &FixPattern) -> PatternEvidence {
        PatternEvidence {
            original_error: ErrorSnapshot {
                category: pattern.signature.category,
                message_template: self.templatize_message(&pattern.original_message),
                error_code: pattern.signature.error_code.clone(),
            },
            fix_snapshot: FixSnapshot {
                strategy_type: pattern.strategy.type_name(),
                description: pattern.strategy.description.clone(),
                example_diff: self.anonymize_diff(&pattern.last_successful_diff),
            },
            applicable_patterns: self.generalize_paths(&pattern.successful_paths),
            keywords: pattern.signature.keywords.clone(),
        }
    }

    fn templatize_message(&self, message: &str) -> String {
        // Replace specific identifiers with placeholders
        // "cannot find type `MyStruct`" → "cannot find type `{}`"
        IDENTIFIER_REGEX.replace_all(message, "{}").to_string()
    }

    fn anonymize_diff(&self, diff: &str) -> Option<String> {
        // Remove project-specific paths and identifiers
        // Keep structure and pattern of the fix
        Some(ANONYMIZE_REGEX.replace_all(diff, "[...]").to_string())
    }
}
```

**Consolidation Schedule**:

```rust
pub struct PatternConsolidator {
    consolidation_interval: Duration,  // Default: 1 hour
    graduation_check_interval: Duration,  // Default: 1 day
}

impl PatternConsolidator {
    pub async fn run_maintenance(&mut self) -> Result<()> {
        // 1. Remove low-performing patterns (< 30% success, 30 days unused)
        self.remove_low_performers().await?;

        // 2. Merge similar patterns (> 90% signature similarity)
        self.merge_similar_patterns().await?;

        // 3. Graduate patterns before event expiration
        let expiring = self.event_store.events_expiring_within(Duration::days(1)).await?;
        self.pattern_bank.graduate_patterns(&expiring).await?;

        Ok(())
    }
}
```

**EventStore Expiration Query** (required for graduation):

```rust
impl EventStore {
    /// Find events that will be deleted by retention policy within the given duration.
    /// Events are retained for 30 days from creation (see config: retention_days = 30).
    pub async fn events_expiring_within(&self, duration: Duration) -> Result<Vec<EventId>> {
        // Calculate cutoff: events older than (30 days - duration) will expire within duration
        let retention = Duration::days(self.config.retention_days as i64);
        let cutoff = Utc::now() - (retention - duration);

        let events = sqlx::query!(
            "SELECT id FROM events WHERE timestamp < ? ORDER BY timestamp ASC",
            cutoff
        )
        .fetch_all(&self.db)
        .await?
        .into_iter()
        .map(|row| EventId::from_string(&row.id))
        .collect();

        Ok(events)
    }
}
```

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
│   └── claude-pilot/
│       └── SKILL.md             # REQUIRED: entry skill
├── agents/
│   ├── architect.md  # REQUIRED: main coordinator
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
        let orchestrate_skill = self.load_skill("claude-pilot")
            .context("claude-pilot skill is required")?;

        // 4. Load required agents
        let orchestrator_agent = self.load_agent("architect")
            .context("architect agent is required")?;
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
| claude-pilot skill | Fail fast |
| architect agent | Fail fast |
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

**Goal**: Close the gap between current state and minimal viable multi-agent system.

**Gap Analysis (Current State)**:

| Component | claudegen | claude-pilot |
|-----------|-----------|--------------|
| Plugin output | ❌ None | N/A |
| Module detection | ⚠️ Partial (DistributedAnalyzer) | ❌ No loader |
| Agent generation | ⚠️ Type exists, unused | ❌ No execution |
| Consensus | N/A | ❌ Config only |
| Event store | N/A | ⚠️ Checkpoint only |

**claudegen:**
- [ ] Add module boundary detection phase
- [ ] Implement module_map.json generation (schema-compliant)
- [ ] Generate plugin.json manifest
- [ ] Generate claude-pilot/SKILL.md template
- [ ] Generate architect.md agent
- [ ] Generate qa-reviewer.md agent (unified, not per-language)
- [ ] Generate per-module agents

**claude-pilot:**
- [ ] Implement plugin loader (file system based)
- [ ] Implement module_map.json parser
- [ ] Implement request router with tiered routing logic
- [ ] Implement TaskPromptBuilder for context embedding in Task prompts
- [ ] Implement SubagentStop hook scripts for result capture

**Deliverable:** Single-module tasks work with generated agents

### Phase 2: Consensus (Weeks 5-8)

**claudegen:**
- [ ] Generate cross-cutting agents (architect, security)
- [ ] Generate consensus-planning skill
- [ ] Add evidence references to agents
- [ ] Generate qa-reviewer.md (unified, with language-specific tool dispatch)

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

### 14.6 Agent Failure Recovery

**Problem**: Module agents can fail mid-execution due to:
- Context window exhaustion
- API rate limits / timeouts
- Invalid tool calls
- Unrecoverable errors

**Solution**: Hierarchical recovery with state preservation.

**CRITICAL: Rollback Execution Model**:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     ROLLBACK EXECUTION PATHS                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  Path A: Mid-Session Rollback (immediate)                                   │
│  ├── Orchestrator detects failure from Task tool response                   │
│  ├── Orchestrator spawns "rollback agent" via Task tool                     │
│  │   ├── rollback agent has Write permissions                               │
│  │   ├── Reads checkpoint file states                                       │
│  │   └── Restores files to pre-task state                                   │
│  └── Orchestrator continues with retry or next task                         │
│                                                                              │
│  Path B: Post-Session Rollback (deferred)                                   │
│  ├── claude-pilot detects failure from transcript/events                    │
│  ├── Uses git to restore files: git checkout -- <modified_files>            │
│  └── Injects recovery context into next session                            │
│                                                                              │
│  NOTE: The Rust code below documents claude-pilot's cross-session           │
│  recovery logic. Mid-session rollback is handled by a dedicated             │
│  rollback agent spawned by the orchestrator.                                │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

```rust
pub struct AgentFailureHandler {
    event_store: EventStore,
    checkpoint_manager: CheckpointManager,
}

pub enum FailureType {
    ContextExhaustion,
    ApiTimeout,
    RateLimit,
    ToolError(String),
    UnknownError(String),
}

pub enum RecoveryAction {
    Retry { delay: Duration, with_context_reduction: bool },
    Reassign { to_agent: AgentId, with_checkpoint: CheckpointId },
    Rollback { to_checkpoint: CheckpointId },
    Escalate { reason: String },
}

impl AgentFailureHandler {
    pub async fn handle_failure(
        &self,
        agent_id: &AgentId,
        task_id: &TaskId,
        failure: FailureType,
    ) -> Result<RecoveryAction> {
        // 1. Emit failure event
        self.event_store.append(Event::AgentFailed {
            agent_id: agent_id.clone(),
            task_id: task_id.clone(),
            failure_type: failure.type_name(),
            timestamp: Utc::now(),
        }).await?;

        // 2. Create checkpoint of current state
        let checkpoint = self.checkpoint_manager
            .create_failure_checkpoint(agent_id, task_id)
            .await?;

        // 3. Determine recovery action based on failure type
        let action = match failure {
            FailureType::ContextExhaustion => {
                // Reduce context and retry
                RecoveryAction::Retry {
                    delay: Duration::seconds(0),
                    with_context_reduction: true,
                }
            }

            FailureType::ApiTimeout | FailureType::RateLimit => {
                // Exponential backoff retry
                let retry_count = self.get_retry_count(task_id).await?;
                if retry_count < 3 {
                    RecoveryAction::Retry {
                        delay: Duration::seconds(2_u64.pow(retry_count)),
                        with_context_reduction: false,
                    }
                } else {
                    RecoveryAction::Escalate {
                        reason: "Max retries exceeded for API errors".to_string(),
                    }
                }
            }

            FailureType::ToolError(ref tool) => {
                // Check if another agent can handle this
                if let Some(alternate) = self.find_alternate_agent(task_id, tool).await? {
                    RecoveryAction::Reassign {
                        to_agent: alternate,
                        with_checkpoint: checkpoint.id,
                    }
                } else {
                    RecoveryAction::Rollback {
                        to_checkpoint: self.last_successful_checkpoint(task_id).await?,
                    }
                }
            }

            FailureType::UnknownError(ref msg) => {
                RecoveryAction::Escalate {
                    reason: format!("Unknown error: {}", msg),
                }
            }
        };

        // 4. Emit recovery action event
        self.event_store.append(Event::RecoveryActionTaken {
            task_id: task_id.clone(),
            checkpoint_id: checkpoint.id,
            action: action.type_name(),
        }).await?;

        Ok(action)
    }
}
```

**Recovery Strategy Matrix**:

| Failure Type | 1st Attempt | 2nd Attempt | 3rd Attempt | Final |
|--------------|-------------|-------------|-------------|-------|
| Context Exhaustion | Reduce context, retry | Summarize + retry | Split task | Escalate |
| API Timeout | Wait 2s, retry | Wait 4s, retry | Wait 8s, retry | Escalate |
| Rate Limit | Wait 2s, retry | Wait 4s, retry | Wait 8s, retry | Escalate |
| Tool Error | Try alternate agent | Rollback to checkpoint | - | Escalate |
| Unknown Error | - | - | - | Escalate immediately |

**Partial Completion Handling**:

```rust
pub struct PartialCompletionState {
    pub task_id: TaskId,
    pub completed_steps: Vec<StepId>,
    pub pending_steps: Vec<StepId>,
    pub files_modified: Vec<PathBuf>,
    pub rollback_possible: bool,
}

impl CheckpointManager {
    pub async fn handle_partial_completion(
        &self,
        state: PartialCompletionState,
    ) -> Result<PartialRecoveryAction> {
        if state.rollback_possible && state.completed_steps.len() < 2 {
            // Few changes made, safe to rollback
            Ok(PartialRecoveryAction::Rollback)
        } else if state.pending_steps.len() == 1 {
            // Almost done, try to complete
            Ok(PartialRecoveryAction::CompleteRemaining)
        } else {
            // Complex partial state, need human decision
            Ok(PartialRecoveryAction::EscalateWithOptions {
                options: vec![
                    "Rollback all changes",
                    "Keep completed, abandon rest",
                    "Continue with fresh agent",
                ],
            })
        }
    }
}
```

**Rollback Mechanism**:

```rust
impl CheckpointManager {
    pub async fn rollback_to(&self, checkpoint_id: CheckpointId) -> Result<()> {
        let checkpoint = self.load_checkpoint(checkpoint_id).await?;

        // 1. Restore modified files
        for (path, original_content) in &checkpoint.file_states {
            if let Some(content) = original_content {
                fs::write(path, content).await?;
            } else {
                // File was created during task, remove it
                fs::remove_file(path).await.ok();
            }
        }

        // 2. Emit rollback event
        self.event_store.append(Event::RollbackCompleted {
            checkpoint_id,
            files_restored: checkpoint.file_states.len(),
        }).await?;

        Ok(())
    }
}
```

---

## 15. Risk Mitigation

| Risk | Severity | Mitigation |
|------|----------|------------|
| **Cost explosion** | HIGH | Cap 8 agents per round, Haiku for voting, Sonnet for implementation, cache contexts |
| **Consensus deadlock** | HIGH | Max 3 rounds, architect tiebreaker, user escalation |
| **Consensus oscillation** | HIGH | Hash-based cycle detection, force-accept on 2nd oscillation (Section 9.6) |
| **Module boundary errors** | MEDIUM | Confidence thresholds, hybrid detection, user confirmation below 0.5 (Section 7.4) |
| **Context loss in sub-agents** | HIGH | Prompt-embedded context via `<task-context>` tags (Section 6.3) |
| **QA agent proliferation** | MEDIUM | Unified qa-reviewer with language-specific tool dispatch, not N agents |
| **Stale context** | MEDIUM | 7-day TTL, event-driven updates, lazy reconstruction from event store |
| **Plugin incompatibility** | LOW | Strict schema versioning, fail-fast validation |
| **Event Store / Shared Memory mismatch** | LOW | Lazy reconstruction from events on cache miss (Section 11.4) |

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
- [ ] Generate architect.md
- [ ] Generate module-{id}.md for each module
- [ ] Generate architect.md
- [ ] Generate qa-reviewer.md (unified, language-agnostic)
- [ ] Generate security-reviewer.md

**Skill Generation:**
- [ ] Generate claude-pilot/SKILL.md (entry point)
- [ ] Generate consensus-planning/SKILL.md
- [ ] Generate module-{id}/SKILL.md for each module
- [ ] Generate qa-review/SKILL.md (unified QA skill)

**Rules Generation:**
- [ ] Generate global rules (security, error-handling)
- [ ] Generate language-specific rules
- [ ] Generate module-specific rules with paths frontmatter

**Plugin Output:**
- [ ] Generate plugin.json manifest
- [ ] Include schema version for compatibility
- [ ] Add hooks configuration

**Hook Scripts Generation:**
- [ ] Generate .claudegen/hooks/validate-module-scope.sh (validates Edit/Write within module)
- [ ] Generate .claudegen/hooks/run-module-tests.sh (runs tests after edits)
- [ ] Generate settings.json hook configuration template
- [ ] Generate event capture hooks (SubagentStart/Stop)

**Hook Script Generation Rules:**
```
1. Output path: {plugin_root}/.claudegen/hooks/{script_name}.sh
2. Scripts MUST use $CLAUDE_PROJECT_DIR for all path references (portability)
3. Scripts MUST be executable (chmod +x during generation)
4. Scripts MUST output valid JSON for machine parsing
5. Exit codes: 0 = success, non-zero = block the operation
6. PreToolUse/PostToolUse hooks receive tool input via STDIN (JSON), NOT as CLI args

Example generated script:
#!/bin/bash
# validate-module-scope.sh - Generated by claudegen
# Usage: Invoked by Claude Code PreToolUse hook
# CLI arg $1 = module_id (hardcoded in agent hook command)
# STDIN = {"tool":"Edit","input":{"file_path":"...","old_string":"...","new_string":"..."}}
MODULE_ID="$1"
TOOL_INPUT=$(cat -)
FILE_PATH=$(echo "$TOOL_INPUT" | jq -r '.input.file_path // .input.command // empty')
MODULE_PATHS=$(jq -r ".modules[] | select(.module_id==\"$MODULE_ID\") | .paths[]" \
    "$CLAUDE_PROJECT_DIR/.claudegen/module_map.json")
# ... validation logic ...
```

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

## 18. Success Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| Single-module task success | > 95% | Completion rate |
| Multi-module consensus rate | > 80% | Accepted within 3 rounds |
| Convergence rate | > 90% | Achieved 2 clean rounds |
| Time to consensus | < 2 min | Average across tasks |
| User escalation rate | < 10% | Tasks requiring human |
| Evidence quality | > 0.70 | Average score |
| Agent accuracy | > 85% | Module scope validation |

---

## 19. Key Constraints Summary

### NON-NEGOTIABLE Requirements

1. **Orchestrator is READ-ONLY**: Uses Task tool to delegate, never directly modifies files
2. **Skills cannot have `skills:` field**: Content must be included directly in skill body
3. **2-round convergent verification**: Must pass 2 consecutive clean rounds
4. **Evidence-based decisions**: All claims must have @file:line references
5. **Tier 1 content rejection**: Generic knowledge is always rejected

### Claude Code Spec Limitations

| Feature | Skills | Agents | Plugin.json |
|---------|--------|--------|-------------|
| `skills:` field | ❌ | ✅ | N/A |
| `hooks:` field | ✅ (limited) | ✅ (limited) | ✅ (full) |
| SubagentStart/Stop hooks | ❌ | ❌ | ✅ |
| PreToolUse/PostToolUse/Stop | ✅ | ✅ | ✅ |
| SessionStart/End | ❌ | ❌ | ✅ |

**Context Propagation Constraint**: Task tool calls CANNOT be intercepted. Use prompt-embedded context:
1. Orchestrator embeds `<task-context>` JSON directly in Task `prompt` parameter (NO file writes)
2. Sub-agent parses context from prompt (NO file reads required)
3. Sub-agent outputs `<task-result>` in conversation
4. Result capture: Orchestrator parses immediately OR claude-pilot parses post-session
5. This approach respects orchestrator's READ-ONLY permission constraint

**claude-pilot Implementation Model** (see Section 4.2):
- claude-pilot is a **CLI wrapper** that runs BEFORE and AFTER Claude Code sessions
- NOT a daemon, NOT real-time interception
- Pre-session: Load plugin, configure hooks
- Post-session: Parse transcript, extract events, store to SQLite

### Claudegen Schema Requirements

**plugin.json** (per `docs/schemas/plugin.json.schema.json`):
- Required: `schema_version`, `generator` (string), `project_name`
- `generator` format: `"claudegen@2.0.0"` (string, not object)

**module_map.json** (per `docs/schemas/module_map.json.schema.json`):
- Required: `module_map_version`, `modules[]`
- Per module required: `module_id`, `paths`, `coverage_ratio`, `key_files` (string[]), `dependencies`
- `key_files` is array of strings, not objects

### Environment Variables in Hooks

| Variable | Description |
|----------|-------------|
| `CLAUDE_PROJECT_DIR` | Absolute path to project root |
| `CLAUDE_PLUGIN_ROOT` | Absolute path to plugin directory |
| `CLAUDE_ENV_FILE` | File for persisting env vars (SessionStart, Setup) |
| `CLAUDE_CODE_REMOTE` | `true` if running remotely |

---

*Document Version: 3.10.0*
*Status: Final*
*Last Updated: 2025-01-27*
