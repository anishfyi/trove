#!/usr/bin/env bash
# Trove test suite. Plain bash; optional tools (shellcheck, jq) skip if absent.
set -u
cd "$(dirname "$0")/.." || exit 1
ROOT="$PWD"
SCRIPTS="$ROOT/plugins/trove/scripts"
PASS=0; FAIL=0

ok()   { PASS=$((PASS+1)); echo "  ok: $1"; }
fail() { FAIL=$((FAIL+1)); echo "  FAIL: $1"; }

assert_contains() { # file needle label
  if grep -qF -- "$2" "$1"; then ok "$3"; else fail "$3 (missing: $2)"; fi
}

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

make_trove() { # $1 = dir in which to create .claude/trove
  local T="$1/.claude/trove"
  mkdir -p "$T/entries/big-system-map"
  cat > "$T/INDEX.md" <<'EOF'
# Trove Index

> Your personal Claude Code trove. One line per entry, newest first.

<!-- entries below -->

- [Deploy gotcha](entries/deploy-gotcha.md) - staging push blocks on settings.py
- [Endpoint table](entries/endpoint-table.json) - API endpoints by service
- [Big system map](entries/big-system-map/00-README.md) - multi-file map entry
EOF
  cat > "$T/entries/deploy-gotcha.md" <<'EOF'
---
title: Deploy gotcha
slug: deploy-gotcha
type: gotcha
created: 2026-07-01
tags: [deploy]
---

Staging push blocks when settings.py changed.

**Why it matters:** silent deploy failures.
**Related:** [[future-entry]]
EOF
  cat > "$T/entries/endpoint-table.json" <<'EOF'
{
  "title": "Endpoint table",
  "slug": "endpoint-table",
  "type": "reference",
  "created": "2026-07-02",
  "tags": ["api"],
  "summary": "API endpoints by service",
  "data": { "orders": "/api/orders" }
}
EOF
  printf '# Big system map\n\nmulti-file entry root\n' > "$T/entries/big-system-map/00-README.md"
}

echo "- validate-trove.sh -"
mkdir -p "$TMP/proj" && make_trove "$TMP/proj"
( cd "$TMP/proj" && bash "$SCRIPTS/validate-trove.sh" > out.txt 2>&1 )
rc=$?
if [ "$rc" -eq 0 ] && grep -q "clean" "$TMP/proj/out.txt"; then ok "clean trove -> exit 0"; else fail "clean trove -> exit 0 (rc=$rc)"; cat "$TMP/proj/out.txt" | sed 's/^/    /'; fi

# Break it in every category the validator covers.
cat > "$TMP/proj/.claude/trove/entries/orphan.md" <<'EOF'
---
title: Orphan
slug: not-orphan
type: gotcha
created: 2026-7-1
---

body
EOF
printf '{ broken json' > "$TMP/proj/.claude/trove/entries/bad.json"
echo "- [Bad JSON](entries/bad.json) - broken" >> "$TMP/proj/.claude/trove/INDEX.md"
echo "- [Ghost](entries/ghost.md) - points nowhere" >> "$TMP/proj/.claude/trove/INDEX.md"

( cd "$TMP/proj" && bash "$SCRIPTS/validate-trove.sh" > out.txt 2>&1 )
rc=$?
if [ "$rc" -eq 1 ]; then ok "broken trove -> exit 1"; else fail "broken trove -> exit 1 (rc=$rc)"; fi
assert_contains "$TMP/proj/out.txt" "INDEX.md links to missing file: entries/ghost.md" "dead index link caught"
assert_contains "$TMP/proj/out.txt" "entries/orphan.md: not listed in INDEX.md" "orphan entry caught"
assert_contains "$TMP/proj/out.txt" "slug 'not-orphan' does not match filename" "slug mismatch caught"
assert_contains "$TMP/proj/out.txt" "created '2026-7-1' is not a YYYY-MM-DD date" "malformed date caught"
if command -v jq > /dev/null || command -v python3 > /dev/null; then
  assert_contains "$TMP/proj/out.txt" "entries/bad.json: invalid JSON" "invalid JSON caught"
fi

echo "- session-end.sh + loader surfacing -"
( cd "$TMP/proj" && HOME="$TMP/nohome" bash "$SCRIPTS/session-end.sh" )
if [ -f "$TMP/proj/.claude/trove/.audit/last-validation.md" ]; then ok "session end writes validation report"; else fail "session end writes validation report"; fi
( cd "$TMP/proj" && HOME="$TMP/nohome" bash "$SCRIPTS/load-trove.sh" > out.txt 2>&1 )
assert_contains "$TMP/proj/out.txt" "Trove validation report" "loader surfaces pending report"
assert_contains "$TMP/proj/out.txt" "/trove:audit" "loader points at /trove:audit"

