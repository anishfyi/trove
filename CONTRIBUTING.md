# Contributing to Trove

Thanks for helping build Trove. This page gets you from clone to a tested
change: engine (Rust) and Claude Code plugin (bash skills) both.

## Layout

```
crates/            Rust engine workspace
  trove-core/      store, memory objects, compression, execution log
  trove-index/     repo crawler: symbols, modules, subsystems, architecture
  trove-retrieve/  retrieval: sources, ranking, prompt assembly
  trove-tools/     installed tool catalog for the agent
  trove-cli/       the `trove` binary (ask, remember, index, query, ...)
  trove-ship/      handoff doc builder (`trove-ship`)
plugins/trove/     the Claude Code plugin
  skills/          index, init, query, recall, remember, render, ship
  scripts/         trove.sh (route/guard/render/init/reflect) + hook loaders
  hooks/           hooks.json (SessionStart -> load-trove.sh, SessionEnd -> reflect-trove.sh)
  tests/           render.test.sh (61 plugin checks + engine e2e)
```

## Build and test the engine

Stable Rust via [rustup](https://rustup.rs), then:

```bash
cargo build --workspace --locked
cargo test --workspace --locked     # 124 tests
cargo fmt --all --check             # CI enforces this on the whole workspace
cargo clippy --no-deps --workspace --all-targets -- -D warnings
```

## Test the plugin

```bash
bash plugins/trove/tests/render.test.sh
```

That script is dependency-free bash (plus `git` and coreutils); if `jq` or
`python3` exists it exercises the JSON readers too, and if the engine binary
at `target/debug/trove` is built it runs the engine end-to-end checks as
well. CI runs it on every push (job `render`).

## Run trove.sh directly

Every plugin skill is a thin shell over `trove.sh` and the engine, so you can
drive it yourself from any git repo working copy:

```bash
bash plugins/trove/scripts/trove.sh init      # scaffold .claude/trove + render skills
bash plugins/trove/scripts/trove.sh render    # INDEX.md -> skills + AGENTS.md updates
bash plugins/trove/scripts/trove.sh show memory
bash plugins/trove/scripts/trove.sh route decision        # type -> section
bash plugins/trove/scripts/trove.sh guard entry.md       # secret scan (refuses on hit)
bash plugins/trove/scripts/trove.sh reflect              # SessionEnd summary
```

## Point Claude Code at your local plugin

```bash
claude plugin install ./plugins/trove        # from a checkout
# or add the path as a local marketplace and install from there
```

The hooks use `${CLAUDE_PLUGIN_ROOT}`; to test the SessionStart hook exactly
as Claude Code runs it:

```bash
CLAUDE_PLUGIN_ROOT=./plugins/trove bash plugins/trove/scripts/load-trove.sh
echo $?   # 0 on success; output is the context the hook hands to the model
```

## Run each skill

In a Claude Code session with the plugin loaded: `/trove:init`,
`/trove:remember`, `/trove:index`, `/trove:render`, `/trove:recall`,
`/trove:query`, `/trove:ship`. Under the hood:

- remember -> guard + `trove remember <title> --type <t>` (or the same rules in trove.sh)
- recall/query -> `trove ask` / `trove query`
- index -> `trove index` then trove.sh render
- ship -> `trove-ship --repo .`

## Entry conventions

One fact per file under `.claude/trove/<section>/entries/` (flat `entries/`
for the personal trove). Front matter, in order:

```
---
title: <human title>
slug: <kebab-case-file-stem>
type: convention|decision|gotcha|preference|reference|project|snippet|context
created: YYYY-MM-DD
tags: [a, b]
---
```

Section routing: `convention`/`preference` -> `conventions`, `context` ->
`context`, everything else -> `memory`. Each section's `INDEX.md` keeps one
line per entry, newest first, right below `<!-- entries below -->`:

```
- [Title](entries/slug.md) - one-line hook
```

Never commit secrets: the guard (and `trove remember`) refuse entries that
look like keys, tokens or private keys.

## PR checklist

- [ ] `cargo test --workspace --locked`, `cargo fmt --all --check`, and
      `cargo clippy --no-deps --workspace --all-targets -- -D warnings` all pass.
- [ ] `bash plugins/trove/tests/render.test.sh` passes.
- [ ] Behavior changes ship with tests (Rust unit tests in the touched crate,
      or new render.test.sh checks for plugin behavior).
- [ ] No secrets in entries, fixtures or logs; assemble fake credentials at
      runtime in tests (see `render.test.sh`).
- [ ] Bash changes stay defensive: quote expansions, fail on error, exit
      non-zero on refusal (trove.sh is the style reference).
- [ ] No gratuitous refactors; keep the diff scoped to the change.
