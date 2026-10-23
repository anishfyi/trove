#!/usr/bin/env bash
# Tests for the project trove generator in scripts/trove.sh: init, render, guard,
# context refresh and the SessionStart re-render. Dependency-free apart from bash,
# git and coreutils; the context cases run once per available JSON reader (jq, python3).
#
# Run: bash plugins/trove/tests/render.test.sh

set -u
export LC_ALL=C

here="$(cd "$(dirname "$0")" && pwd)"
plugin="$(cd "$here/.." && pwd)"
repo="$(cd "$plugin/../.." && pwd)"
trove="$plugin/scripts/trove.sh"
loader="$plugin/scripts/load-trove.sh"

# Resolve symlinks (macOS /var is /private/var) so paths match what git reports.
work="$(cd "$(mktemp -d)" && pwd -P)"
trap 'rm -rf "$work"' EXIT
export HOME="$work/home"
mkdir -p "$HOME"
unset TROVE_ROOT TROVE_PROJECT TROVE_JSON TROVE_SKILL_MAX_BYTES QUIET

pass=0
fail=0
ok() { pass=$((pass + 1)); printf 'ok   %s\n' "$1"; }
no() { fail=$((fail + 1)); printf 'FAIL %s\n' "$1"; [ -n "${2:-}" ] && printf '     %s\n' "$2"; }
check() { if eval "$2"; then ok "$1"; else no "$1" "$2"; fi; }

# A fresh git repo whose directory name is not already a valid skill name.
new_repo() {
  local d="$work/$1"
  rm -rf "$d"; mkdir -p "$d"
  git -C "$d" init -q
  printf '%s\n' "$d"
}

# Front matter checks for a rendered skill: name matches the folder, description is
# present, quoted and short enough for the skill listing.
valid_skill() {
  local f="$1" dir name desc
  dir="$(basename "$(dirname "$f")")"
  [ "$(sed -n 1p "$f")" = "---" ] || return 1
  name="$(sed -n 's/^name: //p' "$f" | head -n 1)"
  desc="$(sed -n 's/^description: //p' "$f" | head -n 1)"
  [ "$name" = "$dir" ] || return 1
  printf '%s' "$name" | grep -Eq '^[a-z0-9]+(-[a-z0-9]+)*$' || return 1
  case "$desc" in \"*\") ;; *) return 1 ;; esac
  [ "${#desc}" -gt 80 ] && [ "${#desc}" -lt 1024 ] || return 1
  [ "$(sed -n '/^---$/=' "$f" | sed -n 2p)" -gt 2 ] || return 1
}

no_dashes() { ! grep -rlE $'\xe2\x80\x94|\xe2\x80\x93' "$@" >/dev/null 2>&1; }

# ---------------------------------------------------------------------------
echo "# init"
r="$(new_repo "My Web_App")"
out="$(cd "$r" && bash "$trove" init 2>&1)"
skills="$r/.claude/skills"
check "init defaults to project scope inside a git repo" '[ -f "$r/.claude/trove/INDEX.md" ] && [ ! -e "$HOME/.claude/trove" ]'
for s in conventions memory context; do
  check "section $s has INDEX.md and entries/" '[ -f "$r/.claude/trove/$s/INDEX.md" ] && [ -d "$r/.claude/trove/$s/entries" ]'
  check "skill my-web-app-$s is valid" 'valid_skill "$skills/my-web-app-$s/SKILL.md"'
done
check "empty context skill points at /trove:index" 'grep -q "Run /trove:index" "$skills/my-web-app-context/SKILL.md"'
check "AGENTS.md has one trove block naming all three skills" \
  '[ "$(grep -c "trove:start" "$r/AGENTS.md")" = 1 ] && grep -q "my-web-app-conventions/SKILL.md" "$r/AGENTS.md" && grep -q "my-web-app-memory/SKILL.md" "$r/AGENTS.md" && grep -q "my-web-app-context/SKILL.md" "$r/AGENTS.md"'
