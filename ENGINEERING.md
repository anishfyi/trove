# Trove - Engineering Design Document

**Author:** anishfyi
**Status:** v0.1 (shipped)
**Last updated:** 2026-06-13
**Companion:** [PRD.md](PRD.md)

---

## 1. Overview

Trove is a Claude Code **plugin** distributed through a **marketplace** in a public GitHub repo. It
contributes three **skills** (`init`, `remember`, `recall`) and one **SessionStart hook**. There is
no runtime service: state is plain markdown on the user's disk. The "intelligence" lives in the skill
instructions that Claude follows; the plugin is the delivery and lifecycle mechanism.

## 2. Architecture

```
                 install via marketplace
  GitHub repo  ───────────────────────────▶  Claude Code
  (this repo)                                   │
                                                ├─ skills/  init · remember · recall
                                                └─ hooks/   SessionStart -> load-trove.sh
                                                                 │
                                                                 ▼
                                  ~/.claude/trove/        (or ./.claude/trove)
                                    ├─ INDEX.md           one line per entry
                                    └─ entries/*.md       one fact per file
```

Data flows one way at session start (trove -> context via the hook) and on demand during a session
(user intent -> skill -> read/write trove).

## 3. Repository and plugin layout

```
trove/
├── .claude-plugin/marketplace.json     # marketplace "anishfyi-trove", lists the plugin
└── plugins/trove/
    ├── .claude-plugin/plugin.json      # plugin identity, kebab-case name "trove"
    ├── skills/{init,remember,recall}/SKILL.md
    ├── hooks/hooks.json                # SessionStart matcher startup|resume
    └── scripts/{load-trove.sh,trove.sh}
```

Plugin skills are namespaced by the plugin name, so the commands are `/trove:init`,
`/trove:remember`, `/trove:recall`. The marketplace name is referenced at install time:
`/plugin install trove@anishfyi-trove`.

## 4. Storage format

### Location resolution
- **Project scope** `./.claude/trove` wins if it has an `INDEX.md` (more specific, travels with repo).
- **User scope** `~/.claude/trove` is the default and fallback (follows the user everywhere).

This single rule is implemented identically in `trove.sh`, `load-trove.sh`, and stated in every
SKILL.md so all paths agree.

### INDEX.md
A human-readable table of contents. One bullet per entry, newest first:
`- [Title](entries/slug.md) - one-line hook`. This file is what the SessionStart hook injects, so it
must stay compact.

### entries/<slug>.md
One atomic fact per file. YAML frontmatter (`title`, `slug`, `type`, `created`, `tags`) plus a short
body, a `Why it matters` line, and `[[slug]]` cross-links. `type` is an enum:
`decision | gotcha | preference | reference | project | snippet`.

Rationale for one-file-per-fact: atomic diffs, trivial dedupe and deletion, clean git history, and a
small context cost when only the index is loaded by default.

## 5. The SessionStart hook

`hooks/hooks.json` registers a `command` hook on `SessionStart` (matcher `startup|resume`) that runs
`load-trove.sh` via `${CLAUDE_PLUGIN_ROOT}`. The script:

- is **read-only** and **defensive**: it never fails a session (no `set -e`, guards every read,
  always `exit 0`),
- emits a short instruction line plus the project and/or user `INDEX.md`,
- prints nothing if no trove exists yet (zero noise for users who have not run `init`).

Only the compact index is auto-loaded, never every entry, to keep the per-session context cost low.
Full entries are pulled on demand by the recall skill.

## 6. The skills

Skills are markdown instructions, not code. Each declares `allowed-tools` to scope its permissions:

- **init** (`Bash, Read, Write, Edit`): choose scope, scaffold the directory and `INDEX.md`, print the
  canonical entry format so the other skills inherit one contract.
- **remember** (`Bash, Read, Write, Edit`): resolve the trove, distill to one atomic fact, dedupe
  against the index, write the entry, update the index newest-first, confirm with the path. Carries
  the "what to save vs skip" policy, including the hard "never store secrets" rule.
- **recall** (`Bash, Read, Grep`): resolve the trove, scan the index, grep entries, open the matches,
  answer grounded with citations, flag staleness, and offer to remember on a miss. Intentionally has
  no write tools.

Descriptions are written keyword-first so Claude auto-triggers the right skill on natural intent
("remember this", "what do I know about X") without the user typing the command.

## 7. The CLI helper

`trove.sh` is a dependency-free bash helper (`path`, `init`, `list`, `grep`) for mechanical work and
for users who installed skills manually without the plugin. The skills can lean on it but do not
require it; they can also operate the filesystem directly, so the system degrades gracefully if the
env var or script path is unavailable.

## 8. Distribution and versioning

- `marketplace.json` (`name: anishfyi-trove`) catalogs the single plugin with a relative
  `source: ./plugins/trove`.
- `plugin.json` carries identity and `version: 0.1.0`.
- Users add the marketplace with `/plugin marketplace add anishfyi/trove` and install with
  `/plugin install trove@anishfyi-trove`. `claude plugin validate` is used in CI/manual checks before
  publishing.

## 9. Security and privacy

- **No network, no telemetry.** Nothing leaves the machine.
- **Read-only auto-load.** The only automatic behavior injects text into context; it never writes.
- **Secrets policy.** The remember skill forbids storing credentials; project-scope codices are
  committed to git, so this is enforced at the instruction layer.
- **Least privilege.** Recall has no write tools; each skill's `allowed-tools` is minimal.

## 10. Testing and validation

- `claude plugin validate ./plugins/trove` and `claude plugin validate .` for schema correctness.
- Manual lifecycle test: init -> remember (twice, including a dedupe) -> recall -> open a new session
  and confirm the hook injects the index.
- `load-trove.sh` is verified to print nothing and exit 0 when no trove exists.

## 11. Future work

- `prune` / `review` skills for retiring stale entries.
- An opt-in session-end capture hook that proposes entries from the transcript.
- `export` / `import` to move a trove between machines as one bundle.
- Recall ranking by tag/type and recency.

## 12. Trade-offs

- **Markdown over a database**: less queryable, but portable, inspectable, diffable, and zero-dependency.
  The right call for a user-owned tool.
- **Index-only auto-load**: cheaper context but a coarser first impression; recall fills the gap on demand.
- **Instruction-driven over deterministic code**: more flexible and adaptive, at the cost of being only
  as reliable as the model following the skill. The CLI helper backstops the mechanical parts.

---

*Companion document: [Product Requirements Doc](PRD.md).*

