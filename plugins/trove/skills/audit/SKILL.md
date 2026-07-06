---
name: audit
description: Validate the Trove, in two tiers. Use when the user says audit my trove, validate the trove, check my trove, when a trove validation report from a previous session is pending, or after a burst of /trove:remember captures. Tier 1 runs the deterministic validator script; tier 2 dispatches a subagent for the semantic pass, so the context that wrote the entries is not the context certifying them. Applies fixes and clears the report.
allowed-tools: Bash, Read, Glob, Grep, Edit, Write, Agent
argument-hint: "[optional scope, e.g. entries about the payments project]"
---

# Audit the Trove

Two tiers, strictly ordered: code checks structure, then a subagent (never this
context) judges content. This context applies fixes at the end; it does not produce
findings.

## Steps

1. **Resolve the trove** the same way every trove skill does: `./.claude/trove` if it
   has an `INDEX.md`, else `~/.claude/trove`. Audit both if both exist.

2. **Tier 1, mechanical.** Run the validator:

   ```
   bash "${CLAUDE_PLUGIN_ROOT}/scripts/validate-trove.sh" <trove-dir>
   ```

   It checks that every index line points at a real file, every top-level entry is
   indexed, markdown frontmatter is complete (title, slug matching filename, type,
   created), and JSON entries parse with their required keys. Fix everything it
   lists; these are structural and need no judgment. Rerun until clean. If the
   script is missing (skills copied manually without the plugin), perform the same
   checks yourself, mechanically, in that order, before moving on.

3. **Tier 2, semantic, by subagent.** Dispatch a read-only subagent briefed with the
   trove path and scope (default: whole trove; the user's argument narrows it). Its
   brief, verbatim where your platform lets you brief subagents:

   > Read INDEX.md and the entries in scope. Report, with the file path and the exact
   > lines for each finding: (a) entries that contradict each other; (b) near-duplicate
   > entries that should merge; (c) entries whose index line misdescribes their
   > content; (d) entries that name files, URLs, systems, or dates you can check and
   > that check fails; (e) entries so vague a cold reader could not act on them.
   > Verdict per finding: KEEP, FIX (say exactly what), MERGE (say into which), or
   > DELETE (say why). Try to refute each entry, not confirm it; lean strict.

   Do not run this pass yourself: you (or a past session) wrote these entries, and a
   writer certifying its own writing is how stale memory survives. No subagents
   available: run the same checklist as a deliberately separate pass and label the
   result self-audited.

4. **Apply the fix list.** FIX edits exactly what the finding says. MERGE folds the
   duplicate in and deletes the loser's file and index line. DELETE removes file and
   index line. Never leave a known-wrong entry indexed: a trove is only worth loading
   if what it says is true.

5. **Close out.** Rerun tier 1 once (fixes can break structure), delete
   `.audit/last-validation.md` if present, and tell the user in one short paragraph:
   entries checked, findings by verdict, what changed, anything the subagent could
   not verify left as an open question.