before="$(cd "$r" && find .claude AGENTS.md -type f -exec cksum {} + | sort)"
out="$(cd "$r" && bash "$trove" init 2>&1)"
after="$(cd "$r" && find .claude AGENTS.md -type f -exec cksum {} + | sort)"
check "init is idempotent" '[ "$before" = "$after" ] && printf "%s" "$out" | grep -q "already exists"'
check "trove.sh project prints the slug" '[ "$(cd "$r" && bash "$trove" project)" = "my-web-app" ]'
check "init works from a subdirectory" '(mkdir -p "$r/src/deep" && cd "$r/src/deep" && bash "$trove" path --project) | grep -q "^$r/.claude/trove$"'

# ---------------------------------------------------------------------------
echo "# render"
conv="$r/.claude/trove/conventions"
cat > "$conv/entries/commit-style.md" <<'EOF'
---
title: Plain commit messages
slug: commit-style
type: convention
created: 2026-10-16
tags: [git]
---

Write commit subjects in the imperative, under 72 characters.

## Example

```bash
# this comment is not a heading
git commit -m "Add cache layer"
```

**Why it matters:** history stays greppable.
EOF
cat > "$conv/entries/layout.json" <<'EOF'
{
  "title": "Directory layout",
  "slug": "layout",
  "type": "convention",
  "created": "2026-10-16",
  "tags": ["structure"],
  "summary": "Where each kind of file lives",
  "data": { "src/": "application code", "tests/": "integration tests" }
}
EOF
cat > "$conv/entries/unindexed.md" <<'EOF'
---
title: Unindexed rule
---
Still rendered, after the indexed entries.
EOF
awk '{ print } /<!-- entries below -->/ {
  print "- [Directory layout](entries/layout.json) - where files live"
  print "- [Plain commit messages](entries/commit-style.md) - imperative subjects"
}' "$conv/INDEX.md" > "$work/idx" && mv "$work/idx" "$conv/INDEX.md"

out="$(cd "$r" && bash "$trove" render conventions 2>&1)"
cs="$skills/my-web-app-conventions/SKILL.md"
check "render reports the changed skill" 'printf "%s" "$out" | grep -q "Rendered my-web-app-conventions"'
check "rendered skill is still valid" 'valid_skill "$cs"'
check "markdown entry inlined with its title and source" 'grep -q "^### Plain commit messages$" "$cs" && grep -q "Write commit subjects in the imperative" "$cs" && grep -q "Source: \`.claude/trove/conventions/entries/commit-style.md\`" "$cs"'
check "front matter of the entry is stripped" '! grep -q "^slug: commit-style" "$cs"'
check "entry headings are demoted below the entry title" 'grep -q "^#### Example$" "$cs"'
check "fenced code is left alone" 'grep -q "^# this comment is not a heading$" "$cs"'
check "json entry rendered from title and summary" 'grep -q "^### Directory layout$" "$cs" && grep -q "Where each kind of file lives" "$cs" && grep -q "entries/layout.json" "$cs"'
order="$(grep '^### ' "$cs" | tr '\n' '|')"
check "entries follow INDEX.md order, unindexed last" '[ "$order" = "### Directory layout|### Plain commit messages|### Unindexed rule|" ]'
check "entry count is shown" 'grep -q "(3 entries)" "$cs"'
out="$(cd "$r" && bash "$trove" render conventions 2>&1)"
check "re-render without changes leaves the file alone" 'printf "%s" "$out" | grep -q "Unchanged my-web-app-conventions"'

sed -i.bak 's/under 72 characters/under 60 characters/' "$conv/entries/commit-style.md" && rm -f "$conv/entries/commit-style.md.bak"
(cd "$r" && bash "$trove" render conventions >/dev/null 2>&1)
check "editing an entry updates the skill" 'grep -q "under 60 characters" "$cs"'

