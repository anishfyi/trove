---
name: init
description: Initialize a personal Trove, a local file-based knowledge index for Claude Code. Use when the user says start/set up/create my trove, or when they want to remember things but no trove exists yet. Scaffolds the trove directory, INDEX.md and entries/ folder at user scope (~/.claude/trove, follows you everywhere) or project scope (./.claude/trove, travels with the repo).
allowed-tools: Bash, Read, Write, Edit
argument-hint: "[--user|--project]"
---

# Initialize a Trove

A **Trove** is a personal, file-based knowledge index that grows as you use Claude Code. It is just
markdown on disk, so it is portable, greppable, diffable, and yours. This skill creates it.

## Steps

1. **Pick a scope** (if the user did not say in the argument):
   - **User scope** `~/.claude/trove/` (default): one trove that follows you across every project.
   - **Project scope** `./.claude/trove/`: a trove that lives in and travels with this repository
     (commit it to share with a team).

   If unclear, default to **user scope** and mention you did.

2. **Scaffold the structure.** Create the directory, an `entries/` subfolder, and an `INDEX.md`:

   ```bash
   TROVE="$HOME/.claude/trove"          # or "$PWD/.claude/trove" for project scope
   mkdir -p "$TROVE/entries"
   ```

   Then write `INDEX.md` (only if it does not already exist) with this exact starter:

   ```markdown
   # Trove Index

   > Your personal Claude Code trove. One line per entry, newest first.
   > Loaded into context at the start of every session by the trove plugin.
   > Capture new entries with /trove:remember and search with /trove:recall.

   <!-- entries below -->
   ```

3. **Confirm** to the user with the resolved path and the three commands they will use:
   `/trove:remember` to capture, `/trove:recall` to search, and that it auto-loads each session.

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

Do not overbuild. The trove is plain markdown; keep it simple, atomic, and honest.
