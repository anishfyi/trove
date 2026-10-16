---
name: init
description: Initialize a Trove, a local file-based knowledge index for coding agents. Use when the user says start/set up/create my trove, set up trove for this project, or wants to remember things but no trove exists yet. In a code repository it scaffolds a project trove (./.claude/trove) with conventions, memory and context sections rendered as skills; otherwise a personal trove at ~/.claude/trove.
allowed-tools: Bash, Read, Write, Edit
argument-hint: "[--project|--user]"
---

# Initialize a Trove

A **Trove** is a file-based knowledge index that grows as you work. It is just plain files on disk
(Markdown for prose, JSON for structured data), so it is portable, greppable, diffable, and yours.
This skill creates it.

## Steps

1. **Pick a scope** (if the user did not say in the argument):
   - **Project scope** `./.claude/trove/` (default inside a git repository): a project trove that lives
     in and travels with this repository. Commit it to share with a team and with other agents.
   - **User scope** `~/.claude/trove/` (default elsewhere): one personal trove that follows you across
     every project.

   If unclear, use project scope in a repository and user scope otherwise, and mention which you used.

2. **Scaffold it** with the bundled helper (it never overwrites existing files):

   ```bash
   bash "${CLAUDE_PLUGIN_ROOT}/scripts/trove.sh" init --project   # or --user
   ```

   **Project scope** creates three sections, each with its own `INDEX.md` and `entries/`:

   ```
   .claude/trove/
   ├── INDEX.md                 # links to the three sections
   ├── conventions/             # how this repo is built: style, structure, naming, commit and PR rules
   ├── memory/                  # decisions, gotchas and history, with the why
   └── context/                 # codebase map from the engine, refreshed by /trove:index
   ```

   It then renders each section into a loadable skill and writes an `AGENTS.md` pointer so non-Claude
   agents (Codex, Cursor, pi) find the same files:

   ```
   .claude/skills/<project>-conventions/SKILL.md
   .claude/skills/<project>-memory/SKILL.md
   .claude/skills/<project>-context/SKILL.md
   AGENTS.md                    # a marked trove block; existing content is kept
   ```

   `<project>` is the repository directory name in lowercase kebab case (`trove.sh project` prints it;
   set `TROVE_PROJECT` to override).

   **User scope** creates `INDEX.md` and a flat `entries/` folder, as before.

3. **Fill the context section (project scope).** Run **/trove:index** so the context skill holds the
   real architecture, subsystems and key modules instead of a placeholder.

4. **Confirm** to the user with the resolved path, the three skill names (project scope), and the
   commands they will use: `/trove:remember` to capture, `/trove:recall` to search, `/trove:index` to
   refresh the codebase map, and that the trove auto-loads each session.

## The entry format (so every skill agrees)

Each fact is its own file in an `entries/` folder (in project scope, the section's `entries/`), in
**one of two formats**. The remember skill chooses by the shape of the content: Markdown for prose,
JSON for structured data.

**Markdown** `entries/<slug>.md` for prose: decisions, gotchas, conventions, narrative notes.

```markdown
---
title: <Human readable title>
slug: <kebab-case-slug>
type: convention | decision | gotcha | preference | reference | project | snippet | context
created: <YYYY-MM-DD>
tags: [tag1, tag2]
---

<One or two short paragraphs: the durable thing worth keeping.>

**Why it matters:** <when this applies / why future-you will care>
**Related:** [[other-slug]]
```

**JSON** `entries/<slug>.json` for structured context: mappings, lists of records, config, schemas.

```json
{
  "title": "<Human readable title>",
  "slug": "<kebab-case-slug>",
  "type": "reference | snippet | project | decision | gotcha | preference | convention | context",
  "created": "<YYYY-MM-DD>",
  "tags": ["tag1", "tag2"],
  "summary": "<one-line hook for the index>",
  "data": { "...": "the structured payload, any shape" }
}
```

Either way, add the matching one-line entry to the `INDEX.md` next to the `entries/` folder. The file
extension tells readers (and the session hook) which format it is:

```markdown
- [<Title>](entries/<slug>.md) - <one-line hook>
- [<Title>](entries/<slug>.json) - <one-line hook>
```

Do not overbuild. Keep entries simple, atomic, and honest. Never store secrets.
