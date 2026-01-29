//! Fixed Skills Generator
//!
//! Generates 5 fixed skills that define operational methodologies.
//! Skills reference rules (auto-injected) for project-specific knowledge.
//!
//! Skills:
//! - code-review: Systematic code review methodology
//! - implement: Feature implementation with context awareness
//! - plan: Implementation planning with architecture awareness
//! - debug: Systematic debugging workflow
//! - refactor: Code refactoring with convention preservation

use crate::types::Skill;

pub struct FixedSkillsGenerator;

impl FixedSkillsGenerator {
    pub fn generate() -> Vec<Skill> {
        vec![
            Self::code_review(),
            Self::implement(),
            Self::plan(),
            Self::debug(),
            Self::refactor(),
        ]
    }

    fn code_review() -> Skill {
        Skill::new(
            "code-review",
            "Systematic code review with auto-injected project context",
            r#"# Code Review

## Overview

Code review **methodology**. Project/module-specific **domain knowledge** is auto-injected from rules.

## Process

### 1. Context Gathering
File paths trigger automatic rule injection:
- rules/project.md (always)
- rules/tech/{lang}.md (by extension)
- rules/modules/{mod}.md (by path)
- rules/domains/*.md (by keyword trigger)

### 2. Convention Check
Check against injected rules' conventions sections:
- [ ] Project-wide conventions
- [ ] Language-specific conventions
- [ ] Module-specific conventions

### 3. Known Issue Scan
Check against injected rules' known_issues sections:
- [ ] Existing issue pattern recurrence
- [ ] Prevention guideline compliance

### 4. Domain Rules
Apply triggered domain rules:
- [ ] Security (if applicable)
- [ ] Error handling (if applicable)
- [ ] Concurrency (if applicable)

### 5. Architecture
- [ ] Dependency direction (layer rules)
- [ ] Group boundaries
- [ ] Public API change impact

## Output Format

```
PASS
```
or
```
ISSUES

[SEVERITY] file:line - description
  Rule: {violated rule path}
  Fix: {suggested fix}
```

## Rule Integration

This skill **references** rules (no duplication).
Automatically reflects rule updates."#,
        )
        .with_tools(vec![
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
        ])
        .with_user_invocable(true)
    }

    fn implement() -> Skill {
        Skill::new(
            "implement",
            "Feature implementation with project context awareness",
            r#"# Implementation

## Process

### 1. Understand
- Analyze requirements
- Identify affected modules
- Check related rules (auto-injected)

### 2. Locate
- Search for files to modify
- Check existing patterns (reference rules)
- Reference similar implementations

### 3. Implement
- Follow module conventions
- Follow language idioms
- Follow error handling patterns

### 4. Verify
- Pass existing tests
- Add new tests (if needed)
- Confirm known issue avoidance

## Constraints

- **Minimal Changes**: Only what's requested
- **No Over-Engineering**: Only what's needed
- **Evidence-Based**: State rationale for changes
- **Test Coverage**: Test new code

## Output

1. List of changed files
2. Summary of changes
3. Test execution results"#,
        )
        .with_tools(vec![
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
            "Edit".into(),
            "Write".into(),
            "Bash".into(),
        ])
        .with_user_invocable(true)
        .with_argument_hint("<feature-description>")
    }

    fn plan() -> Skill {
        Skill::new(
            "plan",
            "Implementation planning with architecture awareness",
            r#"# Planning

## Process

### 1. Scope Analysis
- Decompose requirements
- Identify impact scope
- Analyze dependencies

### 2. Architecture Review
- Check existing patterns (reference rules)
- Select appropriate modules
- Design interfaces

### 3. Task Breakdown
- Separate implementation units
- Determine dependency order
- Identify risks

### 4. Plan Output
- Step-by-step task list
- Verification criteria for each step
- Expected affected files

## Output Format

```markdown
## Plan: {title}

### Affected Modules
- {module}: {impact}

### Tasks
1. [ ] {task} - {files}
2. [ ] {task} - {files}

### Risks
- {risk}: {mitigation}

### Verification
- {how to verify completion}
```"#,
        )
        .with_tools(vec![
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
        ])
        .with_user_invocable(true)
        .with_argument_hint("<feature-or-task-description>")
    }

    fn debug() -> Skill {
        Skill::new(
            "debug",
            "Systematic debugging with codebase context",
            r#"# Debugging

## Process

### 1. Reproduce
- Confirm symptoms
- Identify reproduction conditions
- Collect related logs

### 2. Localize
- Trace error location
- Analyze call stack
- Search related code

### 3. Analyze
- Check known issues (reference rules)
- Search similar patterns
- Root cause analysis

### 4. Fix & Verify
- Apply fix
- Reproduction test
- Regression test

## Output

1. Root cause analysis
2. Fix details
3. Verification results
4. Prevention measures (suggest known_issues addition)"#,
        )
        .with_tools(vec![
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
            "Bash".into(),
        ])
        .with_user_invocable(true)
        .with_argument_hint("<error-or-symptom-description>")
    }

    fn refactor() -> Skill {
        Skill::new(
            "refactor",
            "Code refactoring with convention preservation",
            r#"# Refactoring

## Process

### 1. Assess
- Analyze current state
- Define target state
- Identify impact scope

### 2. Plan
- Separate refactoring stages
- Verification criteria per stage
- Rollback strategy

### 3. Execute
- Execute step by step
- Test each step
- Confirm convention compliance

### 4. Verify
- Pass all tests
- Confirm behavioral equivalence
- Check performance impact

## Constraints

- **Behavior Preservation**: No behavior changes
- **Incremental**: Separate into small steps
- **Testable**: Each step verifiable
- **Reversible**: Can rollback

## Output

1. Change summary
2. Test results
3. Before/After comparison"#,
        )
        .with_tools(vec![
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
            "Edit".into(),
            "Write".into(),
            "Bash".into(),
        ])
        .with_user_invocable(true)
        .with_argument_hint("<refactoring-goal>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generates_five_skills() {
        let skills = FixedSkillsGenerator::generate();
        assert_eq!(skills.len(), 5);
    }

    #[test]
    fn test_skill_names() {
        let skills = FixedSkillsGenerator::generate();
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"code-review"));
        assert!(names.contains(&"implement"));
        assert!(names.contains(&"plan"));
        assert!(names.contains(&"debug"));
        assert!(names.contains(&"refactor"));
    }

    #[test]
    fn test_all_skills_user_invocable() {
        let skills = FixedSkillsGenerator::generate();
        for skill in &skills {
            assert_eq!(
                skill.user_invocable,
                Some(true),
                "{} should be user-invocable",
                skill.name
            );
        }
    }

    #[test]
    fn test_code_review_is_read_only() {
        let skills = FixedSkillsGenerator::generate();
        let code_review = skills.iter().find(|s| s.name == "code-review").unwrap();
        let tools = code_review.allowed_tools.as_ref().unwrap();
        assert!(!tools.contains(&"Edit".to_string()));
        assert!(!tools.contains(&"Write".to_string()));
        assert!(!tools.contains(&"Bash".to_string()));
    }

    #[test]
    fn test_implement_has_edit_tools() {
        let skills = FixedSkillsGenerator::generate();
        let implement = skills.iter().find(|s| s.name == "implement").unwrap();
        let tools = implement.allowed_tools.as_ref().unwrap();
        assert!(tools.contains(&"Edit".to_string()));
        assert!(tools.contains(&"Write".to_string()));
        assert!(tools.contains(&"Bash".to_string()));
    }

    #[test]
    fn test_argument_hints() {
        let skills = FixedSkillsGenerator::generate();

        let code_review = skills.iter().find(|s| s.name == "code-review").unwrap();
        assert!(code_review.argument_hint.is_none());

        let implement = skills.iter().find(|s| s.name == "implement").unwrap();
        assert!(implement.argument_hint.is_some());

        let plan = skills.iter().find(|s| s.name == "plan").unwrap();
        assert!(plan.argument_hint.is_some());

        let debug = skills.iter().find(|s| s.name == "debug").unwrap();
        assert!(debug.argument_hint.is_some());

        let refactor = skills.iter().find(|s| s.name == "refactor").unwrap();
        assert!(refactor.argument_hint.is_some());
    }

    #[test]
    fn test_skills_validation() {
        let skills = FixedSkillsGenerator::generate();
        for skill in &skills {
            let issues = skill.validate();
            let errors: Vec<_> = issues.iter().filter(|i| i.is_error()).collect();
            assert!(
                errors.is_empty(),
                "Skill {} has validation errors: {:?}",
                skill.name,
                errors
            );
        }
    }
}
