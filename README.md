<p align="center"><img src="logo.svg" alt="Trove" width="88" height="88"></p>

# Trove

**Beyond context windows.**

Trove makes LLMs operate over repositories and knowledge bases far larger than their native context
window. Not by stuffing more tokens into the prompt, but by indexing everything once and retrieving
only what matters, at the right level of detail.

Live page: **https://anishfyi.com/trove/**

---

## Two layers, one system

| Layer | What it is | Where it lives |
|-------|-----------|----------------|
| **Engine** | Symbol index, module/subsystem/architecture summaries, dependency graph, progressive retrieval | `.trove/` in any repo (Rust CLI) |
| **Plugin** | Personal engineering memory: decisions, gotchas, conventions | `~/.claude/trove` or `./.claude/trove` (Claude Code) |

The engine answers "what is in this codebase and where?" The plugin answers "what did we decide and
why?" Together they give Claude both structural repo knowledge and durable session memory.

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
│   └── trove-cli/              # trove binary
├── plugins/trove/
│   ├── skills/                 # init, remember, recall, index, query
│   ├── hooks/                  # SessionStart load + SessionEnd reflect
│   └── scripts/                # load-trove.sh, reflect-trove.sh, trove.sh
├── index.html                  # landing page
├── PRD.md
└── ENGINEERING.md
```

## Docs

- [Vision: Beyond Context Windows](VISION.md)
- [Product Requirements (v0.1 plugin)](PRD.md)
- [Engineering Design](ENGINEERING.md)

---

Built by [anishfyi](https://github.com/anishfyi). MIT licensed.