out="$(cd "$r" && TROVE_SKILL_MAX_BYTES=200 bash "$trove" render conventions 2>&1)"
check "entries past the size budget are listed, not inlined" 'grep -q "^## More entries" "$cs" && grep -q "^- Unindexed rule: \`.claude/trove/conventions/entries/unindexed.md\`" "$cs"'
(cd "$r" && bash "$trove" render conventions >/dev/null 2>&1)

check "route sends conventions, memory and context to the right section" \
  '[ "$(bash "$trove" route convention)" = conventions ] && [ "$(bash "$trove" route gotcha)" = memory ] && [ "$(bash "$trove" route decision)" = memory ] && [ "$(bash "$trove" route architecture)" = context ]'
check "grep searches section entries" '(cd "$r" && bash "$trove" grep imperative) | grep -q "commit-style.md"'

# ---------------------------------------------------------------------------
echo "# guard"
cat > "$work/secret.md" <<'EOF'
---
title: Deploy notes
---
Use sk-proj-abcdefghijklmnopqrstuvwxyz0123456789 for the staging client.
aws AKIAIOSFODNN7EXAMPLE and ghp_abcdefghijklmnopqrstuvwxyz0123456789 and xoxb-123456789012-abcdefghij
export API_KEY="abc123def456ghi"
-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAA
-----END OPENSSH PRIVATE KEY-----
Kept line.
EOF
cat > "$work/prose.md" <<'EOF'
---
title: Ordinary prose
---
The auth token is passed in the Authorization header, never in the query string.
Set max_tokens: 4096 when calling the model. Secrets live in Vault under the deploy path.
Configure it with token=$API_TOKEN or password: ${DB_PASSWORD} or api_key=<your key>.
A password: required field and a secret: rotate it monthly. sk-learn is a library name.
Keys in config: token_ttl: 3600, password_min_length: 12.
EOF
out="$(bash "$trove" guard "$work/secret.md" 2>&1)"; status=$?
check "guard refuses an entry with a fake sk- token" '[ "$status" = 1 ] && printf "%s" "$out" | grep -q "openai/anthropic key"'
check "guard reports lines, not the secret itself" '! printf "%s" "$out" | grep -q "abcdefghijklmnop"'
check "guard flags aws, github, slack, assignment and private key shapes" \
  'for k in "aws access key" "github token" "slack token" "secret assignment" "private key"; do printf "%s" "$out" | grep -q "$k" || exit 1; done'
out="$(bash "$trove" guard "$work/prose.md" 2>&1)"; status=$?
# widened assignment shapes: quoted multi-word passphrases, key families,
# spaced spellings (fake credentials assembled at runtime)
frag_q="Nf3Wq9"; frag_r="Zb7Xe2KmYt"
cat > "$work/quoted.md" <<EOF
password = "correct horse battery staple 42"
passphrase: '$frag_q$frag_r'
signing key = $frag_q$frag_r
encryption_key: $frag_q$frag_r
master-key = $frag_q$frag_r
session_key = $frag_q$frag_r
api key = $frag_q$frag_r
EOF
out="$(bash "$trove" guard "$work/quoted.md" 2>&1)"; status=$?
check "guard flags quoted passphrases and key-family assignments" '[ "$status" = 1 ]'

cat > "$work/clean2.md" <<'EOF'
The password policy rotates monthly; api key guidance lives in the wiki.
password = "short1"
passphrase = get_passphrase()
credentials = env.CREDS
EOF
out="$(bash "$trove" guard "$work/clean2.md" 2>&1)"; status=$?
check "guard still passes prose, short quoted values and code references" '[ "$status" = 0 ] && [ -z "$out" ]'

