#!/usr/bin/env bash
# SessionEnd hook: validate the trove(s) as the session closes and leave the
# findings where the NEXT session will see them (.audit/last-validation.md
# inside the trove, surfaced by load-trove.sh at start).
#
# SessionEnd output goes nowhere visible, so this writes files instead of
# printing. Must never fail the session: exits 0 no matter what.

HERE="$(cd "$(dirname "$0")" && pwd)"

for dir in "$PWD/.claude/trove" "$HOME/.claude/trove"; do
  if [ -f "$dir/INDEX.md" ]; then
    bash "$HERE/validate-trove.sh" "$dir" --report "$dir/.audit/last-validation.md" > /dev/null 2>&1
  fi
done

exit 0
