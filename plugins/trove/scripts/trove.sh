#!/usr/bin/env bash
# trove.sh - a tiny, dependency-free helper for the Trove skills.
# The skills drive most logic through Claude, but this gives a mechanical CLI
# for the boring parts and lets manual (non-plugin) users script the trove too.
#
# Usage:
#   trove.sh path    [--user|--project]   print the resolved trove directory
#   trove.sh init    [--user|--project]   scaffold a trove (project scope gets sections)
#   trove.sh list    [--user|--project]   print the index
#   trove.sh grep    <term>               search entries
#   trove.sh route   <type>               print the section an entry type belongs to
#   trove.sh guard   [--redact] <file>... scan files for secrets (exit 1 on a hit)
#   trove.sh render  [section|all]        render sections into .claude/skills/<project>-<section>
#   trove.sh context                      refresh the context section from the engine (.trove/)
#   trove.sh sync                         context (when .trove/ exists) plus render all
#   trove.sh agents                       write the AGENTS.md pointer block
#   trove.sh project                      print the project skill prefix
#
# Resolution: --project -> <repo>/.claude/trove, --user -> ~/.claude/trove.
# With no flag: an existing project trove wins, else the user trove.
# <repo> is $TROVE_ROOT, else the git top level, else the current directory.
#
# A project trove has three sections, each with its own INDEX.md and entries/:
#   conventions/  how this repo is built (style, structure, naming, commit and PR rules)
#   memory/       decisions, gotchas and history, with the why
#   context/      the codebase map from the engine, refreshed on re-index

set -u
export LC_ALL=C

SECTIONS="conventions memory context"
GENERATED_MARK="generated: trove-index"
MAX_BYTES="${TROVE_SKILL_MAX_BYTES:-24000}"

repo_root() {
  if [ -n "${TROVE_ROOT:-}" ]; then echo "$TROVE_ROOT"; return; fi
  git rev-parse --show-toplevel 2>/dev/null || pwd
}

user_dir="$HOME/.claude/trove"
root="$(repo_root)"
proj_dir="$root/.claude/trove"

resolve() {
  case "${1:-}" in
    --user)    echo "$user_dir" ;;
    --project) echo "$proj_dir" ;;
    *)
      if [ -f "$proj_dir/INDEX.md" ]; then echo "$proj_dir";
      else echo "$user_dir"; fi ;;
  esac
}

say() { [ -n "${QUIET:-}" ] || printf '%s\n' "$*"; }
warn() { printf '%s\n' "$*" >&2; }

# ---------------------------------------------------------------------------
# Names and templates
# ---------------------------------------------------------------------------

project_name() {
  local name slug
  name="${TROVE_PROJECT:-$(basename "$root")}"
  slug="$(printf '%s' "$name" | tr '[:upper:]' '[:lower:]' \
    | sed -E 's/[^a-z0-9]+/-/g; s/^-+//; s/-+$//' | cut -c1-40 | sed -E 's/-+$//')"
  [ -n "$slug" ] || slug="project"
  printf '%s\n' "$slug"
}

is_section() {
  case "${1:-}" in conventions|memory|context) return 0 ;; *) return 1 ;; esac
}

section_title() {
  case "$1" in
    conventions) echo "Conventions" ;;
    memory)      echo "Memory" ;;
    context)     echo "Context" ;;
  esac
}

section_blurb() {
  case "$1" in
    conventions) echo "How this repo is built: style, structure, naming, testing, commit and PR rules, and things to never do." ;;
    memory)      echo "Decisions, gotchas and history, each with the why behind it." ;;
    context)     echo "The current map of the codebase: architecture, subsystems and key modules, refreshed from the Trove engine on /trove:index." ;;
  esac
}

# Skill descriptions are what Claude matches against intent, so they name the
# situations where each section matters, not just what it contains.
skill_description() {
  local p="$1"
  case "$2" in
    conventions) echo "Coding conventions for the $p repository: style, structure, naming, testing, commit and PR rules, and things to never do. Use before writing, editing, refactoring, reviewing or committing code in $p, when deciding where a new file goes or what to name something, or when asked how things are done in this repo." ;;
    memory)      echo "Project memory for $p: past decisions and why they were made, known gotchas, workarounds and history. Use before changing behavior, choosing a library or approach, or debugging something that may have broken before, and when asked why something is the way it is, what was decided, or whether an idea was already tried." ;;
    context)     echo "Codebase map for $p from the Trove index: architecture, subsystems and key modules, and where code lives. Use when orienting in the repo, locating where a feature, file or symbol lives, planning a change that spans modules, or explaining how the pieces fit together." ;;
  esac
}

