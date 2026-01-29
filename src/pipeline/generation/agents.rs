//! Fixed Agents Generator
//!
//! Generates 3 fixed agents that define operational roles.
//! Agents use skills and reference rules (auto-injected) for project context.
//!
//! Agents:
//! - reviewer: Code quality gatekeeper (read-only)
//! - coder: Feature implementation specialist
//! - architect: System design and planning specialist

use crate::types::agent::{Agent, AgentColor, AgentModel, ConsensusRole, PermissionMode};

pub struct FixedAgentsGenerator;

impl FixedAgentsGenerator {
    pub fn generate() -> Vec<Agent> {
        vec![Self::reviewer(), Self::coder(), Self::architect()]
    }

    fn reviewer() -> Agent {
        Agent::new(
            "reviewer",
            "Code quality gatekeeper",
            r#"# Reviewer Agent

## Role

Code quality gatekeeper.
Validates correctness, security, and convention compliance of changes.

## Perspective

- Quality first
- Conservative judgment (when in doubt, raise ISSUES)
- Specific feedback (file:line required)

## Workflow

1. Receive files for review
2. Rules auto-injected based on file paths
3. Execute `code-review` skill
4. Output PASS/ISSUES

## Constraints

- **Read-Only**: No file modifications
- **Evidence-Based**: All issues require evidence
- **Rule-Referenced**: Cite violated rule

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

## Consensus Role

- Priority: 70 (quality gate)
- Veto: On CRITICAL issues"#,
        )
        .with_color(AgentColor::Blue)
        .with_model(AgentModel::Sonnet)
        .with_tools(vec![
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
        ])
        .with_skills(vec!["code-review".into()])
        .with_consensus(ConsensusRole::new(70).with_veto())
        .with_permission_mode(PermissionMode::Default)
    }

    fn coder() -> Agent {
        Agent::new(
            "coder",
            "Feature implementation specialist",
            r#"# Coder Agent

## Role

Feature implementation specialist.
Transforms requirements into working code.

## Perspective

- Practical implementation
- Follow existing patterns
- Testable code

## Workflow

1. Receive implementation request
2. Rules auto-injected for context
3. Execute appropriate skill (implement/debug/refactor)
4. Verify with tests

## Constraints

- **Module Boundaries**: Work within module responsibility
- **Convention Compliance**: Follow rules
- **Minimal Footprint**: Only necessary changes
- **Test Coverage**: Test new code

## Output

1. List of changed files
2. Summary of changes
3. Test execution results

## Consensus Role

- Priority: 50 (implementation perspective)
- Feasibility feedback"#,
        )
        .with_color(AgentColor::Green)
        .with_model(AgentModel::Sonnet)
        .with_tools(vec![
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
            "Edit".into(),
            "Write".into(),
            "Bash".into(),
        ])
        .with_skills(vec![
            "implement".into(),
            "debug".into(),
            "refactor".into(),
        ])
        .with_consensus(ConsensusRole::new(50))
        .with_permission_mode(PermissionMode::AcceptEdits)
    }

