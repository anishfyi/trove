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

**4. Create your personal trove**

```
/trove:init
```

From here: `/trove:remember` to capture, `/trove:recall` to search, `/trove:index` to index the
repo, `/trove:query` to retrieve layered context.

### Plugin commands

| Command | What it does |
|---------|--------------|
| `/trove:init` | Scaffold `~/.claude/trove` or `./.claude/trove` |
| `/trove:remember` | Capture one durable learning as an atomic entry |
| `/trove:recall` | Search entries and answer with citations |
| `/trove:index` | Index the repo into Trove memory layers |
| `/trove:query` | Progressive retrieval with a context budget |

Skills auto-trigger on intent: "remember this", "what do I know about X", "index this repo".

Every session, the SessionStart hook loads your `INDEX.md` so Claude starts aware of past decisions.

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

Entry types: `decision`, `gotcha`, `preference`, `reference`, `project`, `snippet`.

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
