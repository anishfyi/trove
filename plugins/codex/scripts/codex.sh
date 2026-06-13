#!/usr/bin/env bash
# codex.sh - a tiny, dependency-free helper for the Codex skill.
# The skills drive most logic through Claude, but this gives a mechanical CLI
# for the boring parts and lets manual (non-plugin) users script the codex too.
#
# Usage:
#   codex.sh path   [--user|--project]   print the resolved codex directory
#   codex.sh init   [--user|--project]   scaffold codex dir + INDEX.md
#   codex.sh list                        print the index
#   codex.sh grep <term>                 search entries
#
# Resolution: --project -> ./.claude/codex, --user -> ~/.claude/codex.
# With no flag: an existing project codex wins, else the user codex.

set -u

user_dir="$HOME/.claude/codex"
proj_dir="$PWD/.claude/codex"

resolve() {
  case "${1:-}" in
    --user)    echo "$user_dir" ;;
    --project) echo "$proj_dir" ;;
    *)
      if [ -f "$proj_dir/INDEX.md" ]; then echo "$proj_dir";
      else echo "$user_dir"; fi ;;
  esac
}

index_template() {
  cat <<'EOF'
# Codex Index

> Your personal Claude Code codex. One line per entry, newest first.
> Loaded into context at the start of every session by the codex plugin.
> Capture new entries with /codex:remember and search with /codex:recall.

<!-- entries below -->
EOF
}

cmd="${1:-}"; shift || true

case "$cmd" in
  path)
    resolve "${1:-}"
    ;;
  init)
    dir="$(resolve "${1:-}")"
    mkdir -p "$dir/entries"
    if [ ! -f "$dir/INDEX.md" ]; then
      index_template > "$dir/INDEX.md"
      echo "Created codex at $dir"
    else
      echo "Codex already exists at $dir"
    fi
    ;;
  list)
    dir="$(resolve "${1:-}")"
    if [ -f "$dir/INDEX.md" ]; then cat "$dir/INDEX.md"; else
      echo "No codex found. Run: codex.sh init"; fi
    ;;
  grep)
    dir="$(resolve)"
    term="${1:-}"
    if [ -z "$term" ]; then echo "usage: codex.sh grep <term>"; exit 2; fi
    grep -rin --include='*.md' "$term" "$dir/entries" 2>/dev/null || echo "No matches."
    ;;
  *)
    echo "usage: codex.sh {path|init|list|grep} [--user|--project]"
    exit 2
    ;;
esac
