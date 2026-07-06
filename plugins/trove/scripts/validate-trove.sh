#!/usr/bin/env bash
# validate-trove.sh: deterministic lint of a trove. Code checks what code can
# check, so no model judgment (and no model hallucination) is involved; the
# semantic pass (contradictions, staleness) belongs to /trove:audit's subagent.
#
# Usage: validate-trove.sh [trove-dir] [--report <file>]
#   trove-dir defaults to the same resolution as trove.sh: project trove
#   (./.claude/trove) if present, else user trove (~/.claude/trove).
#   --report writes findings as markdown to <file> when issues exist, and
#   removes <file> when the trove is clean.
#
# Exit codes: 0 clean (notes allowed), 1 issues found, 2 usage or no trove.

set -u

TROVE=""
REPORT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --report)
      REPORT="${2:-}"
      [ -z "$REPORT" ] && { echo "usage: validate-trove.sh [trove-dir] [--report <file>]" >&2; exit 2; }
      shift 2 ;;
    -*)
      echo "usage: validate-trove.sh [trove-dir] [--report <file>]" >&2; exit 2 ;;
    *)
      TROVE="$1"; shift ;;
  esac
done

if [ -z "$TROVE" ]; then
  if [ -f "$PWD/.claude/trove/INDEX.md" ]; then
    TROVE="$PWD/.claude/trove"
  else
    TROVE="$HOME/.claude/trove"
  fi
fi

if [ ! -f "$TROVE/INDEX.md" ]; then
  echo "no trove at $TROVE" >&2
  exit 2
fi

ISSUES=""
NOTES=""
issue() { ISSUES="${ISSUES}- $1
"; }
note() { NOTES="${NOTES}- $1
"; }

frontmatter_field() { # file key -> value
  # Tolerant of CRLF line endings, tabs or extra spaces after the colon, and
  # trailing whitespace on the value: none of those make a fact false.
  awk -v key="$2" '
    { sub(/\r$/, "") }
    NR==1 && $0!="---" { exit }
    NR>1 && $0=="---" { exit }
    NR>1 {
      if (match($0, "^" key ":[ \t]*")) {
        val = substr($0, RLENGTH + 1)
        gsub(/[ \t]+$/, "", val)
        print val
        exit
      }
    }
  ' "$1"
}

json_ok() { # file -> 0 if parseable, 1 if not, 2 if no checker available
  if command -v jq > /dev/null 2>&1; then
    jq . "$1" > /dev/null 2>&1 && return 0
    return 1
  elif command -v python3 > /dev/null 2>&1; then
    python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$1" > /dev/null 2>&1 && return 0
    return 1
  else
    return 2
  fi
}

# Every index line must point at an existing file (entries can be nested,
# e.g. entries/<dir>/00-README.md, so check any depth). Anchor on the
# markdown-link form and strip any #fragment; tr -d would mangle
# paren-bearing names.
while IFS= read -r target; do
  [ -e "$TROVE/$target" ] || issue "INDEX.md links to missing file: $target"
done < <(grep -o '](entries/[^)]*)' "$TROVE/INDEX.md" 2>/dev/null | sed 's/^](//; s/)$//; s/#.*$//')

# Top-level entries must be indexed and well-formed. Files nested deeper are
# parts of a multi-file entry; only their indexed root is checked.
for f in "$TROVE/entries"/*.md "$TROVE/entries"/*.json; do
  [ -e "$f" ] || continue
  base="$(basename "$f")"
  name="${base%.*}"

  grep -qF "entries/$base" "$TROVE/INDEX.md" || issue "entries/$base: not listed in INDEX.md"

  case "$f" in
    *.md)
      if [ "$(head -n 1 "$f" | tr -d '\r')" != "---" ]; then
        issue "entries/$base: no frontmatter block"
        continue
      fi
      for key in title slug type created; do
        val="$(frontmatter_field "$f" "$key")"
        if [ -z "$val" ]; then
          issue "entries/$base: missing frontmatter field '$key'"
        elif [ "$key" = "slug" ] && [ "$val" != "$name" ]; then
          issue "entries/$base: slug '$val' does not match filename"
        elif [ "$key" = "created" ] && ! printf '%s' "$val" | grep -Eq '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'; then
          issue "entries/$base: created '$val' is not a YYYY-MM-DD date"
        fi
      done
      ;;
    *.json)
      json_ok "$f"
      case $? in
        1) issue "entries/$base: invalid JSON" ;;
        2) note "entries/$base: JSON not checked (no jq or python3)" ;;
        *)
          for key in title slug created summary; do
            grep -q "\"$key\"" "$f" || issue "entries/$base: missing key \"$key\""
          done
          ;;
      esac
      ;;
  esac
done

# Dangling [[links]] mark future entries by convention: note, not issue.
for f in "$TROVE/entries"/*.md; do
  [ -e "$f" ] || continue
  base="$(basename "$f")"
  while IFS= read -r link; do
    slug="${link#[[}"
    slug="${slug%]]}"
    if [ ! -e "$TROVE/entries/$slug.md" ] && [ ! -e "$TROVE/entries/$slug.json" ] && [ ! -d "$TROVE/entries/$slug" ]; then
      note "entries/$base: [[${slug}]] not written yet"
    fi
  done < <(grep -o '\[\[[a-z0-9-][a-z0-9-]*\]\]' "$f" 2>/dev/null)
done

status=0
if [ -n "$ISSUES" ]; then
  status=1
  printf 'Trove validation: issues in %s\n\n%s' "$TROVE" "$ISSUES"
else
  printf 'Trove validation: clean (%s)\n' "$TROVE"
fi
[ -n "$NOTES" ] && printf '\nNotes (no action forced):\n%s' "$NOTES"

if [ -n "$REPORT" ]; then
  if [ "$status" -eq 1 ]; then
    mkdir -p "$(dirname "$REPORT")"
    {
      printf '# Trove validation report\n\n'
      printf -- '- generated: %s\n- trove: %s\n\n' "$(date +%Y-%m-%d)" "$TROVE"
      printf '## Issues\n\n%s' "$ISSUES"
      [ -n "$NOTES" ] && printf '\n## Notes\n\n%s' "$NOTES"
      printf '\nResolve with /trove:audit, then delete this file.\n'
    } > "$REPORT"
  else
    rm -f "$REPORT"
  fi
fi

exit "$status"
