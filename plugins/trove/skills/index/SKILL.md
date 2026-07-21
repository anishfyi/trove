---
name: index
description: Index the repository into Trove memory layers (symbols, modules, subsystems, architecture). Use when the user asks to index the repo, build the trove index, refresh symbols, or re-index after large changes.
allowed-tools: Bash, Read
argument-hint: "[--full]"
---

# Index (build repository memory)

Build or refresh the Trove index for the current repository.

## Steps

1. **Ensure the CLI is built.** From the repo root (or any repo with a `.trove/` dir):

   ```bash
   cargo build --release -p trove-cli 2>/dev/null || true
   ```

2. **Run the indexer.** Incremental by default (only stale files rebuild):

   ```bash
   cargo run -p trove-cli -- index
   ```

   For a full rebuild:

   ```bash
   cargo run -p trove-cli -- index --full
   ```

3. **Report results.** Print symbol, module, and subsystem counts from the command output.
   If indexing fails, suggest `cargo build -p trove-cli` first.

4. **Optional: import historical entries** from the Claude trove into L5 memory:

   ```bash
   cargo run -p trove-cli -- import-historical
   ```

The index is written to `.trove/` (gitignored). Never commit `.trove/` unless the user explicitly wants project-scope index artifacts checked in.
