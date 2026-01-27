---
name: qa-review
description: "Multi-turn QA review for changes and plans."
allowed-tools: Read, Grep, Glob
model: sonnet
context: fork
user-invocable: true
---

# QA Review

Review the changes or plan for correctness, risks, and regressions.

## Output
- Pass/fail judgment.
- Issues found with evidence references.
- Required changes (if any).
