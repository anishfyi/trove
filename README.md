<p align="center">
  <img src="logo.svg" alt="Trove: a sprout growing in a terracotta pot" width="160" height="160">
</p>

<h1 align="center">Trove</h1>

<p align="center"><strong>An open-source memory, semantic and context layer for coding agents.</strong></p>

<p align="center">
  <a href="https://github.com/anishfyi/trove/releases/latest"><img src="https://img.shields.io/github/v/release/anishfyi/trove?color=c0613a&label=release" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/anishfyi/trove?color=5b9c6a" alt="MIT license"></a>
  <a href="https://anishfyi.com/trove/"><img src="https://img.shields.io/badge/site-anishfyi.com%2Ftrove-2f6f6a" alt="Live site"></a>
</p>

Coding agents forget everything between sessions and only see what fits in their context window.
Trove gives them a memory that lasts. It indexes a repository once into layered memory, keeps the
decisions, conventions and gotchas your team learns along the way, and hands an agent only the
context a task needs, at the right level of detail.

- **Memory**: durable entries for decisions, gotchas, conventions and references, personal or per
  project, reloaded every session.
- **Semantics**: a symbol index, module and subsystem summaries, an architecture map and a
  dependency graph, written to `.trove/`.
- **Context**: progressive retrieval from architecture down to symbols, inside a token budget.

Everything is plain files in your repo or home directory. It runs locally, works offline and is
MIT licensed.

Live page: **https://anishfyi.com/trove/**

---

## What is new in v0.3

