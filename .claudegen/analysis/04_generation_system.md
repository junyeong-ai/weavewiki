# 04. Generation System and Document Structure Analysis

## 1. Executive Summary

The claudegen generation system is a multi-layered artifact production pipeline that transforms codebase analysis into Claude Code configuration files (rules, skills, agents, CLAUDE.md). The architecture follows an **LLM-First** design philosophy: programmatic analysis provides structured context, but the LLM makes all classification and content decisions. The system uses evidence-based generation (no artifact without verifiable file references), hierarchical organization with priority-based injection, and Progressive Disclosure for managing information density.

**Key architectural decisions:**
- All skills are LLM-discovered; no hardcoded "core" skill set
- Rules use a priority-ordered category hierarchy (100 down to 50)
- Agents are generated in 5 layers with increasing specialization
- CLAUDE.md uses differential updates with section-level caching
- Context is budget-aware with tiered allocation (Tier1/Tier2/Tier3)

---

## 2. System Architecture Overview

### 2.1 Generation Pipeline Flow

```
Analysis Results
      |
      v
GenerationContext (unified data carrier)
      |
      +---> RulesGenerator
      |       |-- ProjectRuleGenerator (priority 100)
      |       |-- TechRuleGenerator (priority 90)
      |       |-- FrameworkRuleGenerator (priority 85)
      |       |-- ModuleRuleGenerator (priority 80)
      |       |-- GroupRuleGenerator (priority 70)
      |       |-- DomainRuleGenerator (priority 75)
      |       |-- CrossCuttingRuleGenerator (priority 75)
      |       +-- CustomRuleCategory (priority 50, dynamic)
      |
      +---> Orchestrator
      |       |-- SkillsGenerator (first)
      |       +-- AgentsGenerator (second, references skills)
      |
      +---> ClaudeMdGenerator
              |-- Section-level caching
              |-- @import extraction for large sections
              +-- NavigationMapGenerator
```

### 2.2 Output Directory Structure

```
.claude/
  rules/
    project.md              (priority 100, always_inject)
    tech/{lang}.md           (priority 90, by extension)
    frameworks/{fw}.md       (priority 85, by path/keywords)
    modules/{module}.md      (priority 80, by module path)
    groups/{group}.md        (priority 70, by member paths)
    domains/{domain}.md      (priority 75, by keyword trigger)
    cross-cutting/{concern}.md (priority 75, by trigger)
    services/{service}.md    (priority 65, by service path)
    {custom-name}/{item}.md  (priority 50, dynamic discovery)
  skills/
    {skill}/SKILL.md         (frontmatter + body)
  agents/
    {agent}.md               (frontmatter + prompt)
  docs/
    architecture.md          (@import from CLAUDE.md)
    standards.md             (@import from CLAUDE.md)
    domain.md                (@import from CLAUDE.md)
CLAUDE.md                    (overview + navigation map)
```

---

## 3. Rule Generation System

### 3.1 Hierarchical Category System

The `RuleCategory` enum (`src/types/rule.rs:18-44`) defines 8 fixed categories plus a `Custom` variant:

| Category | Priority | Subdirectory | Matching Strategy |
|----------|----------|--------------|-------------------|
| Project | 100 | (root) | always_inject=true, paths=`**/*` |
| Tech | 90 | tech/ | File extension matching |
| Framework | 85 | frameworks/ | Path globs + keyword triggers |
| Module | 80 | modules/ | Module path patterns |
| CrossCutting | 75 | cross-cutting/ | Keyword triggers |
| Domain | 75 | domains/ | Keyword triggers |
| Group | 70 | groups/ | Member module paths |
| Service | 65 | services/ | Service-specific paths |
| Custom | 50 (default) | configurable | Dynamic discovery |

**Extensibility mechanism:** The `Custom` variant carries `name: String, priority: Option<u8>, subdirectory: Option<String>`, allowing runtime creation of categories not captured by fixed variants. Custom categories are discovered from `SynthesizedInsights` via `discover_custom_categories()` in `src/pipeline/generation/rules/mod.rs`.

### 3.2 Rule Content Structure

Each rule category has a dedicated generator module:

