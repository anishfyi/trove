<p align="center"><img src="logo.svg" alt="Trove logo, a sprout in a pot" width="88" height="88"></p>

# Trove

**Beyond context windows.** Repository memory for LLMs: a Rust index engine and a Claude Code memory plugin.

[![Release](https://img.shields.io/github/v/release/anishfyi/trove)](https://github.com/anishfyi/trove/releases/latest)
[![License: MIT](https://img.shields.io/github/license/anishfyi/trove)](LICENSE)

**Documentation: https://velofy.co/trove/**

Trove helps LLMs work over repositories and knowledge bases larger than their context window. Not by
stuffing more tokens into the prompt, but by indexing everything once and retrieving only what
matters, at the right level of detail.

---

## Two layers, one system

| Layer | What it is | Where it lives |
|-------|-----------|----------------|
| **Engine** | Rust CLI: symbol index, summaries, dependency graph, progressive retrieval | `.trove/` in the indexed repo |
| **Plugin** | Claude Code memory: decisions, gotchas, conventions | `~/.claude/trove` or `./.claude/trove` |

The engine answers "what is in this codebase and where?" The plugin answers "what did we decide and
why?" Together they give Claude both structural repo knowledge and durable session memory.

---

## Quick start: engine

Requires Rust. There are no prebuilt binaries yet.

```bash
git clone https://github.com/anishfyi/trove
cd trove && cargo build -p trove-cli

# Index the current repo (writes .trove/, gitignored)
cargo run -p trove-cli -- index

# Check what was indexed
cargo run -p trove-cli -- status

# Retrieve context for a task (architecture, subsystem, module, symbol)
cargo run -p trove-cli -- query "modify vendor onboarding"
```

`cargo run` indexes the current directory, which here is the Trove clone. To index another
repository, pass `--repo` before the subcommand, or install the binary:

```bash
cargo run -p trove-cli -- --repo /path/to/repo index
cargo install --path crates/trove-cli    # then: trove --repo /path/to/repo index
```

Other commands: `import-historical`, `record`, `patch`. Run `cargo run -p trove-cli -- --help` for
the full list, or see the [CLI reference](https://velofy.co/trove/cli-reference/).

### What gets indexed

- **L1 Symbols**: functions, classes, methods, structs, enums, traits, modules (Rust, Python, JS/TS, Go, Bash)
- **L2 Modules**: per-file summaries: exports, dependencies, assumptions, side effects
- **L3 Subsystems**: clusters by top-level directory
- **L4 Architecture**: repo-wide summary of subsystems and the dependency graph
- **L5 Historical**: imported from your Claude trove entries with `import-historical`

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

## Documentation

Full docs live at **https://velofy.co/trove/**:

- [Installation](https://velofy.co/trove/installation/)
- [Engine quickstart](https://velofy.co/trove/engine-quickstart/)
- [Plugin quickstart](https://velofy.co/trove/plugin-quickstart/)
- [Indexing a repository](https://velofy.co/trove/indexing/)
- [Retrieval levels](https://velofy.co/trove/retrieval/)
- [Memory hierarchy](https://velofy.co/trove/memory-hierarchy/): what is built today
- [Personal trove](https://velofy.co/trove/personal-trove/)
- [CLI reference](https://velofy.co/trove/cli-reference/)
- [Plugin reference](https://velofy.co/trove/plugin-reference/)
- [Roadmap](https://velofy.co/trove/roadmap/)
- [Changelog](https://velofy.co/trove/changelog/)

Design documents in this repository:

- [Vision: Beyond Context Windows](VISION.md): the full L0-L6 memory hierarchy design
- [Product Requirements (v0.1 plugin)](PRD.md)
- [Engineering Design](ENGINEERING.md)

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
├── index.html, 404.html        # redirects to https://velofy.co/trove/
├── PRD.md
└── ENGINEERING.md
```

---

## Contributing

Issues and pull requests are welcome at https://github.com/anishfyi/trove. Before opening a pull
request, run:

```bash
cargo build --workspace
cargo test --workspace
```

Repository content avoids em dashes and en dashes (see `.cursor/rules/no-em-dashes.mdc`).

## License

MIT, see [LICENSE](LICENSE). Built by [anishfyi](https://github.com/anishfyi).
