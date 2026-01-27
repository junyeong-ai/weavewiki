//! Few-Shot Reference Library
//!
//! Provides project-type-specific examples for LLM-based inference.

use crate::config::ProjectType;

/// Example project for few-shot learning
#[derive(Debug, Clone)]
pub struct FewShotExample {
    pub name: &'static str,
    pub description: &'static str,
    pub architecture: &'static str,
    pub patterns: &'static [&'static str],
    pub constraints: &'static [&'static str],
}

impl FewShotExample {
    pub fn to_context(&self) -> String {
        let patterns = self
            .patterns
            .iter()
            .map(|p| format!("- {}", p))
            .collect::<Vec<_>>()
            .join("\n");

        let constraints = self
            .constraints
            .iter()
            .map(|c| format!("- {}", c))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"### {} ({})

**Architecture:**
{}

**Patterns:**
{}

**Constraints:**
{}"#,
            self.name, self.description, self.architecture, patterns, constraints
        )
    }
}

/// Get examples for a project type
pub fn get_examples(project_type: ProjectType) -> Vec<FewShotExample> {
    match project_type {
        ProjectType::Cli => vec![CLI_EXAMPLE],
        ProjectType::Library => vec![LIBRARY_EXAMPLE],
        ProjectType::Backend => vec![BACKEND_EXAMPLE],
        ProjectType::Frontend => vec![FRONTEND_EXAMPLE],
        ProjectType::Monorepo => vec![MONOREPO_EXAMPLE],
        ProjectType::Agent => vec![AGENT_EXAMPLE],
        ProjectType::Hybrid | ProjectType::Auto => {
            vec![CLI_EXAMPLE, BACKEND_EXAMPLE, FRONTEND_EXAMPLE]
        }
    }
}

/// Get CLAUDE.md example for a project type
pub fn get_claude_md_example(project_type: ProjectType) -> &'static str {
    match project_type {
        ProjectType::Cli => CLI_CLAUDE_MD,
        ProjectType::Library => LIBRARY_CLAUDE_MD,
        ProjectType::Backend => BACKEND_CLAUDE_MD,
        ProjectType::Frontend => FRONTEND_CLAUDE_MD,
        ProjectType::Monorepo => MONOREPO_CLAUDE_MD,
        ProjectType::Agent => AGENT_CLAUDE_MD,
        _ => GENERIC_CLAUDE_MD,
    }
}

/// Get skill example for a project type
pub fn get_skill_example(project_type: ProjectType) -> &'static str {
    match project_type {
        ProjectType::Cli => CLI_SKILL,
        ProjectType::Backend => BACKEND_SKILL,
        ProjectType::Frontend => FRONTEND_SKILL,
        _ => GENERIC_SKILL,
    }
}

/// Get rule example for a project type
pub fn get_rule_example(project_type: ProjectType) -> &'static str {
    match project_type {
        ProjectType::Cli => CLI_RULE,
        ProjectType::Backend => BACKEND_RULE,
        ProjectType::Frontend => FRONTEND_RULE,
        _ => GENERIC_RULE,
    }
}

// ============================================================================
// Few-Shot Examples
// ============================================================================

const CLI_EXAMPLE: FewShotExample = FewShotExample {
    name: "symora",
    description: "LSP-based code navigator",
    architecture: "Command → Service → LSP Backend",
    patterns: &[
        "Commands: `app <subcommand> [options]`",
        "Output: `--json` for machine-readable",
        "Config: TOML + env override",
    ],
    constraints: &[
        "No direct stdout - use OutputFormatter",
        "No direct service creation - use DI",
    ],
};

const LIBRARY_EXAMPLE: FewShotExample = FewShotExample {
    name: "semantic-search",
    description: "Search library with CLI wrapper",
    architecture: "Library-first (lib.rs exports, main.rs thin wrapper)",
    patterns: &[
        "Dual interface: Library + CLI",
        "Feature flags for optional CLI",
        "Explicit re-exports only",
    ],
    constraints: &[
        "Public API changes need CHANGELOG",
        "Breaking changes need major version",
    ],
};

const BACKEND_EXAMPLE: FewShotExample = FewShotExample {
    name: "web-api",
    description: "Backend service",
    architecture: "Hexagonal (Ports & Adapters)",
    patterns: &[
        "Inbound: web controllers",
        "Outbound: repository interfaces",
        "Domain: pure business logic",
    ],
    constraints: &[
        "No infrastructure in domain",
        "Transaction at use case level",
    ],
};