- **ProjectRuleGenerator** (`rules/project.rs`): Project identity, architecture pattern/layers, global conventions, key directories, anti-patterns, gotchas, language-specific commands (build/test/lint/format), environment variables. Uses `ProjectCommands::from_detection()` with hardcoded commands for 8 languages (rust, typescript, python, go, java, kotlin, ruby, php).

- **TechRuleGenerator** (`rules/tech.rs`): One rule per detected language. Content: error handling, async patterns, naming conventions, testing, detected patterns with evidence citations, language constraints, cross-cutting insights filtered by file extension.

- **FrameworkRuleGenerator** (`rules/framework.rs`): Data-driven with `KNOWN_FRAMEWORKS` const array (10 frameworks: tokio, actix, axum, react, nextjs, express, django, fastapi, gin, spring). Unknown frameworks detected in `tech_stack` also get rules. Content sections: Setup, Async Runtime, Error Handling, Architecture, Testing, Patterns, Constraints.

- **ModuleRuleGenerator** (`rules/module.rs`): Per-module rules with responsibility, dependency table, conventions with evidence, known issues, key files, API exposure detection, anti-patterns, cross-reference insights, and cross-layer `@import` references to tech/framework rules. **Modules without conventions are skipped.**

- **GroupRuleGenerator** (`rules/group.rs`): Cross-module group rules with member module table, cross-module constraints from hidden_dependencies, boundary rules.

- **BusinessDomainRuleGenerator** (`rules/business_domain.rs`): DDD bounded context rules. Generates from `Domain` structs with boundary rules, interfaces, groups-in-domain. Paths collected by traversing domain -> groups -> modules. Triggers from domain id/name/interfaces.

- **CrossCuttingRuleGenerator** (`rules/cross_cutting.rs`): Two-tier system:
  1. **Well-known concerns** (8 hardcoded): security, error-handling, concurrency, testing, performance, api, data, logging -- each with trigger keywords and specialized content generators.
  2. **Dynamic discovery** from constraints: `categorize_implicit_rule()` maps constraint descriptions to additional concerns (validation, observability, configuration, feature-flags, etc.). Evidence-based path scoping.

### 3.3 Custom Category Discovery

`discover_custom_categories()` in `rules/mod.rs` creates categories from:

1. **Tier3Category enum** mapping: `ConcurrencyTrap`, `ResourceLeak`, `StateInvariant`, `SecurityBoundary`, `PerformanceTrap` -- each maps to a custom rule category name.
2. **Pattern-based**: High-frequency patterns (>= 3 locations) with identifiable categories generate categories from pattern category names.
3. **Cross-constraint based**: `CrossModuleConstraint` types generate "cross-module-{type}" categories.
4. **Deduplication**: Category names are deduped against existing categories by string comparison.

**Limitation analysis:** The `Tier3Category` enum is a fixed set of 5 categories. While the system can generate custom categories from patterns and constraints, the initial seeding from `Tier3Category` represents a Rust-centric classification (ConcurrencyTrap, ResourceLeak). For diverse domains (e.g., financial compliance, healthcare data governance), the LLM-discovered custom categories from patterns/constraints provide the extensibility escape hatch, but the well-known concerns list (`WELL_KNOWN_CONCERNS`) is also heavily systems-programming oriented.

---

## 4. Skills Generation System

### 4.1 Architecture

`SkillsGenerator` (`skills/mod.rs`) has two paths:
- **Async LLM path** (`generate_with_llm`): Retries up to `MAX_RETRIES=3` with exponential backoff and negative feedback on failures.
- **Sync fallback** (`generate`): Template-based generation without LLM.

**Post-generation pipeline:**
1. `ExtendedSkillsGenerator` -- adds test/document/security-audit skills based on evidence
2. `SkillCrossReferencer` -- annotates related skills by shared tools/keywords
3. `ProgressiveDisclosure` -- splits content into main/reference/examples by value tier
4. `RuleCrossReferencer` -- adds `@.claude/rules/` references
5. `DynamicContextInjector` -- adds `!command` directives

### 4.2 LLM Skill Discovery