check "guard does not flag ordinary prose or placeholders" '[ "$status" = 0 ] && [ -z "$out" ]'
cp "$work/secret.md" "$work/redact.md"
bash "$trove" guard --redact "$work/redact.md" >/dev/null 2>&1
check "redact removes every secret" 'bash "$trove" guard "$work/redact.md" >/dev/null 2>&1 && ! grep -q "abcdefghij" "$work/redact.md" && ! grep -q "AKIA" "$work/redact.md"'
check "redact keeps keys and surrounding prose" 'grep -q "^export API_KEY=\"\[REDACTED\]" "$work/redact.md" && grep -q "^\[REDACTED PRIVATE KEY\]$" "$work/redact.md" && grep -q "^Kept line.$" "$work/redact.md" && ! grep -q "b3BlbnNz" "$work/redact.md"'
cp "$work/prose.md" "$work/prose2.md"
bash "$trove" guard --redact "$work/prose2.md" >/dev/null 2>&1
check "redact leaves clean files byte for byte" 'cmp -s "$work/prose.md" "$work/prose2.md"'

cp "$work/secret.md" "$r/.claude/trove/memory/entries/deploy-notes.md"
err="$(cd "$r" && bash "$trove" render memory 2>&1 >/dev/null)"
ms="$skills/my-web-app-memory/SKILL.md"
check "render redacts secrets that slipped into an entry, with a warning" \
  'printf "%s" "$err" | grep -q "Redacted possible secrets" && ! grep -q "sk-proj" "$ms" && bash "$trove" guard "$ms" >/dev/null 2>&1'
rm -f "$r/.claude/trove/memory/entries/deploy-notes.md"
(cd "$r" && bash "$trove" render memory >/dev/null 2>&1)

# ---------------------------------------------------------------------------
echo "# context"
write_engine_index() {
  local t="$1/.trove"
  rm -rf "$t"; mkdir -p "$t/architecture" "$t/subsystems" "$t/modules"
  cat > "$t/manifest.json" <<'EOF'
{
  "version": "0.2.0",
  "repo_root": "/x",
  "indexed_at": "2026-10-16T01:14:31.165851174+00:00",
  "symbol_count": 42,
  "module_count": 3,
  "subsystem_count": 2
}
EOF
  cat > "$t/architecture/architecture_root.json" <<'EOF'
{
  "id": "architecture:root",
  "kind": "architecture",
  "path": "my-web-app",
  "text": {
    "raw": "# Architecture: my-web-app\n\n## Overview\nTrove-indexed repository with 42 symbols across 2 subsystems.\n\n## Subsystems\n- src (30 symbols, 2 modules)\n- tests (12 symbols, 1 modules)\n",
    "summary": "short",
    "ultra_summary": null,
    "keywords": ["architecture"],
    "embedding": null
  },
  "symbols": []
}
EOF
  cat > "$t/subsystems/subsystem_src.json" <<'EOF'
{
  "id": "subsystem:src",
  "kind": "subsystem",
  "path": "src",
  "text": { "raw": "# Subsystem: src\n\nFiles: 2\nSymbols: 30\n\n## Composition\n- 30 function\n\n## Modules\n- src/app.ts\n- src/db.ts", "keywords": [] },
  "symbols": []
}
EOF
  cat > "$t/subsystems/subsystem_tests.json" <<'EOF'
{
  "id": "subsystem:tests",
  "kind": "subsystem",
  "path": "tests",
  "text": { "raw": "# Subsystem: tests\n\nFiles: 1\nSymbols: 12\n\n## Modules\n- tests/app.test.ts", "keywords": [] },
  "symbols": []
}
EOF
  cat > "$t/modules/module_src_app_ts.json" <<'EOF'
{ "id": "module:src/app.ts", "path": "src/app.ts", "text": { "keywords": ["router", "handler"] }, "symbols": ["a", "b", "c"] }
EOF
  cat > "$t/modules/module_src_db_ts.json" <<'EOF'
{ "id": "module:src/db.ts", "path": "src/db.ts", "text": { "keywords": [] }, "symbols": ["a", "b", "c", "d", "e"] }
EOF
  cat > "$t/modules/module_README_md.json" <<'EOF'
{ "id": "module:README.md", "path": "README.md", "text": { "keywords": [] }, "symbols": [] }
EOF
}

