# Trove - a personal memory index for Claude Code

**Install once, and Claude Code keeps a personal, file-based trove of your decisions, gotchas,
conventions and references as you work, and reloads it into context at the start of every session.**

Live page: **https://anishfyi.github.io/trove/**

Every fact is one markdown file with frontmatter. The trove is portable, greppable, diffable, and
yours. No database, no service, no lock-in.

---

## Install

### Plugin (recommended)

Run these **one at a time** inside Claude Code, top to bottom.

**1. Add the marketplace.** A marketplace is just a Git repo that lists installable plugins. This
points Claude Code at Trove's repo. Nothing is installed yet.

```
/plugin marketplace add anishfyi/trove
```

> Hit an SSH `Permission denied (publickey)` error? You have no SSH key on GitHub, so use the HTTPS
> URL instead: `/plugin marketplace add https://github.com/anishfyi/trove.git`

**2. Install the plugin.** Pulls the Trove plugin (its three skills plus the SessionStart hook) from
that marketplace. Read it as `plugin@marketplace`.

```
/plugin install trove@anishfyi-trove
```

**3. Reload so it activates.** A freshly installed plugin is **not live until you reload**. This
activates the skills and hook in your current session (verify with `/plugin list`). The auto-load
hook starts firing from your **next new session** onward.

```
/reload-plugins
```

**4. Create your trove.** Scaffolds `~/.claude/trove` with an `INDEX.md` and an `entries/` folder.

```
/trove:init
```

Done. From here, say "remember this" or run `/trove:remember` to capture, and `/trove:recall` to
search it back.

### Manual (skills only, no auto-load hook)

```bash
git clone https://github.com/anishfyi/trove /tmp/trove
cp -r /tmp/trove/plugins/trove/skills/* ~/.claude/skills/
```

This gives you `/init`, `/remember`, and `/recall`. The automatic session-load hook ships only with
the plugin install above.

---

## Usage

| Command | What it does |
|---------|--------------|
| `/trove:init` | Create the trove at user scope (`~/.claude/trove`) or project scope (`./.claude/trove`). |
| `/trove:remember` | Distill a durable learning into one atomic entry and add it to the index. |
| `/trove:recall` | Search the trove and answer grounded in the relevant entries. |

The skills also auto-trigger on intent: say "remember this in my trove" or "what do I know about X"
and the right skill fires without typing the command. Every new session, the SessionStart hook loads
your `INDEX.md` into context so Claude starts aware of what it already knows.

---

## Why it speeds you up

Every Claude Code session normally starts from zero: you re-explain the codebase, the conventions,
the decisions you already made. Trove ends that. What you teach it once it knows for good, so each
session begins further ahead than the last.

**It paces up your work**

- **Stop re-explaining.** Claude opens each session already aware of your decisions, conventions and
  gotchas. No daily "here is how this repo works" preamble.
- **Stop repeating mistakes.** A footgun you hit once is written down, so Claude does not walk into
  it a second time.
- **Decide faster.** Past decisions and their rationale are one recall away, so you do not
  re-litigate or contradict yourself.
- **Onboard instantly.** A new machine, or a teammate on a project-scope trove, inherits all the
  accumulated knowledge at once.
- **Compounding returns.** Teach it once, benefit every session after. The longer you use it, the
  more leverage it has.

**Why the index is the engine**

The `INDEX.md` is the part that actually loads into context at session start, one compact line per
entry, so Claude knows *everything it has* without paying to load every full file. Recall stays fast
and cheap: Claude scans the index, then opens only the handful of entries that matter instead of
dumping the whole trove into the prompt. It is also your human-readable map, skimmable and prunable
like a notebook's table of contents, and each one-line hook is written to match intent. The index is
the difference between a pile of files and a memory you can actually use.

---

## What the trove looks like

```
~/.claude/trove/
├── INDEX.md                 # one line per entry, newest first
└── entries/
    ├── use-pgx-not-orm.md
    ├── deploy-single-binary.md
    └── staging-db-mirror.md
```

An entry:

```markdown
---
title: Use pgx + sqlc, not a heavy ORM, in Go services
slug: use-pgx-not-orm
type: decision
created: 2026-06-13
tags: [go, postgres, data-layer]
---

We standardised on pgx for the driver and sqlc to generate type-safe query code
from plain SQL. GORM was rejected for hiding query cost behind reflection.

**Why it matters:** reach for this on any new Go service.
**Related:** [[deploy-single-binary]]
```

Entry `type` is one of: `decision`, `gotcha`, `preference`, `reference`, `project`, `snippet`.

---

## What is worth remembering

**Save:** decisions and their rationale, gotchas and footguns, conventions you expect Claude to
follow, external references (URLs, dashboards, tickets), and project constraints that are not obvious
from the code.

**Skip:** secrets, transient task state, and anything trivially re-derivable from the codebase or
git history. If a fact is one of those, capture what was *non-obvious* about it instead.

---

## Repository layout

```
trove/
├── .claude-plugin/
│   └── marketplace.json          # lists the trove plugin
├── plugins/
│   └── trove/
│       ├── .claude-plugin/
│       │   └── plugin.json
│       ├── skills/
│       │   ├── init/SKILL.md
│       │   ├── remember/SKILL.md
│       │   └── recall/SKILL.md
│       ├── hooks/
│       │   └── hooks.json         # SessionStart: load the index
│       └── scripts/
│           ├── load-trove.sh      # hook body (read-only, defensive)
│           └── trove.sh           # tiny CLI helper (path/init/list/grep)
├── index.html                     # AlpineJS landing page
├── PRD.md                         # product requirements
├── ENGINEERING.md                 # engineering design
└── LICENSE
```

## Docs

- [Product Requirements Doc](PRD.md)
- [Engineering Design Doc](ENGINEERING.md)

---

Built by [anishfyi](https://github.com/anishfyi). MIT licensed.
