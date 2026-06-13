# Trove - Product Requirements Document

**Author:** anishfyi
**Status:** v0.1 (shipped)
**Last updated:** 2026-06-13

---

## 1. Problem

Claude Code sessions are stateless across time. Every session, hard-won context is rediscovered:
the decision you made last week, the gotcha that cost you an afternoon, the convention your team
agreed on, the dashboard URL you always lose. People paste the same context back in over and over,
or they keep notes somewhere Claude never reads.

Claude Code already ships an internal memory mechanism, but it is not something an everyday user can
shape, own, and carry between machines. There is no simple, portable, user-owned way to say
"remember this" and have it actually persist and resurface.

## 2. Goal

Give anyone who uses Claude Code a **personal knowledge trove** that:

- captures durable learnings with a single command (or naturally, by intent),
- stores each fact as plain, portable markdown the user fully owns,
- and reloads automatically at the start of every session so nothing is forgotten.

Success looks like: a user installs the plugin, says "remember this," and weeks later Claude answers
a question correctly because it loaded their trove without being asked.

## 3. Non-goals

- Not a database, server, or hosted service. No accounts, no sync backend.
- Not a replacement for Claude Code's built-in memory; it is a user-owned, inspectable layer.
- Not a note-taking app with a UI. The interface is Claude Code plus the filesystem.
- Not an automatic scraper of everything. Capture is deliberate and curated, not a firehose.

## 4. Users

- **Solo builders** who want continuity across many small projects (user-scope trove).
- **Teams** who want a shared, committed knowledge base that travels with a repo (project-scope trove).
- **Power users** who already keep notes and want Claude to actually read and write them.

## 5. Requirements

### Must have (v0.1)
1. **Init**: create a trove at user scope (`~/.claude/trove`) or project scope (`./.claude/trove`).
2. **Remember**: distill a learning into one atomic entry file with frontmatter, dedupe against
   existing entries, and update a human-readable `INDEX.md`.
3. **Recall**: search the index and entries, answer grounded in them, and cite sources.
4. **Auto-load**: a SessionStart hook injects the index into context every session, read-only.
5. **Portability**: everything is markdown on disk. No external dependency to read or edit it.
6. **Installable by anyone**: distributed as a Claude Code plugin via a marketplace in a public repo.

### Should have
- Intent-based auto-trigger (no need to memorize command names).
- A tiny CLI helper (`trove.sh`) for mechanical operations and non-plugin users.
- Clear guidance on what is worth remembering vs. what to skip (secrets, transient state).

### Could have (later)
- `prune` / `review` to retire stale entries.
- `export` to a single bundled file, and `import`.
- Tag and type filtering in recall.
- Optional richer auto-capture hook (opt-in) that proposes entries at session end.

### Won't have (for now)
- Any network calls, telemetry, or hosted storage.
- A graphical interface beyond the static landing page.

## 6. User experience

The entire product is four moves: **init -> remember -> recall -> auto-load**. The landing page
mirrors exactly this. The defining moment is invisible: the session that opens already knowing what
you taught it, with no action from the user.

## 7. Principles

- **User-owned and inspectable.** If a user cannot open it in a text editor, it does not belong here.
- **Atomic and honest.** One fact per file. Dedupe. Delete what is wrong. No hoarding.
- **Deliberate capture.** Curated knowledge beats an undifferentiated log.
- **Safe by default.** The automatic hook is read-only. Nothing writes without intent.

## 8. Success metrics

- A user can install and create a trove in under two minutes.
- Recall answers cite real entries, not hallucinations.
- Returning to a project after weeks, the relevant context is present without re-pasting.

## 9. Risks

- **Over-capture** dilutes signal. Mitigated by explicit "what to skip" guidance in the skills.
- **Secrets leakage** into a committed project trove. Mitigated by an explicit "never save secrets"
  rule in the remember skill.
- **Staleness.** Mitigated by recall flagging old entries and references to files that no longer exist.

---

*Companion document: [Engineering Design Doc](ENGINEERING.md).*

