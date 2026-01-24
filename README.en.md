# claudegen

[![CI](https://github.com/junyeong-ai/claudegen/workflows/CI/badge.svg)](https://github.com/junyeong-ai/claudegen/actions)
[![Rust](https://img.shields.io/badge/rust-1.92.0%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)

> **English** | **[한국어](README.md)**

**Automatic Claude Code plugin generation from codebase analysis.** Based on official Claude Code plugin architecture.

---

## Why claudegen?

- **Official Plugin Format** — Follows official Claude Code structure
- **Auto Skills Generation** — Automatically generates project-specific skills
- **Memory Management** — Includes project rules and guidelines in CLAUDE.md
- **Extensible** — Custom agents and hooks support

---

## Output Structure (Official Claude Code Plugin)

```
project/
├── CLAUDE.md                          # Project memory (rules, guidelines)
├── .claudegen/                        # Internal data (graph, cache, config)
│   └── config.toml
└── {project-name}-plugin/             # Plugin directory
    ├── .claude-plugin/
    │   └── plugin.json                # Plugin manifest
    ├── skills/
    │   └── {skill-name}/
    │       └── SKILL.md               # Skill with YAML frontmatter
    └── agents/
        └── {agent-name}.md            # Agent definitions
```

---

## Quick Start

```bash
# Install
cargo install claudegen

# Initialize project and generate plugin
cd your-project
claudegen init
claudegen generate

# Check results
ls .claudegen/
```

---

## Key Features

### Plugin Generation
```bash
claudegen generate                    # Generate plugin
claudegen generate --dry-run          # Preview config only
```

### Code Analysis
```bash
claudegen analyze                     # Analyze code structure
claudegen query "src/main.rs"         # Query dependencies
claudegen validate                    # Verify output
```

### Management
```bash
claudegen init                        # Initialize project
claudegen status                      # Check status
claudegen clean --all                 # Clean data
claudegen config show                 # Show config
```

---

## Installation

### Cargo
```bash
cargo install claudegen
```

### Build from Source
```bash
git clone https://github.com/junyeong-ai/claudegen && cd claudegen
cargo build --release
```

**Requirements**: Rust 1.92.0+

---

## Skill Format (SKILL.md)

```yaml
---
name: skill-name
description: "This skill should be used when..."
allowed-tools: Read, Grep, Glob
model: sonnet
context: fork
user-invocable: true
---

Skill prompt body...
```

---

## Configuration

`.claudegen/config.toml`:
```toml
version = "2.0"
preset = "standard"  # quick | standard | thorough | exhaustive

[project]
name = "my-project"
type = "auto"

[analysis]
include = ["**/*"]
exclude = ["node_modules/**", "dist/**", ".git/**", "target/**"]

[llm]
provider = "claude-agent"
model = "claude-sonnet-4-5-20250929"
```

---

## Configuration Priority

```
1. Built-in defaults
2. Global config (~/.config/claudegen/config.toml)
3. Project config (.claudegen/config.toml)
4. Environment variables (CLAUDEGEN_*)
5. CLI arguments (highest priority)
```

---

## Troubleshooting

```bash
# Reset data
claudegen clean --all && claudegen init

# Check status
claudegen status

# Debug mode
RUST_LOG=debug claudegen generate
```

---

## Support

- [GitHub Issues](https://github.com/junyeong-ai/claudegen/issues)
- [Developer Guide](CLAUDE.md)

---

<div align="center">

**English** | **[한국어](README.md)**

Made with Rust

</div>