rm -f "$TMP/proj/.claude/trove/entries/orphan.md" "$TMP/proj/.claude/trove/entries/bad.json"
grep -v -e 'entries/ghost.md' -e 'entries/bad.json' "$TMP/proj/.claude/trove/INDEX.md" > "$TMP/proj/idx" && mv "$TMP/proj/idx" "$TMP/proj/.claude/trove/INDEX.md"
( cd "$TMP/proj" && HOME="$TMP/nohome" bash "$SCRIPTS/session-end.sh" )
if [ ! -f "$TMP/proj/.claude/trove/.audit/last-validation.md" ]; then ok "clean session end clears stale report"; else fail "clean session end clears stale report"; fi

echo "- validator tolerance (regression) -"
# Trailing spaces, tabs after the colon, and index anchor links are not defects.
printf -- '---\ntitle: Space case\nslug: space-case\ntype: gotcha\ncreated: 2026-07-01 \ntags:\t[x]\n---\n\nBody.\n' > "$TMP/proj/.claude/trove/entries/space-case.md"
echo "- [Space case](entries/space-case.md) - whitespace variants" >> "$TMP/proj/.claude/trove/INDEX.md"
echo "- [Deploy gotcha, anchored](entries/deploy-gotcha.md#why) - anchor link" >> "$TMP/proj/.claude/trove/INDEX.md"
( cd "$TMP/proj" && bash "$SCRIPTS/validate-trove.sh" > out.txt 2>&1 )
rc=$?
if [ "$rc" -eq 0 ]; then ok "whitespace + anchor links -> still clean"; else fail "whitespace + anchor links -> still clean (rc=$rc)"; sed 's/^/    /' "$TMP/proj/out.txt"; fi

echo "- load-trove.sh basics -"
mkdir -p "$TMP/empty"
( cd "$TMP/empty" && HOME="$TMP/nohome" bash "$SCRIPTS/load-trove.sh" > out.txt 2>&1 )
rc=$?
if [ "$rc" -eq 0 ] && [ ! -s "$TMP/empty/out.txt" ]; then ok "no trove -> silent exit 0"; else fail "no trove -> silent exit 0 (rc=$rc)"; fi
( cd "$TMP/proj" && HOME="$TMP/nohome" bash "$SCRIPTS/load-trove.sh" > out.txt 2>&1 )
assert_contains "$TMP/proj/out.txt" "Deploy gotcha" "INDEX content injected"

echo "- trove.sh -"
( cd "$TMP/proj" && bash "$SCRIPTS/trove.sh" validate > out.txt 2>&1 )
rc=$?
if [ "$rc" -eq 0 ] && grep -q "clean" "$TMP/proj/out.txt"; then ok "trove.sh validate delegates"; else fail "trove.sh validate delegates (rc=$rc)"; fi

echo "- static checks -"
if command -v shellcheck > /dev/null; then
  if shellcheck -S warning plugins/trove/scripts/*.sh tests/run.sh; then ok "shellcheck"; else fail "shellcheck"; fi
else
  echo "  skip: shellcheck not installed"
fi
if command -v jq > /dev/null; then
  bad=0
  for j in plugins/trove/.claude-plugin/plugin.json plugins/trove/hooks/hooks.json .claude-plugin/marketplace.json; do
    jq . "$j" > /dev/null 2>&1 || { bad=1; fail "invalid JSON: $j"; }
  done
  [ "$bad" -eq 0 ] && ok "JSON files valid"
  if jq -e '.hooks.SessionStart[0].matcher == "startup|resume|clear|compact"' plugins/trove/hooks/hooks.json > /dev/null 2>&1; then
    ok "SessionStart matcher covers clear+compact"
  else
    fail "SessionStart matcher covers clear+compact"
  fi
  if jq -e '.hooks.SessionEnd[0].hooks[0].command | contains("session-end.sh")' plugins/trove/hooks/hooks.json > /dev/null 2>&1; then
    ok "SessionEnd hook wired to session-end.sh"
  else
    fail "SessionEnd hook wired to session-end.sh"
  fi
  if jq -e '.version == "0.2.0"' plugins/trove/.claude-plugin/plugin.json > /dev/null 2>&1; then
    ok "plugin version is 0.2.0"
  else
    fail "plugin version is 0.2.0"
  fi
else
  echo "  skip: jq not installed"
fi

# House style: no em dashes (pattern built from escapes so this file cannot
# match itself).
EMDASH="$(printf '\342\200\224')"
if grep -rn --exclude-dir=.git -e "$EMDASH" . > /dev/null 2>&1; then
  fail "house style: no em dashes in the repo"
  grep -rln --exclude-dir=.git -e "$EMDASH" . | sed 's/^/    /'
else
  ok "house style: no em dashes in the repo"
fi

echo
echo "passed: $PASS  failed: $FAIL"
[ "$FAIL" -eq 0 ]