const FRONTEND_EXAMPLE: FewShotExample = FewShotExample {
    name: "web-app",
    description: "React application",
    architecture: "Feature-based components",
    patterns: &[
        "API: auto-generated clients",
        "State: TanStack Query + Context",
        "Styling: TailwindCSS",
    ],
    constraints: &["No manual API modifications", "No any types - strict mode"],
};

const MONOREPO_EXAMPLE: FewShotExample = FewShotExample {
    name: "enterprise",
    description: "Multi-project workspace",
    architecture: "services/ + packages/ + apps/",
    patterns: &[
        "Shared config at root",
        "Cross-project dependencies",
        "Affected-only CI/CD",
    ],
    constraints: &[
        "Shared changes affect consumers",
        "API changes need regeneration",
    ],
};

const AGENT_EXAMPLE: FewShotExample = FewShotExample {
    name: "mcp-agent",
    description: "AI agent with tools",
    architecture: "tools/ + context/ + prompts/",
    patterns: &[
        "JSON Schema tool definitions",
        "Token budget management",
        "Template-based prompts",
    ],
    constraints: &["No hardcoded prompts", "Tools need schema validation"],
};

// ============================================================================
// CLAUDE.md Templates
// ============================================================================

const CLI_CLAUDE_MD: &str = r#"# Project Overview

{name} is a cli project written in {language}.

## Architecture

**Pattern**: {architecture}

{description}

- `src/cli/` - Command definitions
- `src/{domain}/` - Business logic

## Code Standards

- Follow {architecture} architecture pattern
- ✗ Direct stdout in library code: Use logging or return data for CLI layer
- ✗ Direct service creation: Use dependency injection
- ⚠️ CLI argument changes require version consideration
"#;

const LIBRARY_CLAUDE_MD: &str = r#"# Project Overview

{name} is a library written in {language}.

## Architecture

**Pattern**: Library-first

Public API exports in `src/lib.rs`. Only re-exported items are public.

- `src/lib.rs` - Public API
- `src/core/` - Core functionality

## Code Standards

- Breaking changes require major version bump
- New features behind feature flags first
- CHANGELOG.md updated for API changes
"#;

const BACKEND_CLAUDE_MD: &str = r#"# Project Overview

{name} is a backend service.

## Architecture

**Pattern**: Hexagonal (Ports & Adapters)

- `adapter/inbound/` - HTTP controllers
- `port/inbound/` - Use case interfaces
- `port/outbound/` - Repository interfaces
- `domain/` - Pure business logic

## Code Standards

- Business logic in domain layer only
- Controllers delegate to use cases
- Database access through repository interfaces
"#;

const FRONTEND_CLAUDE_MD: &str = r#"# Project Overview

{name} is a frontend application.

## Architecture

**Pattern**: Feature-based

- `src/components/` - UI components
- `src/pages/` - Route components
- `src/hooks/` - Custom hooks
- `src/services/` - API clients

## Code Standards

- Server state: TanStack Query
- API clients: auto-generated (no manual edits)
- Styling: TailwindCSS
"#;

const MONOREPO_CLAUDE_MD: &str = r#"# Project Overview

{name} is a monorepo workspace.

## Architecture

**Pattern**: Multi-project workspace

- `services/` - Backend services
- `packages/` - Shared libraries
- `apps/` - Frontend applications

## Code Standards

- Shared package changes affect all consumers
- API changes require frontend regeneration
- Use workspace-level commands
"#;

const AGENT_CLAUDE_MD: &str = r#"# Project Overview

{name} is an AI agent.

## Architecture

**Pattern**: Tool-based Agent

- `src/tools/` - Tool definitions
- `src/context/` - Context management
- `src/prompts/` - System prompts

## Code Standards

- Tools must have JSON Schema definitions
- Token budget management required
- Use template system for prompts
"#;

const GENERIC_CLAUDE_MD: &str = r#"# Project Overview

{name} is a {type} project written in {language}.

## Architecture

**Pattern**: {architecture}

{description}

## Code Standards

- Follow project conventions
- Maintain consistency with existing patterns
"#;

// ============================================================================
// Skill Templates
// ============================================================================

