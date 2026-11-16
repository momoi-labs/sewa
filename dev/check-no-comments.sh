#!/usr/bin/env bash
set -euo pipefail

base=${1:-origin/main}
root=$(git rev-parse --show-toplevel)
cd "$root"

added=$(
  git diff -U0 --no-color "$(git merge-base "$base" HEAD)" -- '*.rs' '*.ts' '*.tsx' |
    awk '
      /^\+\+\+ / { file = substr($0, 7); next }
      /^@@ /     { split($3, p, ","); line = substr(p[1], 2) + 0; next }
      /^\+/      { print file ":" line ":" substr($0, 2); line++ }
    '
)

comments=$(
  printf '%s\n' "$added" |
    grep -E '^[^:]+:[0-9]+:(\s*(//|/\*|\{/\*)|.*([^:\\/]//|[^*/]/\*|[^*]\*/\*))' |
    grep -vE '^[^:]+:[0-9]+:\s*//\s*https?://\S+/(issues|pull)/[0-9]+\S*\s*$' || true
)

if [ -n "$comments" ]; then
  echo "The diff adds comments. Put the why in the commit message instead:" >&2
  echo "$comments" >&2
  exit 1
fi