`SkillDiscoveryPrompt` (`skills/discovery.rs`) builds a comprehensive prompt including:
- Project summary (type, language, frameworks, file count, module count)
- Structural section (entry points, modules)
- AST facts (function stats, key types)
- Modules with dependencies and key files
- Insights (cross-module discoveries)
- Patterns (sorted by location count)
- Constraints (gotchas, hidden deps, anti-patterns)
- Domain knowledge (policies, logic, terms)
- Budget guidance
- Dynamic commands

The prompt requests `SkillSuggestion` JSON responses: `{name, description, tools, triggers, scope, body_outline}`. Each suggestion generates an individual skill via `generate_skill_from_suggestion` with full context.

**Agent inference from skills:** review -> reviewer, plan -> architect, write tools -> coder.

### 4.3 Progressive Disclosure

`ProgressiveDisclosure` (`skills/disclosure.rs`) implements a 3-tier value classification:

```
ValueTier::Critical (3) -- Keywords: security, auth, database, payment, config, deploy
ValueTier::High (2)     -- Keywords: test, error, validation, logging, performance
ValueTier::Normal (1)   -- Everything else
```

**Splitting logic:**
- Only applies when skill body exceeds `consideration_threshold` (500 lines)
- Sections categorized into main/reference/examples
- Minimum section size enforced (`min_section_size`)
- Extracted sections become `additional_files` on the Skill struct

### 4.4 Cross-Referencing

Three cross-referencers operate in sequence:

1. **RuleCrossReferencer**: Matches skill names/triggers against rule names/paths/categories to inject `@.claude/rules/{path}` references.
2. **SkillCrossReferencer**: Finds related skills by shared tool sets and keyword overlap. Adds `## Related Skills` section with recommended agent (reviewer/architect/coder).
3. **DynamicContextInjector**: Adds `!command` directives based on skill name and tech stack (e.g., `!cargo test` for Rust test skills, `!npm run lint` for TS lint skills).

### 4.5 Extended Skills

Evidence-conditional generation (`skills/extended.rs`):

| Skill | Required Evidence | Config |
|-------|------------------|--------|
| test | test_files + framework | Auto/Enabled/Disabled |
| document | docs_directory + readme | Auto/Enabled/Disabled |
| security-audit | auth_modules + crypto + database | Auto/Enabled/Disabled |

### 4.6 Monorepo Support

`MonorepoSkillsGenerator` (`skills/monorepo.rs`):
- Root-level cross-workspace skills (coordination, integration)
- Per-workspace scoped skills with `output_path` scoping
- Scoped `@import` with workspace path prefix
- Tool selection by skill type: review=read-only, build/test=+Bash, impl=full

### 4.7 Module-Level Skill Resolution

`ModuleSkillResolver` (`skills/resolver.rs`): Config-driven `tag_skill_map` matches module paths and responsibility text to available skills. Falls back to "implement" skill. Case-insensitive matching with deduplication.

---

## 5. Agent Generation System

### 5.1 Five-Layer Architecture

`AgentsGenerator` (`agents/mod.rs`) produces agents in 5 layers:

| Layer | Generator | Condition | Tool Set |
|-------|-----------|-----------|----------|
| 1. Base | `BaseAgentGenerator` | Always | Per-spec |
| 2. Module | `ModuleAgentGenerator` | value_score >= threshold, coverage >= min | full_access |
| 3. Domain | `DomainAgentGenerator` | domains.len() > 0 | read_only |
| 4. LLM | `DiscoveredAgentGenerator` | Optional (LLM enabled) | Per-discovery |
| 5. Service | `ServiceAgentGenerator` | services.len() > 0 | Per-service-type |

### 5.2 Base Agents

Three methodology templates (`agents/base.rs`):

| Agent | Color | Model | Tools | Mode | Veto | Skills |
|-------|-------|-------|-------|------|------|--------|
| reviewer | Blue | Sonnet | read_only | Default | Yes | code-review |
| coder | Green | Sonnet | full_access | AcceptEdits | No | implement, debug, refactor |
| architect | Purple | Sonnet | read_only | Plan | Yes | plan |

`BaseAgentSpec` is configurable with all fields. Context injection: analysis sections, agent-specific sections (coder -> domain context, architect -> core files), applicable rules, available skills.

