---
name: ship
description: Turn this project and its trove into developer handoff docs (overview, architecture, setup, conventions, decisions, gotchas) with secrets and private info stripped first. Use when the user says ship the docs, write handoff docs, onboarding docs, document this repo for a new dev or contractor, or export the trove safely. Runs trove-ship, which redacts secrets, emails, phones, internal hosts and IPs, local paths, usernames and customer names, writes a redaction report, and fails closed if a secret survives.
allowed-tools: Bash, Read, Write, Edit
argument-hint: "[--format folder|single|site] [--out DIR] [--agent claude|codex|pi]"
---

# Ship (handoff docs, private info stripped)

`trove-ship` builds handoff docs from the engine index (`.trove/`), the project trove
(`./.claude/trove/entries`) and well-known files (README, build manifests, agent rules, ADRs,
TODO markers). A redaction pass runs on every document before anything is written. The personal
user trove in `~/.claude/trove` is never shipped.

## Steps

1. **Find the binary.** Use the first that works and reuse it below as `SHIP`:

   ```bash
   command -v trove-ship                              # installed
   cargo run -q -p trove-ship -- --version            # inside the trove repo
   cargo install --git https://github.com/anishfyi/trove trove-ship   # install it
   ```

2. **Preview without writing.** Show the user what would be removed:

   ```bash
   SHIP --repo . --dry-run
   ```

   The report lists each removed value by placeholder (`[REDACTED:email-1]`), category, detector,
   reason, length and location. It never contains the values themselves. Summarize the counts per
   category for the user.

3. **Tune `.troveignore` if needed.** At the repo root. Bare lines are gitignore-style paths that
   never enter the docs. Directives, one per line:

   ```text
   secrets/
   allow: oss@example-company.dev
   allow-domain: example-company.dev
   customer: Globex
   deny: Project Nightjar
   deny-regex: TICKET-\d+
   internal-domain: corp-example.net
   ```

   Ask before creating or editing this file. `allow` rules cannot keep a high-confidence secret.

4. **Ship.** Default output is a Markdown folder in `.trove/ship/` (gitignored). Pass the user's
   choices through:

   ```bash
   SHIP --repo . --format folder            # README.md + one file per section
   SHIP --repo . --format single            # one HANDOFF.md
   SHIP --repo . --format site --out docs/handoff   # static HTML site
   ```

   Add `--reindex always` to refresh the engine index first. If the output directory exists and was
   not written by trove-ship, it refuses unless `--force` is given; ask the user before forcing.

5. **Optional agent review.** If the user wants a second pass beyond the regexes, add
   `--agent claude`, `--agent codex` or `--agent pi` (or `--agent-cmd "<shell command>"` that reads
   the prompt on stdin and prints a JSON array). Only already-redacted text is sent. If the agent
   fails, the ship aborts and nothing is written.

6. **Review the output yourself.** Read the generated docs. If anything private is still there
   (a customer, a person, a codename, an internal URL), do not quote it back in chat; point to the
   file and line. Propose a `deny:` or `customer:` rule for `.troveignore`, add it once the user
   agrees, and run step 4 again.

7. **Report.** Give the output path, the format, the number of values removed (and how many were
   high confidence), and the path to `REDACTION_REPORT.md` (or `redaction-report.html` for a site).

## If it fails closed

Exit code 2 means a high-confidence secret survived into the rendered output (for example a
Markdown-escaped key like `sk\_live\_...`). Nothing was written. The error lists output file, line
and detector. Find the source in the repo, then either remove the secret there (and tell the user to
rotate it) or exclude that file with a path line in `.troveignore`. Never work around the check.