v0.3 turns Trove from a Claude Code plugin with an engine into a layer any coding agent can use.
The epic is [#19](https://github.com/anishfyi/trove/issues/19); each track landed as its own PR.

| Track | Issue | PR | Status |
|-------|-------|----|--------|
| **Project troves**: conventions, memory and context rendered as skills that load on intent | [#20](https://github.com/anishfyi/trove/issues/20) | [#26](https://github.com/anishfyi/trove/pull/26) | Merged |
| **Dense CLI**: `trove ask` across every trove and doc, `trove tools` for installed CLIs | [#21](https://github.com/anishfyi/trove/issues/21) | [#27](https://github.com/anishfyi/trove/pull/27) | Merged |
| **`trove ship`**: handoff docs with secrets and private info stripped | [#22](https://github.com/anishfyi/trove/issues/22) | [#28](https://github.com/anishfyi/trove/pull/28) | Merged |
| **Website**: redesign, link previews and sitemap | [#23](https://github.com/anishfyi/trove/issues/23) | [#25](https://github.com/anishfyi/trove/pull/25) | Merged |
| **macOS and Linux app**: one place for your work memory across projects and agents | [#24](https://github.com/anishfyi/trove/issues/24) | | Coming soon |

---

## Two layers, one system

| Layer | What it is | Where it lives |
|-------|-----------|----------------|
| **Engine** | Symbol index, module/subsystem/architecture summaries, dependency graph, progressive retrieval | `.trove/` in any repo (Rust CLI) |
| **Plugin** | Engineering memory: decisions, gotchas, conventions | `~/.claude/trove` or `./.claude/trove` (Claude Code) |

The engine answers "what is in this codebase and where?" The plugin answers "what did we decide and
why?" Together they give an agent both structural repo knowledge and durable session memory.

See [VISION.md](VISION.md) for the full memory hierarchy design (L0-L6).

---

## Quick start: engine

Requires Rust. From any repository:

```bash
git clone https://github.com/anishfyi/trove
cd trove && cargo build -p trove-cli

# Index the current repo (writes .trove/, gitignored)
cargo run -p trove-cli -- index

# Check what was indexed
cargo run -p trove-cli -- status

# Retrieve context for a task (architecture → subsystem → module → symbol)
cargo run -p trove-cli -- query "modify vendor onboarding"
```

Other commands: `import-historical`, `record`, `patch`. Run `cargo run -p trove-cli -- --help` for
the full list.

### What gets indexed

- **L1 Symbols**: functions, classes, structs, traits, tests (Rust, Python, JS/TS, Go, Bash)
- **L2 Modules**: per-file summaries: exports, dependencies, assumptions, side effects
- **L3 Subsystems**: package-level clusters
- **L4 Architecture**: repo-wide design map and data flow
- **L5 Historical**: imported from your Claude trove entries on demand

Retrieval stops as soon as it has enough detail. Unused context budget is healthy.

---

## Quick start: Claude Code plugin

Run these **one at a time** inside Claude Code:

**1. Add the marketplace**

```
/plugin marketplace add anishfyi/trove
```

> SSH error? Use HTTPS: `/plugin marketplace add https://github.com/anishfyi/trove.git`

**2. Install the plugin**

```
/plugin install trove@velofy-trove
```

**3. Reload**

```
/reload-plugins
```

**4. Create your trove**

```
/trove:init
```

Inside a repository this creates a **project trove** with conventions, memory and context sections,
rendered as skills. Elsewhere it creates a personal trove at `~/.claude/trove`.

**5. Map the codebase** (project trove)

```
/trove:index
```

From here: `/trove:remember` to capture, `/trove:recall` to search, `/trove:index` to re-index and
refresh the context skill, `/trove:query` to retrieve layered context.

### Plugin commands

| Command | What it does |
|---------|--------------|
| `/trove:init` | Scaffold a project trove (`./.claude/trove`, with sections and skills) or `~/.claude/trove` |
| `/trove:remember` | Capture one durable learning, routed to conventions, memory or context, with a secret guard |
| `/trove:recall` | Search entries and answer with citations |
| `/trove:index` | Index the repo into Trove memory layers and refresh the context skill |
| `/trove:render` | Re-render the project trove sections into skills |
| `/trove:query` | Progressive retrieval with a context budget |

Skills auto-trigger on intent: "remember this", "what do I know about X", "index this repo".

Every session, the SessionStart hook loads your `INDEX.md` files so Claude starts aware of past
decisions, and re-renders any project trove skill whose entries changed (a hand edit or a `git pull`).

### Project troves: conventions, memory and context as skills

Every coding project gets its own trove under `.claude/trove/`, split into three sections. Each
section is rendered into a Claude Code skill whose description is tuned to load on intent:

| Section | Holds | Skill | Loads when |
|---------|-------|-------|------------|
| `conventions/` | Style, structure, naming, testing, commit and PR rules, things to never do | `<project>-conventions` | writing, editing, reviewing or committing code |
| `memory/` | Decisions, gotchas and history, with the why | `<project>-memory` | changing behavior, picking an approach, debugging, asking why |
| `context/` | Architecture, subsystems and key modules from the engine | `<project>-context` | orienting, locating code, planning cross-module changes |

```
.claude/
├── trove/
│   ├── INDEX.md                    # links to the three sections
│   ├── conventions/INDEX.md + entries/
│   ├── memory/INDEX.md + entries/
│   └── context/INDEX.md + entries/ # generated entries refreshed by /trove:index
└── skills/
    ├── <project>-conventions/SKILL.md
    ├── <project>-memory/SKILL.md
    └── <project>-context/SKILL.md
AGENTS.md                           # trove block so Codex, Cursor and pi find the same files
```

`<project>` is the repository folder name in lowercase kebab case. Commit `.claude/trove/`,
`.claude/skills/` and `AGENTS.md` to share them with your team and every agent. The generated
`SKILL.md` files are never edited by hand: change the entries and re-render.

**Keeping them in sync.** `/trove:remember` writes the entry to the right section, updates that
section's `INDEX.md` and re-renders its skill. `/trove:index` runs the engine and then rebuilds the
generated context entries (hand-written context entries are kept). `/trove:recall` searches all three.

**Secret guard.** Every write runs a mechanical scan for credential shapes (`sk-`, `AKIA`, `ghp_`,
`shpat_`, `xox?-`, private key blocks, `key=`/`token=`/`password:` assignments). A hit refuses the
entry (or redacts it with `--redact`), and rendering redacts anything that slipped through.

The same steps work without Claude Code through the bundled script:

```bash
T=plugins/trove/scripts/trove.sh
bash $T init --project          # sections, skills and the AGENTS.md block
bash $T guard <entry-file>      # exit 1 on a possible secret; --redact to scrub it
bash $T render [section|all]    # sections -> .claude/skills/<project>-<section>/SKILL.md
bash $T context                 # .trove/ engine output -> context section and skill (needs jq or python3)
bash $T sync                    # context (when indexed) plus render all
```

Tests for the generator: `bash plugins/trove/tests/render.test.sh`.

### Manual install (skills only, no hooks)

```bash
git clone https://github.com/anishfyi/trove /tmp/trove
cp -r /tmp/trove/plugins/trove/skills/* ~/.claude/skills/
```

---

## Personal trove format

Each entry is one file. Claude picks **Markdown** for prose or **JSON** for structured data.

```
~/.claude/trove/
├── INDEX.md                 # one line per entry, newest first
└── entries/
    ├── use-pgx-not-orm.md
    ├── service-ports.json
    └── staging-db-mirror.md
```

A project trove uses the same entry format inside each section's `entries/`.

Entry types: `convention`, `decision`, `gotcha`, `preference`, `reference`, `project`, `snippet`, `context`.

**Save:** decisions and rationale, gotchas, conventions, external references, non-obvious constraints.

**Skip:** secrets, transient state, anything trivially re-derivable from code or git history.

---

## Repository layout

```
trove/
├── Cargo.toml                  # Rust workspace
├── VISION.md                   # memory hierarchy design
├── crates/
│   ├── trove-core/             # MemoryObject, compression, L0-L6 types
│   ├── trove-index/            # tree-sitter symbol index, dependency graph
│   ├── trove-retrieve/         # progressive retrieval + context budget
│   ├── trove-cli/              # trove binary, with ask and tools (#27)
│   ├── trove-tools/            # installed CLI catalog for trove tools (#27)
│   └── trove-ship/             # handoff docs with redaction (#28)
├── plugins/trove/
│   ├── skills/                 # init, remember, recall, index, query, render (#26), ship (#28)
│   ├── hooks/                  # SessionStart load + SessionEnd reflect
│   ├── scripts/                # load-trove.sh, reflect-trove.sh, trove.sh
│   └── tests/                  # render.test.sh for the project trove skills (#26)
├── .claude-plugin/             # marketplace.json (velofy-trove)
├── index.html                  # landing page
├── logo.svg                    # animated icon, also the favicon
├── PRD.md
└── ENGINEERING.md
```

Entries marked with a PR number landed in that v0.3 PR.

## Docs

- [Vision: Beyond Context Windows](VISION.md)
- [Product Requirements (v0.1 plugin)](PRD.md)
- [Engineering Design](ENGINEERING.md)

---

Built by [anishfyi](https://github.com/anishfyi). MIT licensed.