skill_intro() {
  case "$1" in
    conventions) echo "Follow these conventions whenever you write, change or review code in this repository. They take precedence over general defaults." ;;
    memory)      echo "Check these before changing behavior or repeating an approach. Each entry records what was decided or learned and why. Entries reflect what was true when written, so verify file names, flags and commands before relying on them." ;;
    context)     echo "Use this map to find where things live before opening source files. For deeper, task-specific context, run /trove:query (or the trove CLI query command)." ;;
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

project_index_template() {
  local p="$1"
  cat <<EOF
# Trove Index

> Project trove for $p. Three sections, each with its own INDEX.md, rendered into the
> $p-conventions, $p-memory and $p-context skills under .claude/skills/.
> Capture new entries with /trove:remember and search with /trove:recall.

## Sections

- [Conventions](conventions/INDEX.md) - how this repo is built and what never to do
- [Memory](memory/INDEX.md) - decisions, gotchas and history, with the why
- [Context](context/INDEX.md) - codebase map from the Trove engine

<!-- entries below -->
EOF
}

section_index_template() {
  local s="$1" p="$2"
  printf '# %s Index\n\n' "$(section_title "$s")"
  printf '> %s\n' "$(section_blurb "$s")"
  printf '> One line per entry, newest first. Rendered into the %s-%s skill by trove.sh render.\n\n' "$p" "$s"
  if [ "$s" = "context" ]; then
    printf '%s\n' '<!-- generated:start (written by trove.sh context, do not edit by hand) -->'
    printf '%s\n\n' '<!-- generated:end -->'
  fi
  printf '%s\n' '<!-- entries below -->'
}

# ---------------------------------------------------------------------------
# Secret guard (issue #7): mechanical scan for common credential shapes.
# ---------------------------------------------------------------------------

