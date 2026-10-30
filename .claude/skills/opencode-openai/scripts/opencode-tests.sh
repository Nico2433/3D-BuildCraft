#!/usr/bin/env bash
# Lets GPT write tests: it may edit only files matching the --edit patterns and run only the test command.
# Usage: opencode-tests.sh <provider/model> "<test command>" <brief file> --edit "tests/**" [--edit "<glob>"]... "<prompt>"
# Prints the answer text. Anything that would need approval is denied, so a headless run never waits;
# OPENCODE_TIMEOUT (seconds, default 900) caps the run. The test command must not reach outside the project.
# Prints the answer text. --standalone is required: the background OpenCode service ignores OPENCODE_CONFIG_CONTENT.
set -euo pipefail
[ $# -ge 5 ] || { echo "usage: $0 <provider/model> \"<test command>\" <brief file> --edit <glob>... \"<prompt>\"" >&2; exit 2; }
model=$1; testcmd=$2; brief=$3; shift 3
edits=()
while [ $# -gt 1 ] && [ "$1" = "--edit" ]; do
  [ "$2" = "*" ] || [ "$2" = "**" ] && { echo "refusing an edit pattern that matches everything" >&2; exit 2; }
  edits+=("$2"); shift 2
done
[ ${#edits[@]} -gt 0 ] || { echo "at least one --edit pattern is required" >&2; exit 2; }
[ -f "$brief" ] || { echo "brief file not found: $brief" >&2; exit 2; }
prompt="$1

You may edit only files matching: ${edits[*]}. The only shell command you may run is the test command, exactly as written, optionally followed by test paths or selectors: $testcmd"
edit_json=$(printf '%s\n' "${edits[@]}" | jq -R . | jq -s 'reduce .[] as $p ({"*":"deny"}; . + {($p):"allow"})')
cfg=$(jq -cn --argjson edit "$edit_json" --arg cmd "$testcmd" '{permission:{
  "*":"deny",
  read:{"*":"allow","*.env":"deny","*.env.*":"deny","*.env.example":"allow"},
  glob:"allow", grep:"allow", list:"allow", skill:"allow",
  edit:$edit, external_directory:"deny",
  bash:{"*":"deny", ($cmd):"allow", ($cmd + " *"):"allow"}}}')
OPENCODE_CONFIG_CONTENT="$cfg" timeout "${OPENCODE_TIMEOUT:-900}" opencode run --standalone --title tests --agent build -m "$model" --format json -f "$brief" "$prompt" \
  | jq -r 'select(.type=="text") | .part.text'