### 5.3 Module Specialist Agents

`ModuleAgentGenerator` (`agents/module.rs`): Creates `{module-id}-specialist` for high-value modules. Full-access tools, AcceptEdits permission, skills resolved by `ModuleSkillResolver`. Prompt includes key files, evidence, critical insights, hidden dependencies.

### 5.4 Domain Expert Agents

`DomainAgentGenerator` (`agents/domain.rs`): Creates `{domain-id}-expert` for each detected `Domain`. Read-only tools, veto power, Purple color, plan+code-review skills. Prompt includes scope, boundary rules, domain-related insights and patterns.

### 5.5 LLM-Discovered Agents

`DiscoveredAgentGenerator` (`agents/discovery.rs`): Structured JSON schema response. `DiscoveredAgent` with evidence, scope, tools, skills, color, veto, priority, model, permission_mode. Discovery types: Module Specialists, Domain Experts, Integration Coordinators, Technology Specialists.

### 5.6 Service Specialist Agents

`ServiceAgentGenerator` (`agents/service.rs`): Creates `{service-id}-service` for detected services. `ServiceType`-specific config:

| ServiceType | Color | Guidelines |
|-------------|-------|------------|
| Api | Green | API contracts, versioning |
| Worker | Purple | Idempotency, retry |
| Gateway | Orange | Rate limiting, routing |
| Library | Blue | API stability, semver |
| Cli | Orange | UX, error messages |
| Web | Blue | Accessibility, performance |

### 5.7 Consensus System

`ConsensusRole` (`types/agent.rs:249-285`): priority-based with `can_veto` flag and `vote_threshold` (default 67%). Reviewer and architect get veto power by default.

### 5.8 Tool Sets

Four predefined tool sets (`agents/mod.rs`):
- `read_only`: Read, Glob, Grep, Bash (non-destructive)
- `full_access`: Read, Write, Edit, Glob, Grep, Bash, Skill
- `library`: Read, Glob, Grep, Bash, Write, Edit (no Skill)
- `write_tools`: Read, Write, Edit, Glob, Grep

---

## 6. GenerationContext

### 6.1 Unified Data Carrier

`GenerationContext` (`context/mod.rs:177-196`) aggregates all analysis results:

```
detection: &ProjectDetection
tech_stack: &TechStack
project_name: &str
modules: &[DetectedModule]
groups: &[ModuleGroup]
domains: &[Domain]
deep_analysis: Option<&DeepAnalysisResult>
synthesis: Option<&SynthesizedAnalysis>
domain_analysis: Option<&DomainAnalysisResult>
cross_insights: Option<&SynthesizedInsights>
conventions: &InferredConventions
constraints: &ExtractedConstraints
file_registry: &VerifiedFileRegistry
reference_pool: Option<VerifiedReferencePool>
budget: Option<BudgetedSections>
services: &[DetectedService]
generated_skill_names: Vec<String>
```

### 6.2 LLM-First Methods

All accessor methods return **unfiltered** data -- the LLM decides relevance:
- `module_summaries()`, `all_patterns()`, `all_discovered_insights()`
- `all_hidden_dependencies()`, `all_architecture_violations()`
- `all_cross_constraints()`, `domain_knowledge()`
- `enriched_domain_knowledge()`, `all_files_with_context()`

### 6.3 Budget System

Token budget planning via `plan_budget(max_tokens)` (`context/budget.rs`):
- **Tier1** (essential): project_identity, architecture, conventions
- **Tier2** (patterns): detected patterns, constraints, insights
- **Tier3** (domain): domain knowledge, workflows, terminology
- Sections allocated proportionally; Tier3 dropped when tight

### 6.4 Context Enricher

`ContextEnricher` (`context_enricher.rs`) provides 100% information preservation through aggregation:
- Structural context: entry points, modules, language distribution, key directories
- AST context: function stats, dominant patterns, key types/functions
- Confidence levels: High (>= 10 patterns + 5 constraints), Medium (>= 3 patterns or 2 constraints), Low, StructureOnly

### 6.5 Discovery Formatting

