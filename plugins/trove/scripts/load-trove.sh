#!/usr/bin/env bash
# SessionStart hook: inject the user's Trove index into context so Claude is aware
# of past learnings every session. Must never fail the session, so it stays
# defensive and exits 0 no matter what. The only write it does is re-rendering
# project trove skills whose source entries changed since the last render.

here="$(cd "$(dirname "$0")" && pwd)"
root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
sections="conventions memory context"
emit=""

# Re-render a project trove section when its entries are newer than its skill,
# so hand edits and git pulls never leave the skills stale.
refresh_skills() {
  dir="$1"
  [ -f "$dir/conventions/INDEX.md" ] || return 0
  project="$(TROVE_ROOT="$root" bash "$here/trove.sh" project 2>/dev/null)" || return 0
  for s in $sections; do
    skill="$root/.claude/skills/$project-$s/SKILL.md"
    if [ ! -f "$skill" ] || [ "$dir/$s/INDEX.md" -nt "$skill" ] \
      || [ -n "$(find "$dir/$s/entries" -newer "$skill" 2>/dev/null | head -n 1)" ]; then
      TROVE_ROOT="$root" QUIET=1 bash "$here/trove.sh" render "$s" >/dev/null 2>&1 || true
    fi
  done
}

add_trove() {
  dir="$1"
  if [ -f "$dir/INDEX.md" ]; then
    count=$(find "$dir" -path '*/entries/*' \( -name '*.md' -o -name '*.json' \) 2>/dev/null | wc -l | tr -d ' ')
    pretty="${dir/#$HOME/~}"
    emit="${emit}
## Trove: ${pretty} (${count} entries)
$(cat "$dir/INDEX.md" 2>/dev/null)
"
    for s in $sections; do
      if [ -f "$dir/$s/INDEX.md" ]; then
        emit="${emit}
$(grep -E '^[[:space:]]*[-*] \[' "$dir/$s/INDEX.md" 2>/dev/null | sed "s#](entries/#]($s/entries/#" | sed "s/^/[$s] /")"
      fi
    done
  fi
}

# Project trove first (more specific), then user trove.
refresh_skills "$root/.claude/trove" 2>/dev/null
add_trove "$root/.claude/trove"
add_trove "$HOME/.claude/trove"

if [ -n "$emit" ]; then
  printf '%s\n' "Your Claude Code Trove is loaded. Consult it before answering. Capture new durable learnings with /trove:remember and search with /trove:recall."
  if [ -f "$root/.claude/trove/conventions/INDEX.md" ]; then
    printf '%s\n' "This project trove is also available as the conventions, memory and context skills in .claude/skills/. Load them when the task touches those areas."
  fi
  printf '%s\n' "$emit"
fi

exit 0