    fn architect() -> Agent {
        Agent::new(
            "architect",
            "System design and planning specialist",
            r#"# Architect Agent

## Role

System design specialist.
Designs features to align with architecture.

## Perspective

- Long-term view
- Consistency focus
- Extensibility consideration

## Workflow

1. Receive requirements
2. Check architecture rules (rules/project.md, rules/groups/*.md)
3. Execute `plan` skill
4. Output implementation plan

## Constraints

- **Architecture Alignment**: Consistent with existing architecture
- **Minimal Complexity**: Only necessary complexity
- **Future-Proof**: Extensible design

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
```

## Consensus Role

- Priority: 60 (design perspective)
- Reject on architecture violations"#,
        )
        .with_color(AgentColor::Purple)
        .with_model(AgentModel::Sonnet)
        .with_tools(vec![
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
        ])
        .with_skills(vec!["plan".into()])
        .with_consensus(ConsensusRole::new(60).with_veto())
        .with_permission_mode(PermissionMode::Default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generates_three_agents() {
        let agents = FixedAgentsGenerator::generate();
        assert_eq!(agents.len(), 3);
    }

    #[test]
    fn test_agent_names() {
        let agents = FixedAgentsGenerator::generate();
        let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"reviewer"));
        assert!(names.contains(&"coder"));
        assert!(names.contains(&"architect"));
    }

    #[test]
    fn test_reviewer_is_read_only() {
        let agents = FixedAgentsGenerator::generate();
        let reviewer = agents.iter().find(|a| a.name == "reviewer").unwrap();
        let tools = reviewer.tools.as_ref().unwrap();
        assert!(!tools.contains(&"Edit".to_string()));
        assert!(!tools.contains(&"Write".to_string()));
        assert!(!tools.contains(&"Bash".to_string()));
    }

    #[test]
    fn test_coder_has_edit_tools() {
        let agents = FixedAgentsGenerator::generate();
        let coder = agents.iter().find(|a| a.name == "coder").unwrap();
        let tools = coder.tools.as_ref().unwrap();
        assert!(tools.contains(&"Edit".to_string()));
        assert!(tools.contains(&"Write".to_string()));
        assert!(tools.contains(&"Bash".to_string()));
    }

    #[test]
    fn test_architect_is_read_only() {
        let agents = FixedAgentsGenerator::generate();
        let architect = agents.iter().find(|a| a.name == "architect").unwrap();
        let tools = architect.tools.as_ref().unwrap();
        assert!(!tools.contains(&"Edit".to_string()));
        assert!(!tools.contains(&"Write".to_string()));
        assert!(!tools.contains(&"Bash".to_string()));
    }

    #[test]
    fn test_agent_skills() {
        let agents = FixedAgentsGenerator::generate();

        let reviewer = agents.iter().find(|a| a.name == "reviewer").unwrap();
        assert_eq!(
            reviewer.skills.as_ref().unwrap(),
            &vec!["code-review".to_string()]
        );

        let coder = agents.iter().find(|a| a.name == "coder").unwrap();
        let coder_skills = coder.skills.as_ref().unwrap();
        assert!(coder_skills.contains(&"implement".to_string()));
        assert!(coder_skills.contains(&"debug".to_string()));
        assert!(coder_skills.contains(&"refactor".to_string()));

        let architect = agents.iter().find(|a| a.name == "architect").unwrap();
        assert_eq!(
            architect.skills.as_ref().unwrap(),
            &vec!["plan".to_string()]
        );
    }

    #[test]
    fn test_coder_permission_mode() {
        let agents = FixedAgentsGenerator::generate();
        let coder = agents.iter().find(|a| a.name == "coder").unwrap();
        assert_eq!(coder.permission_mode, Some(PermissionMode::AcceptEdits));
    }

    #[test]
    fn test_agents_have_colors() {
        let agents = FixedAgentsGenerator::generate();
        for agent in &agents {
            assert!(
                agent.color.is_some(),
                "Agent {} should have a color",
                agent.name
            );
        }
    }

    #[test]
    fn test_agents_validation() {
        let agents = FixedAgentsGenerator::generate();
        for agent in &agents {
            let issues = agent.validate();
            let errors: Vec<_> = issues.iter().filter(|i| i.is_error()).collect();
            assert!(
                errors.is_empty(),
                "Agent {} has validation errors: {:?}",
                agent.name,
                errors
            );
        }
    }

    #[test]
    fn test_agents_have_consensus() {
        let agents = FixedAgentsGenerator::generate();
        for agent in &agents {
            assert!(
                agent.consensus.is_some(),
                "Agent {} should have consensus role",
                agent.name
            );
        }
    }

    #[test]
    fn test_reviewer_can_veto() {
        let agents = FixedAgentsGenerator::generate();
        let reviewer = agents.iter().find(|a| a.name == "reviewer").unwrap();
        let consensus = reviewer.consensus.as_ref().unwrap();
        assert!(consensus.can_veto, "Reviewer should have veto power");
        assert_eq!(consensus.priority, 70);
    }

    #[test]
    fn test_coder_no_veto() {
        let agents = FixedAgentsGenerator::generate();
        let coder = agents.iter().find(|a| a.name == "coder").unwrap();
        let consensus = coder.consensus.as_ref().unwrap();
        assert!(!consensus.can_veto, "Coder should not have veto power");
        assert_eq!(consensus.priority, 50);
    }

    #[test]
    fn test_architect_can_veto() {
        let agents = FixedAgentsGenerator::generate();
        let architect = agents.iter().find(|a| a.name == "architect").unwrap();
        let consensus = architect.consensus.as_ref().unwrap();
        assert!(consensus.can_veto, "Architect should have veto power");
        assert_eq!(consensus.priority, 60);
    }
}