const CLI_SKILL: &str = r#"---
name: add-subcommand
description: Add new CLI subcommand
---
## Steps
1. Create `src/cli/commands/{name}.rs`
2. Add variant to Commands enum
3. Add matching in main dispatch
4. Create integration test

## Gotchas
- Output through formatter for --json support
- Load config if command depends on it
"#;

const BACKEND_SKILL: &str = r#"---
name: add-domain-module
description: Add new domain module (Hexagonal)
---
## Steps
1. Create `domain/` - entities, value objects
2. Create `port/inbound/` - use case interfaces
3. Create `port/outbound/` - repository interfaces
4. Create `adapter/inbound/web/` - controllers
5. Create `adapter/outbound/persistence/` - DB impl

## Gotchas
- Follow existing module patterns
- Transaction at use case level
"#;

const FRONTEND_SKILL: &str = r#"---
name: add-page-component
description: Add new page with routing
---
## Steps
1. Create `src/pages/{Name}Page.tsx`
2. Add route to `src/routes.tsx`
3. Regenerate API if needed
4. Create test file

## Gotchas
- Use generated API clients only
- Server state via TanStack Query
"#;

const GENERIC_SKILL: &str = r#"---
name: example-skill
description: Example skill
---
## Steps
1. First step
2. Second step

## Notes
- Important consideration
"#;

// ============================================================================
// Rule Templates
// ============================================================================

const CLI_RULE: &str = r#"---
paths: ["src/cli/**/*"]
---
# CLI Output Pattern

## Rule
All CLI output must go through OutputFormatter.

## Why
- Ensures --json flag support
- Consistent output style
- Machine-readable output

## Examples
❌ `println!("Result: {}", value);`
✅ `formatter.output(&result)?;`
"#;

const BACKEND_RULE: &str = r#"---
paths: ["src/domain/**/*"]
---
# Domain Layer Purity

## Rule
Domain layer must not have infrastructure dependencies.

## Why
- Keeps business logic testable
- Enables infrastructure swapping
- Maintains hexagonal architecture

## Examples
❌ Import database/HTTP libraries in domain
✅ Define interfaces in port/, implement in adapter/
"#;

const FRONTEND_RULE: &str = r#"---
paths: ["src/**/*.tsx", "src/**/*.ts"]
---
# API Client Usage

## Rule
Use only auto-generated API clients. Never modify generated files.

## Why
- API clients stay in sync with backend
- Type safety guaranteed
- Reduces manual errors

## Examples
❌ `fetch('/api/users')`
✅ `useGetUsers()` (generated hook)
"#;

const GENERIC_RULE: &str = r#"---
paths: ["src/**/*"]
---
# Rule Name

## Rule
Description of the rule.

## Why
Reason for the rule.

## Examples
❌ Bad example
✅ Good example
"#;

// ============================================================================
// Inference Prompts
// ============================================================================

/// Build a prompt for convention inference from project structure
pub fn build_inference_prompt(
    project_type: crate::config::ProjectType,
    project_structure: &str,
    sample_files: &[(String, String)],
    max_samples: usize,
) -> String {
    let examples = get_examples(project_type);
    let examples_text = examples
        .iter()
        .map(|e| e.to_context())
        .collect::<Vec<_>>()
        .join("\n\n");

    let samples_text = sample_files
        .iter()
        .take(max_samples)
        .map(|(path, content)| {
            let preview = if content.len() > 500 {
                // Find valid UTF-8 char boundary to avoid panic on multi-byte chars
                let mut end = 500;
                while end > 0 && !content.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...", &content[..end])
            } else {
                content.clone()
            };
            format!("### {}\n```\n{}\n```", path, preview)
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    format!(
        r#"Analyze this project and infer its conventions.

## Reference Examples
{examples_text}

## Project Structure
```
{project_structure}
```

## Sample Files
{samples_text}

## Instructions

Analyze the ACTUAL code structure, not generic patterns. Each inference must reference real files from the project.

Return JSON with:
- `architecture`: pattern_name, description, layers (path_pattern + responsibility)
- `patterns`: name, description, example_file (must exist in structure)
- `layers`: path, role

IMPORTANT:
- Be specific to THIS project
- All file references must exist in the project structure
- Identify actual patterns, not hypothetical ones"#,
        examples_text = examples_text,
        project_structure = project_structure,
        samples_text = samples_text,
    )
}
