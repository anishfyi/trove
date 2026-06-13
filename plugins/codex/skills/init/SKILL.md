---
name: init
description: Initialize a personal Codex, a local file-based knowledge index for Claude Code. Use when the user says start/set up/create my codex, or when they want to remember things but no codex exists yet. Scaffolds the codex directory, INDEX.md and entries/ folder at user scope (~/.claude/codex, follows you everywhere) or project scope (./.claude/codex, travels with the repo).
allowed-tools: Bash, Read, Write, Edit
argument-hint: "[--user|--project]"
---

# Initialize a Codex

A **Codex** is a personal, file-based knowledge index that grows as you use Claude Code. It is just
markdown on disk, so it is portable, greppable, diffable, and yours. This skill creates it.

## Steps

1. **Pick a scope** (if the user did not say in the argument):
   - **User scope** `~/.claude/codex/` (default): one codex that follows you across every project.
   - **Project scope** `./.claude/codex/`: a codex that lives in and travels with this repository
     (commit it to share with a team).

   If unclear, default to **user scope** and mention you did.

2. **Scaffold the structure.** Create the directory, an `entries/` subfolder, and an `INDEX.md`:

   ```bash
   CODEX="$HOME/.claude/codex"          # or "$PWD/.claude/codex" for project scope
   mkdir -p "$CODEX/entries"
   ```

   Then write `INDEX.md` (only if it does not already exist) with this exact starter:

   ```markdown
   # Codex Index

   > Your personal Claude Code codex. One line per entry, newest first.
   > Loaded into context at the start of every session by the codex plugin.
   > Capture new entries with /codex:remember and search with /codex:recall.

   <!-- entries below -->
   ```

3. **Confirm** to the user with the resolved path and the three commands they will use:
   `/codex:remember` to capture, `/codex:recall` to search, and that it auto-loads each session.

## The entry format (so every skill agrees)

Each fact lives in its own file `entries/<slug>.md`:

```markdown
---
title: <Human readable title>
slug: <kebab-case-slug>
type: decision | gotcha | preference | reference | project | snippet
created: <YYYY-MM-DD>
tags: [tag1, tag2]
---

<One or two short paragraphs: the durable thing worth keeping.>

**Why it matters:** <when this applies / why future-you will care>
**Related:** [[other-slug]]
```

And the matching one-line entry in `INDEX.md`:

```markdown
- [<Title>](entries/<slug>.md) - <one-line hook>
```

Do not overbuild. The codex is plain markdown; keep it simple, atomic, and honest.
