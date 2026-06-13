# Trove - a personal memory index for Claude Code

**Install once, and Claude Code keeps a personal, file-based trove of your decisions, gotchas,
conventions and references as you work, and reloads it into context at the start of every session.**

Live page: **https://anishfyi.github.io/trove/**

Every fact is one markdown file with frontmatter. The trove is portable, greppable, diffable, and
yours. No database, no service, no lock-in.

---

## Install

### Plugin (recommended)

Run these inside Claude Code:

```
/plugin marketplace add anishfyi/trove
/plugin install trove@anishfyi-trove
```

The first command registers this repo as a plugin marketplace; the second installs the `trove`
plugin, which provides three skills and a SessionStart hook. Then create your trove once:

```
/trove:init
```

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
