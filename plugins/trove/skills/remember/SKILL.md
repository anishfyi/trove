---
name: remember
description: Capture a durable learning into the Trove. Use when the user says remember this, note this, save to trove, add to my trove, this is how we do things here, or when a session surfaces a convention, decision, gotcha, preference, or external reference worth keeping. Routes the entry to the project trove's conventions, memory or context section, guards against secrets, writes one atomic entry file, updates INDEX.md and re-renders the matching skill.
allowed-tools: Bash, Read, Write, Edit
argument-hint: "[the thing to remember]"
---

# Remember (capture into the Trove)

Persist one durable, atomic fact into the trove. One file per fact, indexed in an `INDEX.md`.

The helper script is `"${CLAUDE_PLUGIN_ROOT}/scripts/trove.sh"`, written `trove.sh` below.

## Steps

1. **Resolve the trove directory.** Prefer a project trove if present, else the user trove:
   - If `./.claude/trove/INDEX.md` exists (at the repo root), use `./.claude/trove`.
   - Else if `~/.claude/trove/INDEX.md` exists, use `~/.claude/trove`.
   - Else there is no trove yet: run the **/trove:init** skill first (or offer to), then continue.

2. **Distill to one atomic fact.** Compress the thing worth remembering into a single, self-contained
   entry. If the user hands you several things, write several entries.

3. **Route it to a section** (project troves have `conventions/`, `memory/` and `context/`; a user
   trove or an older flat project trove has only `entries/`, so skip this step there):

   | Section | What goes there | Types |
   |---------|-----------------|-------|
   | `conventions/` | How this repo is built: code style, structure, naming, testing, commit and PR rules, things to never do. Phrased as a rule to follow. | `convention`, `preference` |
   | `memory/` | What happened and why: decisions with their rationale, gotchas and footguns, incidents, history, external references. | `decision`, `gotcha`, `reference`, `project`, `snippet` |
   | `context/` | Hand-written map knowledge the engine cannot infer: what a subsystem is for, data flow, service boundaries. | `context` |

   Rule of thumb: if it tells an agent **how to write code here**, it is a convention; if it explains
   **why something is the way it is**, it is memory; if it says **where things are and how they
   connect**, it is context. `trove.sh route <type>` prints the section for a type. Never write into
   the generated context entries (front matter `generated: trove-index`); /trove:index replaces them.

4. **Dedupe.** Read the section's `INDEX.md`. If an existing entry already covers this, **update that
   file** instead of creating a near-duplicate. Delete entries that have become wrong.

5. **Choose the format, then write the entry** into `<section>/entries/` (or `entries/` for a flat
   trove). Two formats are supported; you decide by the *shape* of the content, every time:
   - **Markdown** (`<slug>.md`) when the content is prose: a decision and its rationale, a gotcha, a
     convention, any narrative worth a paragraph or two.
   - **JSON** (`<slug>.json`) when the content is *structured data*: a mapping, a list of records, a
     config snapshot, an API/endpoint table, a schema, anything where the structure is the point.

   When in doubt, prefer Markdown. Both formats carry the same top-level fields so every skill agrees.

   **Markdown template:**

   ```markdown
   ---
   title: <Human readable title>
   slug: <kebab-case-slug>
   type: convention | decision | gotcha | preference | reference | project | snippet | context
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
     "type": "reference | snippet | project | decision | gotcha | preference | convention | context",
     "created": "<YYYY-MM-DD>",
     "tags": ["tag1", "tag2"],
     "summary": "<one-line hook for the index>",
     "data": { "...": "the structured payload, any shape" }
   }
   ```

6. **Run the secret guard on the file you wrote.** This is not optional:

   ```bash
   bash "${CLAUDE_PLUGIN_ROOT}/scripts/trove.sh" guard <path-to-entry>
   ```

   It scans for API keys (`sk-`, `AKIA`, `ghp_`, `shpat_`, `xox?-` and more), private key blocks and
   `key=`/`token=`/`password:` assignments. On a hit it exits non-zero and names the line without
   printing the secret. Then rewrite the entry without the secret (keep what was *non-obvious* about
   it, such as where the real value lives), or run `trove.sh guard --redact <path>` to replace each
   match with `[REDACTED]`. Tell the user either way.

7. **Update the index.** Add a one-line bullet at the **top** of the entry list in the section's
   `INDEX.md` (below `<!-- entries below -->`, newest first), pointing at the file you wrote, or
   refresh the existing line if you updated an entry:

   ```markdown
   - [<Title>](entries/<slug>.md) - <one-line hook>
   ```

8. **Re-render the skill (project trove only)** so the `<project>-<section>` skill matches:

   ```bash
   bash "${CLAUDE_PLUGIN_ROOT}/scripts/trove.sh" render <section>
   ```

   Rendering runs the guard again over the generated skill.

9. **Confirm** with the entry path, section and title. Keep it to one line.

## What is worth remembering (and what is not)

**Save:** conventions the user expects you to follow, decisions and their rationale, gotchas and
footguns, external references (URLs, dashboards, tickets), and project constraints that are not
obvious from the code.

**Do not save:** secrets or credentials, transient task state that only matters to this conversation,
or facts trivially re-derivable from the codebase, git history, or existing docs. If asked to remember
one of those, capture instead what was *non-obvious* about it.