readers=""
command -v jq >/dev/null 2>&1 && readers="$readers jq"
command -v python3 >/dev/null 2>&1 && readers="$readers python3"
[ -n "$readers" ] || echo "skip context cases: neither jq nor python3 is installed"
first_ctx=""
for reader in $readers; do
  ctx="$r/.claude/trove/context"
  xs="$skills/my-web-app-context/SKILL.md"
  write_engine_index "$r"
  cat > "$ctx/entries/service-map.md" <<'EOF'
---
title: Service map
type: context
---
The API talks to Postgres only through src/db.ts.
EOF
  grep -q 'service-map.md' "$ctx/INDEX.md" || printf '%s\n' '- [Service map](entries/service-map.md) - hand-written' >> "$ctx/INDEX.md"
  out="$(cd "$r" && TROVE_JSON=$reader bash "$trove" context 2>&1)"; status=$?
  check "[$reader] context refresh succeeds" '[ "$status" = 0 ] && printf "%s" "$out" | grep -q "42 symbols"'
  check "[$reader] generated entries written" '[ -f "$ctx/entries/architecture.md" ] && [ -f "$ctx/entries/subsystem-src.md" ] && [ -f "$ctx/entries/subsystem-tests.md" ] && [ -f "$ctx/entries/key-modules.md" ]'
  check "[$reader] INDEX generated block lists them with counts" 'grep -q "^- \[Subsystem: src\](entries/subsystem-src.md) - 2 files, 30 symbols$" "$ctx/INDEX.md" && grep -q "^- \[Architecture\](entries/architecture.md) - 42 symbols in 3 modules across 2 subsystems$" "$ctx/INDEX.md"'
  check "[$reader] key modules sorted by symbol count, empty ones dropped" '[ "$(grep -c "^- \`" "$ctx/entries/key-modules.md")" = 2 ] && grep -n "src/db.ts" "$ctx/entries/key-modules.md" | grep -q "^11:" && grep -q "src/app.ts\` (3 symbols): router, handler" "$ctx/entries/key-modules.md"'
  check "[$reader] context skill is valid and carries the map" 'valid_skill "$xs" && grep -q "^### Architecture$" "$xs" && grep -q "^### Subsystem: src$" "$xs" && grep -q "src/db.ts" "$xs" && grep -q "Indexed: 2026-10-16T01:14:31." "$xs"'
  check "[$reader] hand-written context entries survive" 'grep -q "^### Service map$" "$xs" && [ -f "$ctx/entries/service-map.md" ]'
  if [ -z "$first_ctx" ]; then
    first_ctx="$(cat "$xs")"
  else
    check "jq and python3 readers render the same skill" '[ "$first_ctx" = "$(cat "$xs")" ]'
  fi

  # Re-index after a subsystem disappears: its generated entry goes too.
  rm -f "$r/.trove/subsystems/subsystem_tests.json"
  (cd "$r" && TROVE_JSON=$reader bash "$trove" context >/dev/null 2>&1)
  check "[$reader] re-index drops stale generated entries" '[ ! -f "$ctx/entries/subsystem-tests.md" ] && ! grep -q "subsystem-tests" "$ctx/INDEX.md" && ! grep -q "Subsystem: tests" "$xs"'
  check "[$reader] generated block stays single" '[ "$(grep -c "generated:start" "$ctx/INDEX.md")" = 1 ]'
done

check "context without an engine index fails clearly" '! (cd "$(new_repo noindex)" && bash "$trove" context >/dev/null 2>&1)'

# ---------------------------------------------------------------------------
echo "# AGENTS.md and hooks"
r2="$(new_repo existing)"
printf '# Agents\n\nRun make test before pushing.\n' > "$r2/AGENTS.md"
(cd "$r2" && bash "$trove" init >/dev/null 2>&1 && bash "$trove" render >/dev/null 2>&1)
check "existing AGENTS.md content is kept and the block added once" \
  'grep -q "Run make test before pushing." "$r2/AGENTS.md" && [ "$(grep -c "trove:start" "$r2/AGENTS.md")" = 1 ]'