`DiscoveryFormat` (`discovery_fmt.rs`) provides shared formatting for agent and skill discovery prompts:
- `for_agents()`: includes domain count and value score
- `for_skills()`: sorts patterns by location count
- Common formatters: `format_project_summary`, `format_modules`, `format_patterns`, `format_insights`, `format_domain_knowledge`

---

## 7. CLAUDE.md Generation

### 7.1 Section-Level Caching

`ClaudeMdGenerator` (`claude_md/mod.rs`) implements differential updates via `ClaudeMdCache` and `SectionManifest`. Each section stores an input hash; only stale sections are regenerated.

### 7.2 Large Section Extraction

Sections exceeding thresholds are extracted to `.claude/docs/` with `@import` references:
- `ARCHITECTURE_MAX_LINES = 30`
- `STANDARDS_MAX_LINES = 50`
- `DOMAIN_MAX_LINES = 30`

### 7.3 Navigation Map

`NavigationMapGenerator` creates a module -> rules/agents/skills mapping table. Minimum 3 modules required to generate the map.

### 7.4 Priority-Based Import Ordering

Extracted imports are ordered by priority to ensure the most important context loads first.

---

## 8. Type System Analysis

### 8.1 Core Artifact Types

| Type | File | Key Fields |
|------|------|------------|
| `Rule` | `types/rule.rs` | name, paths, triggers, priority, category, always_inject, content, evidence, quality |
| `Skill` | `types/skill.rs` | name, description, allowed_tools, model, context, agent, body, evidence, quality |
| `Agent` | `types/agent.rs` | name, description, color, tools, model, permission_mode, skills, consensus, prompt, examples, evidence, quality |

### 8.2 Quality Metrics

`QualityMetrics` (`types/skill.rs:19-25`):
- `file_refs: usize` -- count of `@file:line` references (informational)
- `validity: ValidityState` -- binary Valid/Hallucinated (programmatic checks)

Per LLM-Trust principle: only `validity` is used for programmatic checks.

### 8.3 Artifact Categories

`ArtifactCategory` (`types/artifact_category.rs:24-29`):
- `Methodology` -- base agents (reviewer, coder, architect), min 0 evidence refs
- `ProjectSpecific` -- everything else, min 2 evidence refs

### 8.4 Domain Types

`DomainAnalysisResult` (`types/domain.rs`):
- Policies: `DomainPolicy` with `PolicyType` enum (8 variants: Validation, Authorization, BusinessRule, Invariant, StateTransition, DataIntegrity, RateLimiting, Audit)
- Logic: `CoreDomainLogic` with `DomainLogicType` enum (8 variants: Calculation, Transformation, Aggregation, Decision, Orchestration, Integration, Sanitization, EventHandling)
- Terminology: `DomainGlossary` with `TermCategory` enum (7 variants: Entity, Action, State, Metric, Role, Concept, Event)
- Workflows: `BusinessWorkflow` with `WorkflowStep`

### 8.5 Convention Types

`InferredConventions` (`types/conventions.rs`):
- Architecture: pattern name, layers, data flow
- Naming: file/type/function/module naming cases
- Patterns: `CodePattern` with `PatternCategory` enum (9 variants)
- File organization: `StructureType` enum (5 variants)
- Error handling: `ErrorStyle` enum (4 variants)
- Async: `AsyncStyle` enum (5 variants)
- Testing: framework, location, naming pattern

### 8.6 Hint System

`AnalysisHint` (`types/hint.rs`): Confidence-tagged hints from programmatic analysis.
- `HintConfidence` enum: RequiresValidation, Low, Medium, High, Definitive (ordered)
- `HintCategory` enum: 8 categories (Architecture, ErrorHandling, AsyncPattern, etc.)
- `HintCollection` groups hints by confidence for LLM prompt formatting

### 8.7 Insight Types

`Insight` (`pipeline/insight/types.rs`):
- `InsightCategory` enum: 14 variants spanning technical/business/security/domain
- `ConstraintType` enum: 13 variants including catch-all `Other` with `#[serde(other)]` for extensibility
- `InsightSource` enum: 5 sources (MistakeAnalysis, ConstraintDetection, DomainAnalysis, PatternMining, ManualAnnotation)

---

## 9. Enum Classification Flexibility Analysis

