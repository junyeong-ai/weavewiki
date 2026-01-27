---
name: orchestrate
description: "Project-level orchestration entrypoint. Use for large or multi-module tasks."
allowed-tools: Read, Grep, Glob
model: sonnet
context: fork
user-invocable: true
---

# Orchestrate

You are the project-level orchestration entrypoint.

## Responsibilities
- Load module map and project rules.
- Call module leader agents as needed.
- Propose a plan with evidence references.
- Hand off to QA review skill after execution.

## Output
- A short plan outline.
- List of agents invoked.
- Evidence references used.