# Case-insensitive bracket form of a word, portable to BSD and GNU sed.
ci() {
  local w="$1" out="" c i
  for ((i = 0; i < ${#w}; i++)); do
    c="${w:i:1}"
    case "$c" in
      [a-zA-Z]) out="$out[$(printf '%s' "$c" | tr '[:lower:]' '[:upper:]')$(printf '%s' "$c" | tr '[:upper:]' '[:lower:]')]" ;;
      *) out="$out$c" ;;
    esac
  done
  printf '%s' "$out"
}

GUARD_READY=""
guard_init() {
  [ -n "$GUARD_READY" ] && return
  GUARD_READY=1
  # name<TAB>regex for credentials with a recognizable shape.
  TOKEN_RULES="openai/anthropic key	sk-[A-Za-z0-9_-]{20,}
aws access key	(AKIA|ASIA)[0-9A-Z]{16}
github token	gh[pousr]_[A-Za-z0-9]{30,}
github token	github_pat_[A-Za-z0-9_]{22,}
gitlab token	glpat-[A-Za-z0-9_-]{20,}
shopify token	shp(at|ss|ca|pa)_[a-fA-F0-9]{32}
slack token	xox[abprs]-[A-Za-z0-9-]{10,}
google api key	AIza[0-9A-Za-z_-]{35}
npm token	npm_[A-Za-z0-9]{36}"
  PEM_RE='-----BEGIN ([A-Z0-9]+ )*PRIVATE KEY-----'
  PEM_END_RE='-----END ([A-Z0-9]+ )*PRIVATE KEY-----'

  # key=value and key: value assignments. The value must look like a secret:
  # at least 8 characters with a digit, or at least 20 characters. Placeholders
  # such as <token>, $TOKEN, ${TOKEN} and [REDACTED] never match.
  local v='[A-Za-z0-9_./+=-]' alts="" i
  alts="[0-9]$v{7,}"
  for i in 1 2 3 4 5 6; do alts="$alts|$v{$i}[0-9]$v{$((7 - i)),}"; done
  alts="$alts|$v{7,}[0-9]$v*|$v{20,}|[^'\"]{7,}[0-9][^'\"]*['\"]|[^'\"]{20,}['\"]"
  local keys
  # Keep in parity with the Rust secret-assignment detector
  # (crates/trove-redact/src/detectors.rs): passphrase and key families,
  # spaced spellings, plural/long forms.
  keys="$(ci apikey)|$(ci api_key)|$(ci api-key)|$(ci "api key")|$(ci secret)|$(ci token)|$(ci password)|$(ci passwd)|$(ci pwd)|$(ci passphrase)|$(ci pass_phrase)|$(ci pass-phrase)|$(ci "pass phrase")|$(ci credential)|$(ci credentials)|$(ci access_key)|$(ci "access key")|$(ci auth_key)|$(ci "auth key")|$(ci private_key)|$(ci "private key")|$(ci client_secret)|$(ci signing_key)|$(ci signing-key)|$(ci "signing key")|$(ci encryption_key)|$(ci encryption-key)|$(ci "encryption key")|$(ci master_key)|$(ci master-key)|$(ci "master key")|$(ci session_key)|$(ci session-key)|$(ci "session key")"
  ASSIGN_RE="([A-Za-z0-9_-]*($keys)[A-Za-z0-9_-]*)([[:space:]]*[=:][[:space:]]*['\"]?)($alts)"
}

# guard_scan <file>: print "file:line: possible <kind>" per hit, return 1 on any hit.
guard_scan() {
  local f="$1" hit=0 name re line
  guard_init
  while IFS='	' read -r name re; do
    [ -n "$re" ] || continue
    for line in $(grep -nE -e "$re" "$f" 2>/dev/null | cut -d: -f1); do
      printf '%s:%s: possible %s\n' "$f" "$line" "$name"; hit=1
    done
  done <<EOF
$TOKEN_RULES
EOF
  for line in $(grep -nE -e "$PEM_RE" "$f" 2>/dev/null | cut -d: -f1); do
    printf '%s:%s: possible private key\n' "$f" "$line"; hit=1
  done
  for line in $(grep -nE -e "$ASSIGN_RE" "$f" 2>/dev/null | cut -d: -f1); do
    printf '%s:%s: possible secret assignment\n' "$f" "$line"; hit=1
  done
  return $hit
}

# guard_redact <file>: rewrite the file in place with every hit replaced by [REDACTED].
guard_redact() {
  local f="$1" tmp name re
  guard_init
  tmp="$(mktemp)"
  cp "$f" "$tmp"
  while IFS='	' read -r name re; do
    [ -n "$re" ] || continue
    sed -E "s#$re#[REDACTED]#g" "$tmp" > "$tmp.n" && mv "$tmp.n" "$tmp"
  done <<EOF
$TOKEN_RULES
EOF
  # Group 1 is the key, group 3 the separator (group 2 is inside the key).
  sed -E "s#$ASSIGN_RE#\\1\\3[REDACTED]#g" "$tmp" > "$tmp.n" && mv "$tmp.n" "$tmp"
  awk -v b="$PEM_RE" -v e="$PEM_END_RE" '
    skip { if ($0 ~ e) skip = 0; next }
    $0 ~ b { print "[REDACTED PRIVATE KEY]"; if ($0 !~ e) skip = 1; next }
    { print }
  ' "$tmp" > "$tmp.n" && mv "$tmp.n" "$tmp"
  cat "$tmp" > "$f"
  rm -f "$tmp"
}

cmd_guard() {
  local redact="" f status=0
  if [ "${1:-}" = "--redact" ]; then redact=1; shift; fi
  if [ $# -eq 0 ]; then echo "usage: trove.sh guard [--redact] <file>..."; return 2; fi
  for f in "$@"; do
    [ -f "$f" ] || { warn "guard: no such file: $f"; status=2; continue; }
    if ! guard_scan "$f"; then
      if [ -n "$redact" ]; then
        guard_redact "$f"
        echo "Redacted secrets in $f"
      else
        status=1
      fi
    fi
  done
  if [ "$status" = 1 ]; then
    warn "Refusing: secrets do not belong in a trove. Remove them, or rerun with --redact."
  fi
  return $status
}

# ---------------------------------------------------------------------------
# Small helpers
# ---------------------------------------------------------------------------

# write_if_changed <tmp> <dest>: move tmp over dest only when the content differs.
# An unchanged dest is touched so its mtime records the last render.
write_if_changed() {
  if [ -f "$2" ] && cmp -s "$1" "$2"; then rm -f "$1"; touch "$2"; return 1; fi
  mkdir -p "$(dirname "$2")"
  mv "$1" "$2"
  return 0
}

plural() { if [ "$1" = 1 ]; then echo "1 $2"; else echo "$1 $3"; fi; }

# fm_field <key> <file>: a value from a Markdown entry's YAML front matter.
fm_field() {
  awk -v k="$1" '
    NR == 1 { if ($0 ~ /^---[[:space:]]*$/) { fm = 1; next } else exit }
    fm && /^---[[:space:]]*$/ { exit }
    fm && index($0, k ":") == 1 {
      v = substr($0, length(k) + 2); sub(/^[[:space:]]+/, "", v); sub(/[[:space:]]+$/, "", v)
      if (v ~ /^".*"$/ || v ~ /^'\''.*'\''$/) v = substr(v, 2, length(v) - 2)
      print v; exit
    }
  ' "$2"
}

# json_str <key> <file>: a top-level string field from a pretty-printed JSON entry.
json_str() {
  sed -nE "s/^[[:space:]]*\"$1\"[[:space:]]*:[[:space:]]*\"(([^\"\\\\]|\\\\.)*)\".*/\\1/p" "$2" | head -n 1
}

# md_body <file>: entry body without front matter, headings demoted two levels to sit under the H3 title.
md_body() {
  awk '
    NR == 1 && /^---[[:space:]]*$/ { fm = 1; next }
    fm == 1 { if (/^---[[:space:]]*$/) fm = 2; next }
    /^[[:space:]]*(```|~~~)/ { fence = !fence; print; next }
    !fence && /^#+[[:space:]]/ {
      n = match($0, /^#+/); n = RLENGTH; rest = substr($0, n + 1); n += 2; if (n > 6) n = 6
      h = ""; for (i = 0; i < n; i++) h = h "#"; print h rest; next
    }
    { print }
  ' "$1" | awk 'NF { started = 1 } started' | awk '
    { lines[NR] = $0 } NF { last = NR }
    END { for (i = 1; i <= last; i++) print lines[i] }
  '
}

# ordered_entries <section dir>: entry file names in INDEX.md order, then any unindexed.
ordered_entries() {
  local sdir="$1" f
  {
    [ -f "$sdir/INDEX.md" ] && sed -nE 's/^[[:space:]]*[-*][[:space:]]*\[[^]]*\]\(entries\/([^)]+)\).*/\1/p' "$sdir/INDEX.md"
    if [ -d "$sdir/entries" ]; then
      for f in "$sdir/entries"/*.md "$sdir/entries"/*.json; do
        [ -f "$f" ] && basename "$f"
      done | sort
    fi
  } | awk '!seen[$0]++' | while IFS= read -r f; do
    [ -f "$sdir/entries/$f" ] && printf '%s\n' "$f"
  done
}

# ---------------------------------------------------------------------------
# init
# ---------------------------------------------------------------------------

ensure_sections() {
  local dir="$1" p s
  p="$(project_name)"
  mkdir -p "$dir"
  if [ ! -f "$dir/INDEX.md" ]; then
    project_index_template "$p" > "$dir/INDEX.md"
  elif ! grep -q 'conventions/INDEX.md' "$dir/INDEX.md"; then
    # An older flat project trove: keep its entries, add the section links.
    {
      printf '\n## Sections\n\n'
      printf -- '- [Conventions](conventions/INDEX.md) - how this repo is built and what never to do\n'
      printf -- '- [Memory](memory/INDEX.md) - decisions, gotchas and history, with the why\n'
      printf -- '- [Context](context/INDEX.md) - codebase map from the Trove engine\n'
    } >> "$dir/INDEX.md"
  fi
  for s in $SECTIONS; do
    mkdir -p "$dir/$s/entries"
    [ -n "$(ls -A "$dir/$s/entries" 2>/dev/null)" ] || : > "$dir/$s/entries/.gitkeep"
    [ -f "$dir/$s/INDEX.md" ] || section_index_template "$s" "$p" > "$dir/$s/INDEX.md"
  done
}

cmd_init() {
  local scope="${1:-}" dir existed=""
  if [ -z "$scope" ]; then
    if git -C "$root" rev-parse --git-dir >/dev/null 2>&1 || [ -n "${TROVE_ROOT:-}" ]; then
      scope="--project"
    else
      scope="--user"
    fi
  fi
  dir="$(resolve "$scope")"
  [ -f "$dir/INDEX.md" ] && existed=1
  if [ "$scope" = "--project" ]; then
    ensure_sections "$dir"
    render_all
    write_agents
  else
    mkdir -p "$dir/entries"
    [ -f "$dir/INDEX.md" ] || index_template > "$dir/INDEX.md"
  fi
  if [ -n "$existed" ]; then say "Trove already exists at $dir"; else say "Created trove at $dir"; fi
}

# ---------------------------------------------------------------------------
# render: section -> .claude/skills/<project>-<section>/SKILL.md
# ---------------------------------------------------------------------------

render_section() {
  local s="$1" p sdir skill out tmp count f rel title hook used=0 chunk
  local overflow=""
  p="$(project_name)"
  sdir="$proj_dir/$s"
  skill="$p-$s"
  out="$root/.claude/skills/$skill/SKILL.md"
  if [ ! -f "$sdir/INDEX.md" ]; then
    warn "No $s section at $sdir. Run: trove.sh init --project"
    return 1
  fi
  count="$(ordered_entries "$sdir" | wc -l | tr -d ' ')"
  tmp="$(mktemp)"
  chunk="$(mktemp)"
  {
    printf -- '---\n'
    printf 'name: %s\n' "$skill"
    printf 'description: "%s"\n' "$(skill_description "$p" "$s")"
    printf -- '---\n\n'
    printf '<!-- Generated by trove.sh render from .claude/trove/%s/. Do not edit; change the entries and re-render. -->\n\n' "$s"
    printf '# %s %s\n\n' "$p" "$s"
    printf '%s\n\n' "$(skill_intro "$s")"
    if [ "$s" = "context" ] && [ -f "$sdir/entries/architecture.md" ]; then
      printf 'Indexed: %s.\n\n' "$(fm_field indexed "$sdir/entries/architecture.md")"
    fi
    printf 'Source: `.claude/trove/%s/` (%s). Add or update entries with /trove:remember, or edit the files and run `trove.sh render`. Other agents find the same files through AGENTS.md.\n' "$s" "$(plural "$count" entry entries)"
  } > "$tmp"

  if [ "$count" = 0 ]; then
    if [ "$s" = "context" ]; then
      printf '\n_No context yet._ Run /trove:index to build the codebase map.\n' >> "$tmp"
    else
      printf '\n_No entries yet._ Capture one with /trove:remember.\n' >> "$tmp"
    fi
  else
    printf '\n## Entries\n' >> "$tmp"
    while IFS= read -r f; do
      rel=".claude/trove/$s/entries/$f"
      case "$f" in
        *.json)
          title="$(json_str title "$sdir/entries/$f")"
          hook="$(json_str summary "$sdir/entries/$f")"
          {
            printf '\n### %s\n\n' "${title:-${f%.json}}"
            [ -n "$hook" ] && printf '%s\n\n' "$hook"
            printf 'Structured data: `%s` (payload under `data`).\n' "$rel"
          } > "$chunk"
          ;;
        *)
          title="$(fm_field title "$sdir/entries/$f")"
          {
            printf '\n### %s\n\n' "${title:-${f%.md}}"
            md_body "$sdir/entries/$f"
            printf '\nSource: `%s`\n' "$rel"
          } > "$chunk"
          ;;
      esac
      used=$((used + $(wc -c < "$chunk")))
      if [ -z "$overflow" ] && [ "$used" -le "$MAX_BYTES" ]; then
        cat "$chunk" >> "$tmp"
      else
        if [ -z "$overflow" ]; then
          overflow=1
          printf '\n## More entries\n\nNot inlined to keep this skill small. Open the file when it looks relevant.\n\n' >> "$tmp"
        fi
        printf -- '- %s: `%s`\n' "${title:-$f}" "$rel" >> "$tmp"
      fi
    done <<EOF
$(ordered_entries "$sdir")
EOF
  fi
  rm -f "$chunk"

  # The rendered skill is loaded into context, so it gets the guard too.
  if ! guard_scan "$tmp" >/dev/null; then
    guard_redact "$tmp"
    warn "Redacted possible secrets while rendering $skill. Check .claude/trove/$s/entries with: trove.sh guard <file>"
  fi

  if write_if_changed "$tmp" "$out"; then
    say "Rendered $skill -> ${out#"$root"/}"
  else
    say "Unchanged $skill"
  fi
}

render_all() {
  local s status=0
  for s in $SECTIONS; do render_section "$s" || status=1; done
  return $status
}

cmd_render() {
  local what="${1:-all}"
  if [ "$what" = "all" ]; then
    render_all && write_agents
  elif is_section "$what"; then
    render_section "$what"
  else
    echo "usage: trove.sh render [conventions|memory|context|all]"; return 2
  fi
}

# ---------------------------------------------------------------------------
# AGENTS.md pointer so non-Claude agents find the same files
# ---------------------------------------------------------------------------

agents_block() {
  local p="$1"
  cat <<EOF
<!-- trove:start -->
## Project trove

This repository keeps a project trove for coding agents in \`.claude/trove/\`, rendered as skills under
\`.claude/skills/\`. Every agent (Claude Code, Codex, Cursor, pi and others) should read the matching
file before working here:

- **Conventions**: \`.claude/skills/$p-conventions/SKILL.md\` (source \`.claude/trove/conventions/\`). How this repo is built and what never to do.
- **Memory**: \`.claude/skills/$p-memory/SKILL.md\` (source \`.claude/trove/memory/\`). Decisions, gotchas and history, with the why.
- **Context**: \`.claude/skills/$p-context/SKILL.md\` (source \`.claude/trove/context/\`). Architecture, subsystems and key modules.

To add an entry, write one file under the matching \`.claude/trove/<section>/entries/\`, add a line to
that section's \`INDEX.md\`, then run \`trove.sh render\` from the Trove plugin. Never store secrets.
<!-- trove:end -->
EOF
}

write_agents() {
  local f="$root/AGENTS.md" tmp blk
  tmp="$(mktemp)"
  blk="$(mktemp)"
  agents_block "$(project_name)" > "$blk"
  if [ ! -f "$f" ]; then
    { printf '# AGENTS.md\n\n'; cat "$blk"; } > "$tmp"
  elif grep -q '<!-- trove:start -->' "$f"; then
    awk -v blk="$blk" '
      /<!-- trove:start -->/ { while ((getline l < blk) > 0) print l; skip = 1; next }
      skip && /<!-- trove:end -->/ { skip = 0; next }
      !skip { print }
    ' "$f" > "$tmp"
  else
    { cat "$f"; printf '\n'; cat "$blk"; } > "$tmp"
  fi
  rm -f "$blk"
  if write_if_changed "$tmp" "$f"; then say "Updated AGENTS.md"; fi
  return 0
}

# ---------------------------------------------------------------------------
# context: engine output (.trove/) -> context section
# ---------------------------------------------------------------------------

JSON_TOOL=""
json_tool() {
  [ -n "$JSON_TOOL" ] && return 0
  case "${TROVE_JSON:-}" in
    jq|python3) JSON_TOOL="$TROVE_JSON" ;;
    *)
      if command -v jq >/dev/null 2>&1; then JSON_TOOL=jq
      elif command -v python3 >/dev/null 2>&1; then JSON_TOOL=python3
      fi ;;
  esac
  [ -n "$JSON_TOOL" ]
}

# jget <mode> <file>: modes raw | path | modrow | field:<name>
jget() {
  local mode="$1" f="$2"
  if [ "$JSON_TOOL" = jq ]; then
    case "$mode" in
      raw)     jq -r '.text.raw // .text.summary // ""' "$f" ;;
      path)    jq -r '.path // .id // ""' "$f" ;;
      modrow)  jq -r '[((.symbols // []) | length), (.path // .id), ((.text.keywords // []) | join(", "))] | @tsv' "$f" ;;
      field:*) jq -r --arg k "${mode#field:}" '.[$k] // "" | tostring' "$f" ;;
    esac
  else
    python3 - "$mode" "$f" <<'PY'
import json, sys
mode, path = sys.argv[1], sys.argv[2]
with open(path) as fh:
    d = json.load(fh)
t = d.get("text") or {}
if mode == "raw":
    print(t.get("raw") or t.get("summary") or "")
elif mode == "path":
    print(d.get("path") or d.get("id") or "")
elif mode == "modrow":
    print("%d\t%s\t%s" % (len(d.get("symbols") or []), d.get("path") or d.get("id"), ", ".join(t.get("keywords") or [])))
elif mode.startswith("field:"):
    v = d.get(mode[6:], "")
    print("" if v is None else v)
PY
  fi
}

slugify() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | sed -E 's/[^a-z0-9]+/-/g; s/^-+//; s/-+$//'
}

# Engine summaries use H1/H2; drop the H1 (the entry title covers it) and cap long module lists.
shape_engine_text() {
  awk -v cap="${1:-25}" '
    NR == 1 && /^# / { next }
    /^## / { inmods = ($0 ~ /^## Modules/); n = 0; print; next }
    inmods && /^- / { n++; if (n <= cap) print; else more++; next }
    { if (more) { print "- ... and " more " more"; more = 0 } print }
    END { if (more) print "- ... and " more " more" }
  ' | awk 'NF { started = 1 } started'
}

cmd_context() {
  local tdir="$root/.trove" ctx indexed sym mods subs f path slug raw tmp lines
  if [ ! -f "$tdir/manifest.json" ]; then
    warn "No engine index at $tdir. Run the indexer first (/trove:index)."
    return 1
  fi
  if ! json_tool; then
    warn "trove.sh context needs jq or python3 to read .trove/."
    return 1
  fi
  ensure_sections "$proj_dir"
  ctx="$proj_dir/context"

  indexed="$(jget field:indexed_at "$tdir/manifest.json")"
  sym="$(jget field:symbol_count "$tdir/manifest.json")"
  mods="$(jget field:module_count "$tdir/manifest.json")"
  subs="$(jget field:subsystem_count "$tdir/manifest.json")"
  indexed="${indexed%%.*}"

  # Generated entries are replaced wholesale; hand-written context entries stay.
  for f in "$ctx/entries"/*.md; do
    [ -f "$f" ] && grep -q "^$GENERATED_MARK\$" "$f" && rm -f "$f"
  done

  lines="$(mktemp)"

  if [ -f "$tdir/architecture/architecture_root.json" ]; then
    raw="$(jget raw "$tdir/architecture/architecture_root.json")"
    {
      printf -- '---\ntitle: Architecture\nslug: architecture\ntype: context\n%s\nindexed: %s\n---\n\n' "$GENERATED_MARK" "$indexed"
      printf '%s symbols in %s modules across %s subsystems.\n\n' "$sym" "$mods" "$subs"
      printf '%s\n' "$raw" | shape_engine_text 25
    } > "$ctx/entries/architecture.md"
    printf -- '- [Architecture](entries/architecture.md) - %s symbols in %s modules across %s subsystems\n' "$sym" "$mods" "$subs" >> "$lines"
  fi

  for f in "$tdir"/subsystems/*.json; do
    [ -f "$f" ] || continue
    path="$(jget path "$f")"
    slug="subsystem-$(slugify "$path")"
    raw="$(jget raw "$f")"
    {
      printf -- '---\ntitle: "Subsystem: %s"\nslug: %s\ntype: context\n%s\nindexed: %s\n---\n\n' "$path" "$slug" "$GENERATED_MARK" "$indexed"
      printf '%s\n' "$raw" | shape_engine_text 25
    } > "$ctx/entries/$slug.md"
    printf -- '- [Subsystem: %s](entries/%s.md) - %s\n' "$path" "$slug" \
      "$(printf '%s\n' "$raw" | sed -nE 's/^(Files|Symbols): ([0-9]+)$/\2 \1/p' | tr '[:upper:]' '[:lower:]' | paste -sd, - | sed 's/,/, /g')" >> "$lines"
  done

  if [ -d "$tdir/modules" ]; then
    tmp="$(mktemp)"
    for f in "$tdir"/modules/*.json; do
      [ -f "$f" ] && jget modrow "$f"
    done | awk -F'\t' '$1 > 0' | sort -t '	' -k1,1nr -k2,2 | head -n 20 > "$tmp"
    if [ -s "$tmp" ]; then
      {
        printf -- '---\ntitle: Key modules\nslug: key-modules\ntype: context\n%s\nindexed: %s\n---\n\n' "$GENERATED_MARK" "$indexed"
        printf 'The modules with the most symbols, a good place to start reading.\n\n'
        awk -F'\t' '{ printf "- `%s` (%s symbols)%s\n", $2, $1, ($3 != "" ? ": " $3 : "") }' "$tmp"
      } > "$ctx/entries/key-modules.md"
      printf -- '- [Key modules](entries/key-modules.md) - the %s modules with the most symbols\n' "$(wc -l < "$tmp" | tr -d ' ')" >> "$lines"
    fi
    rm -f "$tmp"
  fi

  # Replace the generated block in context/INDEX.md, keeping hand-written lines.
  tmp="$(mktemp)"
  if ! grep -q '<!-- generated:start' "$ctx/INDEX.md"; then
    awk '
      /<!-- entries below -->/ && !done {
        print "<!-- generated:start (written by trove.sh context, do not edit by hand) -->"
        print "<!-- generated:end -->"; print ""; done = 1
      }
      { print }
      END { if (!done) { print ""; print "<!-- generated:start (written by trove.sh context, do not edit by hand) -->"; print "<!-- generated:end -->" } }
    ' "$ctx/INDEX.md" > "$tmp" && mv "$tmp" "$ctx/INDEX.md"
  fi
  awk -v gen="$lines" '
    /<!-- generated:start/ { print; while ((getline l < gen) > 0) print l; skip = 1; next }
    /<!-- generated:end -->/ { skip = 0; print; next }
    !skip { print }
  ' "$ctx/INDEX.md" > "$tmp" && mv "$tmp" "$ctx/INDEX.md"
  rm -f "$lines"

  say "Refreshed context from $tdir ($sym symbols, $subs subsystems)"
  render_section context
}

# ---------------------------------------------------------------------------
# route: entry type -> section (used by /trove:remember)
# ---------------------------------------------------------------------------

cmd_route() {
  case "$(printf '%s' "${1:-}" | tr '[:upper:]' '[:lower:]')" in
    convention|conventions|preference|style|rule|naming|workflow) echo conventions ;;
    context|architecture|map|subsystem|module|overview) echo context ;;
    decision|gotcha|history|incident|project|reference|snippet|memory|"") echo memory ;;
    *) echo memory ;;
  esac
}

# ---------------------------------------------------------------------------

cmd="${1:-}"; shift || true

case "$cmd" in
  path)
    resolve "${1:-}"
    ;;
  init)
    cmd_init "${1:-}"
    ;;
  list)
    dir="$(resolve "${1:-}")"
    if [ -f "$dir/INDEX.md" ]; then
      cat "$dir/INDEX.md"
      for s in $SECTIONS; do
        [ -f "$dir/$s/INDEX.md" ] && { printf '\n'; cat "$dir/$s/INDEX.md"; }
      done
    else
      echo "No trove found. Run: trove.sh init"
    fi
    ;;
  grep)
    dir="$(resolve)"
    term="${1:-}"
    if [ -z "$term" ]; then echo "usage: trove.sh grep <term>"; exit 2; fi
    set --
    [ -d "$dir/entries" ] && set -- "$dir/entries"
    for s in $SECTIONS; do [ -d "$dir/$s/entries" ] && set -- "$@" "$dir/$s/entries"; done
    if [ $# -eq 0 ]; then echo "No matches."; exit 0; fi
    grep -rin --include='*.md' --include='*.json' -e "$term" "$@" 2>/dev/null || echo "No matches."
    ;;
  route)
    cmd_route "${1:-}"
    ;;
  guard)
    cmd_guard "$@"
    ;;
  render)
    cmd_render "${1:-all}"
    ;;
  context)
    cmd_context
    ;;
  sync)
    if [ -f "$root/.trove/manifest.json" ]; then cmd_context || true; fi
    cmd_render all
    ;;
  agents)
    write_agents
    ;;
  project)
    project_name
    ;;
  *)
    echo "usage: trove.sh {path|init|list|grep|route|guard|render|context|sync|agents|project} [args]"
    exit 2
    ;;
esac
