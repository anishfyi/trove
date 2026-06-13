---
name: remember
description: Capture a durable learning into the personal Trove. Use when the user says remember this, note this, save to trove, add to my trove, or when a session surfaces a decision, gotcha, convention/preference, or external reference worth keeping. Writes one atomic entry file and updates INDEX.md.
allowed-tools: Bash, Read, Write, Edit
argument-hint: "[the thing to remember]"
---

# Remember (capture into the Trove)

Persist one durable, atomic fact into the trove. One file per fact, indexed in `INDEX.md`.

## Steps

1. **Resolve the trove directory.** Prefer a project trove if present, else the user trove:
   - If `./.claude/trove/INDEX.md` exists, use `./.claude/trove`.
   - Else if `~/.claude/trove/INDEX.md` exists, use `~/.claude/trove`.
   - Else there is no trove yet: run the **/trove:init** skill first (or offer to), then continue.

2. **Distill to one atomic fact.** Compress the thing worth remembering into a single, self-contained
   entry. If the user hands you several things, write several entries.

3. **Dedupe.** Read `INDEX.md`. If an existing entry already covers this, **update that file** instead
   of creating a near-duplicate. Delete entries that have become wrong.

4. **Choose the format, then write the entry.** Two formats are supported; you decide by the *shape*
   of the content, every time:
   - **Markdown** (`entries/<slug>.md`) when the content is prose: a decision and its rationale, a
     gotcha, a convention, any narrative worth a paragraph or two.
   - **JSON** (`entries/<slug>.json`) when the content is *structured data*: a mapping, a list of
     records, a config snapshot, an API/endpoint table, a schema, anything where the structure is the
     point and prose would only get in the way.

   When in doubt, prefer Markdown. Both formats carry the same top-level fields so every skill agrees.

   **Markdown template:**

   ```markdown
   ---
   title: <Human readable title>
   slug: <kebab-case-slug>
   type: decision | gotcha | preference | reference | project | snippet
   created: <YYYY-MM-DD>     # use today's date
   tags: [tag1, tag2]
   ---

   <One or two short paragraphs: the durable thing worth keeping.>

   **Why it matters:** <when this applies / why it will matter later>
   **Related:** [[other-slug]]    # link related entries; a not-yet-written slug is fine
   ```

   **JSON template** (put the structured payload under `data`):

   ```json
   {
     "title": "<Human readable title>",
     "slug": "<kebab-case-slug>",
     "type": "reference | snippet | project | decision | gotcha | preference",
     "created": "<YYYY-MM-DD>",
     "tags": ["tag1", "tag2"],
     "summary": "<one-line hook for the index>",
     "data": { "...": "the structured payload, any shape" }
   }
   ```

5. **Update the index.** Add a one-line bullet at the **top** of the entry list in `INDEX.md`
   (newest first), pointing at the file you wrote (`.md` or `.json`), or refresh the existing line if
   you updated an entry:

   ```markdown
   - [<Title>](entries/<slug>.md) - <one-line hook>
   ```

6. **Confirm** with the entry path and title. Keep it to one line.

## What is worth remembering (and what is not)

**Save:** decisions and their rationale, gotchas and footguns, conventions/preferences the user
expects you to follow, external references (URLs, dashboards, tickets), and project constraints that
are not obvious from the code.

**Do not save:** secrets or credentials, transient task state that only matters to this conversation,
or facts trivially re-derivable from the codebase, git history, or existing docs. If asked to remember
one of those, capture instead what was *non-obvious* about it.

