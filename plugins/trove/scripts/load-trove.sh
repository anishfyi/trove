#!/usr/bin/env bash
# SessionStart hook: inject the user's Trove index into context so Claude is aware
# of past learnings every session. Read-only. Must never fail the session, so it
# stays defensive and exits 0 no matter what.

emit=""

add_trove() {
  dir="$1"
  if [ -f "$dir/INDEX.md" ]; then
    count=$(find "$dir/entries" -name '*.md' 2>/dev/null | wc -l | tr -d ' ')
    pretty="${dir/#$HOME/~}"
    emit="${emit}
## Trove: ${pretty} (${count} entries)
$(cat "$dir/INDEX.md" 2>/dev/null)
"
  fi
}

# Project trove first (more specific), then user trove.
add_trove "$PWD/.claude/trove"
add_trove "$HOME/.claude/trove"

if [ -n "$emit" ]; then
  printf '%s\n' "Your Claude Code Trove is loaded. Consult it before answering. Capture new durable learnings with /trove:remember and search with /trove:recall."
  printf '%s\n' "$emit"
fi

exit 0