r3="$(new_repo legacy)"
mkdir -p "$r3/.claude/trove/entries"
printf '# Trove Index\n\n<!-- entries below -->\n- [Old](entries/old.md) - kept\n' > "$r3/.claude/trove/INDEX.md"
printf -- '---\ntitle: Old\n---\nOld note.\n' > "$r3/.claude/trove/entries/old.md"
(cd "$r3" && bash "$trove" init --project >/dev/null 2>&1)
check "an older flat project trove gains sections and keeps its entries" \
  '[ -f "$r3/.claude/trove/memory/INDEX.md" ] && grep -q "entries/old.md" "$r3/.claude/trove/INDEX.md" && grep -q "conventions/INDEX.md" "$r3/.claude/trove/INDEX.md"'

# Edit an entry without rendering, as a hand edit or a git pull would.
sed -i.bak 's/under 60 characters/under 50 characters/' "$conv/entries/commit-style.md" && rm -f "$conv/entries/commit-style.md.bak"
touch -t 202001010000 "$cs"
out="$(cd "$r" && bash "$loader")"; status=$?
check "SessionStart hook exits 0 and loads section indexes" '[ "$status" = 0 ] && printf "%s" "$out" | grep -q "\[conventions\] - \[Plain commit messages\](conventions/entries/commit-style.md)"'
check "SessionStart hook re-renders a stale skill" 'grep -q "under 50 characters" "$cs"'
ms_time="$(cksum < "$ms")"; touch -t 202001010000 "$ms"; touch -t 201901010000 "$r/.claude/trove/memory/INDEX.md"
find "$r/.claude/trove/memory/entries" -exec touch -t 201901010000 {} +
(cd "$r" && bash "$loader" >/dev/null)
check "SessionStart hook leaves fresh skills alone" '[ ! "$ms" -nt "$r/.claude/trove/conventions/INDEX.md" ] && [ "$(cksum < "$ms")" = "$ms_time" ]'
out="$(cd "$work" && bash "$loader")"; status=$?
check "SessionStart hook is quiet and exits 0 outside any trove" '[ "$status" = 0 ] && [ -z "$out" ]'

check "user scope keeps the flat layout" '(cd "$work" && bash "$trove" init --user >/dev/null) && [ -d "$HOME/.claude/trove/entries" ] && [ ! -d "$HOME/.claude/trove/conventions" ]'

check "no em or en dashes in generated files" 'no_dashes "$r/.claude" "$r/AGENTS.md"'
check "no em or en dashes in the plugin" 'no_dashes "$plugin"'

# ---------------------------------------------------------------------------
# End to end with the real engine when a built trove binary is available.
bin="${TROVE_BIN:-$repo/target/debug/trove}"
if [ -x "$bin" ] && { command -v jq >/dev/null 2>&1 || command -v python3 >/dev/null 2>&1; }; then
  echo "# engine end to end"
  r4="$(new_repo e2e)"
  mkdir -p "$r4/src"
  printf 'pub fn add(a: i32, b: i32) -> i32 { a + b }\npub struct Point { x: i32 }\n' > "$r4/src/lib.rs"
  (cd "$r4" && bash "$trove" init >/dev/null 2>&1 && "$bin" --repo . index >/dev/null 2>&1 && bash "$trove" context >/dev/null 2>&1)
  check "real index plus context renders a valid context skill" \
    'valid_skill "$r4/.claude/skills/e2e-context/SKILL.md" && grep -q "^### Subsystem: src$" "$r4/.claude/skills/e2e-context/SKILL.md"'
else
  echo "skip engine end to end: no built trove binary (set TROVE_BIN or cargo build -p trove-cli)"
fi

printf '\n%s passed, %s failed\n' "$pass" "$fail"
[ "$fail" = 0 ]
