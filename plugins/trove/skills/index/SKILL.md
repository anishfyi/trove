---
name: index
description: Index the repository into Trove memory layers (symbols, modules, subsystems, architecture) and refresh the project's context skill from it. Use when the user asks to index the repo, build the trove index, refresh symbols or the codebase map, or re-index after large changes.
allowed-tools: Bash, Read
argument-hint: "[--full]"
---

# Index (build repository memory)

Build or refresh the Trove index for the current repository, then refresh the project trove's
context section and its `<project>-context` skill from it.

## Steps

1. **Find the engine.** Use the `trove` binary if it is on `PATH`. Otherwise, inside a checkout of the
   Trove repository, use `cargo run -p trove-cli --`. In another repository, point cargo at a Trove
   checkout: `cargo run --manifest-path <trove>/Cargo.toml -p trove-cli -- --repo .`. Written `trove`
   below.

2. **Run the indexer.** Incremental by default (only stale files rebuild):

   ```bash
   trove index
   ```

   For a full rebuild:

   ```bash
   trove index --full
   ```

3. **Refresh the context skill** from the engine output in `.trove/`:

   ```bash
   bash "${CLAUDE_PLUGIN_ROOT}/scripts/trove.sh" context
   ```

   This rewrites the generated entries in `.claude/trove/context/` (architecture, one per subsystem,
   key modules), updates the generated block of `context/INDEX.md`, and re-renders
   `.claude/skills/<project>-context/SKILL.md`. Hand-written context entries are kept. If there is no
   project trove yet it scaffolds one (same as `/trove:init --project`). It needs `jq` or `python3`.

4. **Report results.** Print symbol, module, and subsystem counts from the command output and the
   path of the refreshed context skill. If indexing fails, suggest building the CLI first
   (`cargo build -p trove-cli`).

5. **Optional: import historical entries** from the Claude trove into L5 memory:

   ```bash
   trove import-historical
   ```

The engine index is written to `.trove/` (gitignored). Never commit `.trove/` unless the user
explicitly wants project-scope index artifacts checked in. The project trove under `.claude/trove/`
and the rendered skills under `.claude/skills/` are meant to be committed.
