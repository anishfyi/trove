---
name: recall
description: Search and recall from the personal Trove. Use when the user asks what do I know about X, did we decide something, what's in my trove, or any question that past learnings might answer. Reads INDEX.md, opens the relevant entry files, and answers grounded in them with citations.
allowed-tools: Bash, Read, Grep
argument-hint: "[what to recall]"
---

# Recall (search the Trove)

Answer a question using what is stored in the trove, grounded in the actual entry files.

## Steps

1. **Resolve the trove directory** (same rule as remember):
   - `./.claude/trove` if it has an `INDEX.md`, else `~/.claude/trove`.
   - If neither exists, tell the user there is no trove yet and offer **/trove:init**.

2. **Scan the index.** Read `INDEX.md`. Use the one-line hooks (and `grep` over `entries/` for
   keywords and tags) to find candidate entries.

3. **Open the relevant entries.** Read the matching `entries/<slug>.md` files in full. Do not answer
   from the index hooks alone; the body and frontmatter hold the real content.

4. **Synthesize an answer** grounded in those entries. **Cite** the entries you used by title or slug
   so the user can open them. If entries conflict or look stale (old `created` date, references a file
   that no longer exists), say so rather than asserting confidently.

5. **If nothing matches**, say so plainly, give your best general answer, and offer to capture the new
   learning with **/trove:remember**.

Recalled entries reflect what was true when written. If an entry names a file, flag, or command,
verify it still exists before recommending it.
