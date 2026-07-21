---
name: query
description: Progressive retrieval from the Trove index. Use when the user asks what Trove knows about a subsystem, where code lives, or wants context assembled before editing. Runs architecture -> subsystem -> module -> symbol retrieval with a context budget.
allowed-tools: Bash, Read
argument-hint: "[natural language query]"
---

# Query (progressive retrieval)

Retrieve the smallest useful context for a task from the Trove index.

## Steps

1. **Check index exists.** Run:

   ```bash
   cargo run -p trove-cli -- status
   ```

   If no index, run **/trove:index** first.

2. **Query with the user's intent:**

   ```bash
   cargo run -p trove-cli -- query "vendor onboarding"
   ```

   Adjust `--budget` (default 200000 tokens) if the user specifies a context limit.

3. **Use the output progressively.** The CLI returns layers in order:
   - L4 Architecture
   - L3 Subsystems
   - L2 Modules
   - L1 Symbols (signatures)
   - Dependency neighbors

   Stop expanding when you have enough detail. Do not open full source files unless summaries are insufficient.

4. **JSON mode** for structured parsing:

   ```bash
   cargo run -p trove-cli -- query "auth middleware" --json
   ```

Each item includes confidence and retrieval path. Prefer higher-confidence, higher-layer items first.
