---
name: remember
description: Capture a durable learning into the personal Codex. Use when the user says remember this, note this, save to codex, add to my codex, or when a session surfaces a decision, gotcha, convention/preference, or external reference worth keeping. Writes one atomic entry file and updates INDEX.md.
allowed-tools: Bash, Read, Write, Edit
argument-hint: "[the thing to remember]"
---

# Remember (capture into the Codex)

Persist one durable, atomic fact into the codex. One file per fact, indexed in `INDEX.md`.

## Steps

1. **Resolve the codex directory.** Prefer a project codex if present, else the user codex:
   - If `./.claude/codex/INDEX.md` exists, use `./.claude/codex`.
   - Else if `~/.claude/codex/INDEX.md` exists, use `~/.claude/codex`.
   - Else there is no codex yet: run the **/codex:init** skill first (or offer to), then continue.

2. **Distill to one atomic fact.** Compress the thing worth remembering into a single, self-contained
   entry. If the user hands you several things, write several entries.

3. **Dedupe.** Read `INDEX.md`. If an existing entry already covers this, **update that file** instead
   of creating a near-duplicate. Delete entries that have become wrong.

4. **Write the entry** at `entries/<slug>.md` using the canonical format:

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

5. **Update the index.** Add a one-line bullet at the **top** of the entry list in `INDEX.md`
   (newest first), or refresh the existing line if you updated an entry:

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
