#!/usr/bin/env bash
# Runs one read-only OpenCode job and prints the answer text.
# Usage: opencode-readonly.sh <provider/model> [-f <file>]... "<prompt>"
# Only read, glob, grep, list and skill are allowed (no shell, edits, web or .env reads).
# --standalone is required: the background OpenCode service ignores OPENCODE_CONFIG_CONTENT.
set -euo pipefail
[ $# -ge 2 ] || { echo "usage: $0 <provider/model> [-f file]... \"<prompt>\"" >&2; exit 2; }
model=$1; shift
cfg='{"permission":{"*":"deny","read":{"*":"allow","*.env":"deny","*.env.*":"deny","*.env.example":"allow"},"glob":"allow","grep":"allow","list":"allow","skill":"allow","external_directory":"deny"}}'
OPENCODE_CONFIG_CONTENT="$cfg" timeout "${OPENCODE_TIMEOUT:-900}" opencode run --standalone --title readonly --agent plan -m "$model" --format json "$@" \
  | jq -r 'select(.type=="text") | .part.text'