### 9.1 Fixed Enums and Their Impact

The system uses many fixed enums for classification. Here is an assessment of how each affects diverse domain support:

**High flexibility (catch-all or extensible):**
- `RuleCategory::Custom { name, priority, subdirectory }` -- fully extensible
- `ConstraintType::Other` with `#[serde(other)]` -- LLM can classify freely
- `InsightCategory` -- 14 variants covering most practical needs

**Medium flexibility (comprehensive but fixed):**
- `PolicyType` (8 variants) -- covers most policy types but misses domain-specific ones (e.g., GDPR consent, SOX compliance)
- `DomainLogicType` (8 variants) -- covers common logic types but misses domain-specific ones (e.g., MLOps model training, IoT telemetry processing)
- `TermCategory` (7 variants) -- reasonably complete for most domains

**Low flexibility (potentially limiting):**
- `Tier3Category` (5 fixed variants: ConcurrencyTrap, ResourceLeak, StateInvariant, SecurityBoundary, PerformanceTrap) -- strongly biased toward systems programming concerns. A financial services project would benefit from categories like ComplianceViolation, DataLineageGap, AuditTrailMissing.
- `WELL_KNOWN_CONCERNS` (8 fixed: security, error-handling, concurrency, testing, performance, api, data, logging) -- generic but misses domain concerns like compliance, accessibility, i18n, privacy.
- `KNOWN_FRAMEWORKS` (10 hardcoded) -- good coverage of popular frameworks but misses many (Spring Boot sub-frameworks, Flutter, Rails, ASP.NET, etc.).
- `ProjectCommands::from_detection()` -- hardcoded commands for 8 languages. Missing: Dart/Flutter, Elixir, Scala, C#/.NET, Swift.
- `StructuralEntryPoint` (4 variants: Main, LibRoot, ApiHandler, CliCommand) -- misses worker processes, scheduled jobs, serverless handlers, test runners.

### 9.2 Mitigation Strategies in Place

1. **LLM-first philosophy**: The system explicitly delegates classification decisions to the LLM. Fixed enums provide structure, but the LLM can override or supplement via free-text fields.

2. **Dynamic discovery**: Custom rule categories from patterns and constraints provide runtime extensibility beyond the fixed `Tier3Category` enum.

3. **Cross-cutting dynamic discovery**: `categorize_implicit_rule()` maps constraint descriptions to additional concern categories not in `WELL_KNOWN_CONCERNS`.

4. **Unknown framework handling**: `FrameworkRuleGenerator` generates rules for frameworks detected in `tech_stack` even if not in `KNOWN_FRAMEWORKS`.

5. **`ConstraintType::Other`**: The `#[serde(other)]` catch-all allows JSON deserialization of any constraint type the LLM produces.

### 9.3 Recommendations for Improved Flexibility

1. **`Tier3Category`**: Add a catch-all `Custom(String)` variant or convert to `String`-based classification to support domain-specific categories.

2. **`PolicyType` and `DomainLogicType`**: Add `Other(String)` variants to avoid force-fitting domain-specific types into generic categories.

3. **`KNOWN_FRAMEWORKS`**: Convert from const array to a configurable list, loaded from `claudegen.toml`, so users can add project-specific frameworks.

4. **`ProjectCommands`**: Add a user-configurable commands section in config that overrides or supplements the hardcoded language commands.

5. **`StructuralEntryPoint`**: Add variants for Worker, ServerlessHandler, ScheduledJob, TestRunner to better support diverse deployment patterns.

---

## 10. Evidence System

### 10.1 Evidence Gate

`EvidenceMetrics` (`evidence_gate.rs`) computes from `GenerationContext`:
- `verified_refs`: Count from reference pool
- `patterns`: Pattern count from deep analysis
- `constraints`: Constraint count
- `confidence`: Overall synthesis confidence

`has_evidence()` = verified_refs > 0 || patterns > 0 || constraints > 0

### 10.2 Evidence Profile

`EvidenceProfile` (`evidence_class.rs`):
- `verified_count`, `inferred_count`, `convention_count`
- `verification_ratio()` = verified / (verified + inferred + convention)

### 10.3 File Reference Validation

