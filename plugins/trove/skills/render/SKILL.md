---
name: render
description: Render the project trove into loadable skills. Use when the user asks to sync, regenerate or rebuild the trove skills, after editing files under .claude/trove/ by hand, after pulling trove changes from teammates, or when the conventions, memory or context skills look out of date. Writes .claude/skills/<project>-conventions, <project>-memory and <project>-context SKILL.md files and the AGENTS.md pointer.
allowed-tools: Bash, Read
argument-hint: "[conventions|memory|context|all]"
---

# Render (project trove to skills)

A project trove keeps three sections under `.claude/trove/`, each with an `INDEX.md` and `entries/`.
This skill turns each section into a Claude Code skill so the right knowledge loads on intent:

| Section | Skill | Loads when |
|---------|-------|------------|
| `conventions/` | `<project>-conventions` | writing, editing, reviewing or committing code; naming and placing files |
| `memory/` | `<project>-memory` | changing behavior, picking an approach, debugging, asking why |
| `context/` | `<project>-context` | orienting, locating code, planning cross-module changes |

## Steps

1. **Check for a project trove.** If `.claude/trove/conventions/INDEX.md` does not exist at the repo
   root, run **/trove:init** with `--project` first.

2. **Render** (all sections by default, or the one named in the argument):

   ```bash
   bash "${CLAUDE_PLUGIN_ROOT}/scripts/trove.sh" render all
   ```

   Replace `all` with `conventions`, `memory` or `context` to render one section.

   For each section it writes `.claude/skills/<project>-<section>/SKILL.md` with:
   - front matter `name` and a `description` tuned to trigger on intent,
   - every entry inlined in `INDEX.md` order (newest first), each with its source path,
   - a short "More entries" list of paths once the skill passes about 24 KB
     (`TROVE_SKILL_MAX_BYTES` to change).

   Rendering is deterministic and only rewrites files whose content changed. The secret guard runs
   over each rendered skill and redacts anything that looks like a credential, with a warning that
   names the section to fix. `render all` also refreshes the trove block in `AGENTS.md`.

3. **To refresh the codebase map as well**, run `trove.sh sync` instead: it rebuilds the context
   section from `.trove/` (when the engine index exists) and then renders everything.

4. **Report** which skills changed. Claude Code picks up edits to existing skill folders mid-session;
   if `.claude/skills/` did not exist when the session started, tell the user to run `/reload-skills`.

Never edit the generated `SKILL.md` files by hand; edit the entries and re-render. The SessionStart
hook also re-renders any section whose entries are newer than its skill.
