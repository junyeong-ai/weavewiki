pub struct HookScriptGenerator;

impl HookScriptGenerator {
    pub fn generate_validate_module_scope() -> String {
        r#"#!/usr/bin/env bash
set -euo pipefail

MODULE_ID="${1:?Usage: validate-module-scope.sh <module-id>}"
MODULE_MAP="$CLAUDE_PROJECT_DIR/.claudegen/module_map.json"

# Read file_path from Claude Code JSON stdin
INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')
[ -z "$FILE_PATH" ] && exit 0
[ ! -f "$MODULE_MAP" ] && exit 0

MODULE_PATHS=$(jq -r --arg id "$MODULE_ID" \
    '.modules[] | select(.module_id == $id) | .paths[]' \
    "$MODULE_MAP" 2>/dev/null)
[ -z "$MODULE_PATHS" ] && exit 0

while IFS= read -r MOD_PATH; do
    [[ "$FILE_PATH" == *"$MOD_PATH"* ]] && exit 0
done <<< "$MODULE_PATHS"

echo "SCOPE VIOLATION: $FILE_PATH is outside module '$MODULE_ID' scope" >&2
exit 2
"#
        .to_string()
    }

    pub fn generate_run_module_tests() -> String {
        r#"#!/usr/bin/env bash
set -euo pipefail

MODULE_ID="${1:?Usage: run-module-tests.sh <module-id>}"
MODULE_MAP="$CLAUDE_PROJECT_DIR/.claudegen/module_map.json"

# Read file_path from Claude Code JSON stdin
INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')
[ -z "$FILE_PATH" ] && exit 0
[ ! -f "$MODULE_MAP" ] && exit 0

MODULE_PATHS=$(jq -r --arg id "$MODULE_ID" \
    '.modules[] | select(.module_id == $id) | .paths[]' \
    "$MODULE_MAP" 2>/dev/null)
[ -z "$MODULE_PATHS" ] && exit 0

# Detect project type and run appropriate tests
if [ -f "Cargo.toml" ]; then
    while IFS= read -r MOD_PATH; do
        cargo test --lib -- "${MOD_PATH//\//::#}" 2>/dev/null || true
    done <<< "$MODULE_PATHS"
elif [ -f "package.json" ]; then
    while IFS= read -r MOD_PATH; do
        npx jest --testPathPattern="$MOD_PATH" 2>/dev/null || \
        npx vitest run "$MOD_PATH" 2>/dev/null || true
    done <<< "$MODULE_PATHS"
elif [ -f "pyproject.toml" ] || [ -f "setup.py" ]; then
    while IFS= read -r MOD_PATH; do
        python -m pytest "$MOD_PATH" 2>/dev/null || true
    done <<< "$MODULE_PATHS"
elif [ -f "go.mod" ]; then
    while IFS= read -r MOD_PATH; do
        go test "./$MOD_PATH/..." 2>/dev/null || true
    done <<< "$MODULE_PATHS"
fi
"#
        .to_string()
    }

    pub fn generate_plugin_hooks() -> crate::types::hook::HooksConfig {
        use crate::types::hook::{Hook, HooksConfig};
        HooksConfig {
            post_tool_use: Some(vec![Hook::new(
                "Edit|Write",
                "${CLAUDE_PLUGIN_ROOT}/.claudegen/hooks/run-module-tests.sh",
            )]),
            ..HooksConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_module_scope_script() {
        let script = HookScriptGenerator::generate_validate_module_scope();
        assert!(script.contains("MODULE_ID"));
        assert!(script.contains("module_map.json"));
        assert!(script.contains("SCOPE VIOLATION"));
        assert!(script.contains("jq -r '.tool_input.file_path"));
        assert!(script.contains("CLAUDE_PROJECT_DIR"));
        assert!(script.contains("exit 2"));
    }

    #[test]
    fn test_run_module_tests_script() {
        let script = HookScriptGenerator::generate_run_module_tests();
        assert!(script.contains("MODULE_ID"));
        assert!(script.contains("Cargo.toml"));
        assert!(script.contains("package.json"));
        assert!(script.contains("go.mod"));
        assert!(script.contains("jq -r '.tool_input.file_path"));
        assert!(script.contains("CLAUDE_PROJECT_DIR"));
    }

    #[test]
    fn test_generate_plugin_hooks() {
        let hooks = HookScriptGenerator::generate_plugin_hooks();
        assert!(hooks.pre_tool_use.is_none());
        let post = hooks.post_tool_use.unwrap();
        assert_eq!(post.len(), 1);
        assert_eq!(post[0].matcher, "Edit|Write");
        assert!(post[0].hooks[0].command.contains("run-module-tests.sh"));
    }
}