`FileRef` (`pipeline/file_reference.rs`): Validates `@path:line` references against `VerifiedFileRegistry`. Three depth levels:
- Level 0: file-only (`@src/main.rs`)
- Level 1: file+line (`@src/main.rs:42`)
- Level 2: file+range (`@src/main.rs:42-50`)

Canonical validation via `is_valid_file_ref()` checks file existence and line range.

---

## 11. Cross-Cutting Concerns

### 11.1 Orchestration Order

`Orchestrator` (`orchestration.rs`): Skills generated first, then agents. This order is critical because agents reference available skills in their prompts. `generated_skill_names` is populated on `GenerationContext` after skill generation.

### 11.2 Naming Conventions

All artifact names must be kebab-case, validated by `is_kebab_case()`:
- Skills: max 64 characters
- Agents: no length limit specified
- Rules: no length limit specified

Validation codes are consistently prefixed: `SKILL_*`, `AGENT_*`, `RULE_*`.

### 11.3 Serialization

- Skills: YAML frontmatter via `IndexMap` (preserves insertion order) + body
- Agents: YAML frontmatter via `IndexMap` + prompt (+ optional examples)
- Rules: Minimal frontmatter (paths only) + content
- Tools in skills: comma-separated string (Claude Code format)
- Tools in agents: YAML array

### 11.4 Deduplication

`GeneratedArtifacts` (`types/artifacts.rs`) validates uniqueness via `HashSet`:
- `DUPLICATE_SKILL_NAME`, `DUPLICATE_AGENT_NAME`, `DUPLICATE_RULE_NAME`

---

## 12. Bottom-Up / Top-Down Analysis Support

### 12.1 Bottom-Up (File -> Module -> Group -> Domain)

- File registry provides raw file inventory
- Module detection groups files into logical units
- Groups aggregate modules with shared concerns
- Domains provide bounded context boundaries
- Each level feeds the next in generation context

### 12.2 Top-Down (Project -> Architecture -> Conventions -> Rules)

- Project detection identifies type, languages, workspace config
- Architecture conventions infer layering, data flow
- Conventions guide rule generation (naming, error handling, async patterns)
- Rules materialize conventions as actionable guidance

### 12.3 Cross-Axis (Insights, Constraints, Domain Knowledge)

- Insights span both axes: found bottom-up but applied top-down
- Constraints discovered in analysis feed rule/skill content
- Domain knowledge enriches all artifact types
- Hidden dependencies create cross-module rules/agent specializations

---

## 13. Enterprise Scalability Assessment

### 13.1 Strengths

- **Monorepo support**: Workspace-scoped skills and agents with proper path prefixing
- **Budget-aware context**: Tiered allocation prevents context overflow for large codebases
- **Differential updates**: Section-level caching avoids regenerating unchanged content
- **5-layer agents**: Scales from small projects (base agents only) to large organizations (all 5 layers)
- **Service detection**: Microservice architectures get per-service agents and rules

### 13.2 Limitations

- **Single-pass generation**: No iterative refinement across artifact types (e.g., skills cannot trigger additional rule generation)
- **Linear orchestration**: Skills then agents -- no feedback loop between the two
- **No team awareness**: Agent generation doesn't account for team boundaries or ownership
- **Fixed tool vocabulary**: `is_valid_tool()` checks against a hardcoded list; custom MCP tools would trigger warnings
- **No version management**: No mechanism for artifact versioning or migration between generations

### 13.3 Memory Considerations

- `GenerationContext` holds references (`&'a`) to all analysis data, minimizing copying
- Budget system prevents unbounded token allocation
- `VerifiedFileRegistry` provides efficient file lookup without loading file contents

---

## 14. Key Design Patterns

### 14.1 Builder Pattern

Extensively used across all types: `Rule::tech()`, `Skill::new().with_tools().with_user_invocable()`, `Agent::new().with_color().with_model()`. Provides fluent APIs for constructing complex artifacts.

### 14.2 Strategy Pattern

Different generators for each rule category, agent layer, and skill type. Each generator has consistent interface but specialized logic.

### 14.3 Chain of Responsibility

