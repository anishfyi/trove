#!/usr/bin/env bash
# trove.sh - a tiny, dependency-free helper for the Trove skill.
# The skills drive most logic through Claude, but this gives a mechanical CLI
# for the boring parts and lets manual (non-plugin) users script the trove too.
#
# Usage:
#   trove.sh path   [--user|--project]   print the resolved trove directory
#   trove.sh init   [--user|--project]   scaffold trove dir + INDEX.md
#   trove.sh list                        print the index
#   trove.sh grep <term>                 search entries
#
# Resolution: --project -> ./.claude/trove, --user -> ~/.claude/trove.
# With no flag: an existing project trove wins, else the user trove.

set -u

user_dir="$HOME/.claude/trove"
proj_dir="$PWD/.claude/trove"

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
# Trove Index

> Your personal Claude Code trove. One line per entry, newest first.
> Loaded into context at the start of every session by the trove plugin.
> Capture new entries with /trove:remember and search with /trove:recall.

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
      echo "Created trove at $dir"
    else
      echo "Trove already exists at $dir"
    fi
    ;;
  list)
    dir="$(resolve "${1:-}")"
    if [ -f "$dir/INDEX.md" ]; then cat "$dir/INDEX.md"; else
      echo "No trove found. Run: trove.sh init"; fi
    ;;
  grep)
    dir="$(resolve)"
    term="${1:-}"
    if [ -z "$term" ]; then echo "usage: trove.sh grep <term>"; exit 2; fi
    grep -rin --include='*.md' "$term" "$dir/entries" 2>/dev/null || echo "No matches."
    ;;
  *)
    echo "usage: trove.sh {path|init|list|grep} [--user|--project]"
    exit 2
    ;;
esac
