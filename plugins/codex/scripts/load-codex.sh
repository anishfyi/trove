#!/usr/bin/env bash
# SessionStart hook: inject the user's Codex index into context so Claude is aware
# of past learnings every session. Read-only. Must never fail the session, so it
# stays defensive and exits 0 no matter what.

emit=""

add_codex() {
  dir="$1"
  if [ -f "$dir/INDEX.md" ]; then
    count=$(find "$dir/entries" -name '*.md' 2>/dev/null | wc -l | tr -d ' ')
    pretty="${dir/#$HOME/~}"
    emit="${emit}
## Codex: ${pretty} (${count} entries)
$(cat "$dir/INDEX.md" 2>/dev/null)
"
  fi
}

# Project codex first (more specific), then user codex.
add_codex "$PWD/.claude/codex"
add_codex "$HOME/.claude/codex"

if [ -n "$emit" ]; then
  printf '%s\n' "Your Claude Code Codex is loaded. Consult it before answering. Capture new durable learnings with /codex:remember and search with /codex:recall."
  printf '%s\n' "$emit"
fi

exit 0