Post-generation pipeline: ExtendedSkills -> CrossRef -> Disclosure -> RuleCrossRef -> DynamicContext. Each stage enriches artifacts without modifying core structure.

### 14.4 Data Carrier

`GenerationContext` acts as a passive data carrier, aggregating all analysis results without processing logic. Generators pull what they need.

### 14.5 Evidence-First

No artifact generation without evidence. `EvidenceMetrics`, `EvidenceProfile`, `QualityMetrics`, and `ValidityState` ensure traceability from generated content back to source code.

---

## 15. Files Analyzed

### Generation Core
- `src/pipeline/generation/mod.rs` -- Module declarations and re-exports
- `src/pipeline/generation/orchestration.rs` -- Skills-first orchestration
- `src/pipeline/generation/context_enricher.rs` -- Structural/AST context aggregation
- `src/pipeline/generation/discovery_fmt.rs` -- Shared discovery prompt formatting
- `src/pipeline/generation/evidence_gate.rs` -- Evidence metrics
- `src/pipeline/generation/evidence_class.rs` -- Evidence profiling

### Rules
- `src/pipeline/generation/rules/mod.rs` -- Hierarchical rule generation, custom category discovery
- `src/pipeline/generation/rules/project.rs` -- Project-level rules
- `src/pipeline/generation/rules/tech.rs` -- Language-specific rules
- `src/pipeline/generation/rules/framework.rs` -- Framework-specific rules
- `src/pipeline/generation/rules/module.rs` -- Module-specific rules
- `src/pipeline/generation/rules/group.rs` -- Cross-module group rules
- `src/pipeline/generation/rules/business_domain.rs` -- DDD bounded context rules
- `src/pipeline/generation/rules/cross_cutting.rs` -- Cross-cutting concern rules

### Skills
- `src/pipeline/generation/skills/mod.rs` -- Skills generator with LLM path
- `src/pipeline/generation/skills/discovery.rs` -- LLM skill discovery
- `src/pipeline/generation/skills/disclosure.rs` -- Progressive Disclosure
- `src/pipeline/generation/skills/extended.rs` -- Evidence-conditional skills
- `src/pipeline/generation/skills/monorepo.rs` -- Monorepo-aware skills
- `src/pipeline/generation/skills/prompt.rs` -- Skill prompt builder
- `src/pipeline/generation/skills/resolver.rs` -- Module-level skill resolution

### Agents
- `src/pipeline/generation/agents/mod.rs` -- 5-layer agent generation
- `src/pipeline/generation/agents/base.rs` -- Base agents
- `src/pipeline/generation/agents/discovery.rs` -- LLM agent discovery
- `src/pipeline/generation/agents/domain.rs` -- Domain expert agents
- `src/pipeline/generation/agents/module.rs` -- Module specialist agents
- `src/pipeline/generation/agents/service.rs` -- Service-specific agents

### Context and CLAUDE.md
- `src/pipeline/generation/context/mod.rs` -- Unified GenerationContext
- `src/pipeline/generation/claude_md/mod.rs` -- CLAUDE.md generation with caching

### Types
- `src/types/rule.rs` -- Rule and RuleCategory types
- `src/types/skill.rs` -- Skill, QualityMetrics, ContextMode types
- `src/types/agent.rs` -- Agent, ConsensusRole, PermissionMode types
- `src/types/domain.rs` -- Domain analysis types (policies, logic, glossary, workflows)
- `src/types/insight.rs` -- Insight classification types
- `src/types/artifacts.rs` -- GeneratedArtifacts container and validation
- `src/types/artifact_category.rs` -- Methodology vs ProjectSpecific categorization
- `src/types/conventions.rs` -- Convention types (architecture, naming, patterns)
- `src/types/detection.rs` -- Project detection types
- `src/types/hint.rs` -- Confidence-tagged analysis hints
- `src/types/hook.rs` -- Claude Code hook types
- `src/types/module_map.rs` -- Module, Domain, TechStack types

### Pipeline Support
- `src/pipeline/insight/mod.rs` -- Insight module exports
- `src/pipeline/insight/types.rs` -- Insight, Constraint, ConstraintType types
- `src/pipeline/file_reference.rs` -- File reference validation
