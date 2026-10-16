---
name: recall
description: Search and recall from the Trove. Use when the user asks what do I know about X, did we decide something, how do we do X here, what's in my trove, or any question that past learnings might answer. Reads the INDEX.md files (including the project trove's conventions, memory and context sections), opens the relevant entry files, and answers grounded in them with citations.
allowed-tools: Bash, Read, Grep
argument-hint: "[what to recall]"
---

# Recall (search the Trove)

Answer a question using what is stored in the trove, grounded in the actual entry files.

## Steps

1. **Resolve the trove directory** (same rule as remember):
   - `./.claude/trove` (at the repo root) if it has an `INDEX.md`, else `~/.claude/trove`.
   - If neither exists, tell the user there is no trove yet and offer **/trove:init**.

2. **Scan the indexes.** Read `INDEX.md`. A project trove links to three sections, each with its own
   index: `conventions/INDEX.md` (how this repo is built), `memory/INDEX.md` (decisions, gotchas,
   history) and `context/INDEX.md` (codebase map). Start with the section that fits the question, but
   search all of them. Use the one-line hooks and grep for keywords and tags:

   ```bash
   bash "${CLAUDE_PLUGIN_ROOT}/scripts/trove.sh" grep "<term>"
   ```

   Entries may be `.md` or `.json`, in `entries/` or `<section>/entries/`.

3. **Open the relevant entries.** Read the matching entry files in full. Do not answer from the index
   hooks alone; the body holds the real content. For a JSON entry, the structured payload lives under
   its `data` key, parse and read it. The rendered `.claude/skills/<project>-*/SKILL.md` files mirror
   the entries, but the entry files are the source of truth.

4. **Synthesize an answer** grounded in those entries. **Cite** the entries you used by title or path
   so the user can open them. If entries conflict or look stale (old `created` date, references a file
   that no longer exists, a context entry indexed long ago), say so rather than asserting confidently.

5. **If nothing matches**, say so plainly, give your best general answer, and offer to capture the new
   learning with **/trove:remember**.

Recalled entries reflect what was true when written. If an entry names a file, flag, or command,
verify it still exists before recommending it. If the context section looks stale, suggest
**/trove:index**.
