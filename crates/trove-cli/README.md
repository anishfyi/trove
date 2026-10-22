# trove CLI

One entry point on macOS and Linux for everything Trove knows: your personal trove,
the current project's trove and engine index, repo docs, and every CLI installed on
the machine.

```bash
cargo install --path crates/trove-cli   # installs the `trove` binary
```

## Ask

```bash
trove                                   # interactive prompt (:help for commands)
trove ask "how does the retrieval budget work"
trove ask --json "where is the tool catalog cached"    # for agents
trove ask --budget 400 "freshness"     # cap the context in tokens (default 1500)
```

Sources, ranked together with BM25 plus title and coverage boosts:

| Kind | Where it comes from |
|------|---------------------|
| `personal` | `~/.claude/trove/entries/*` (flat) |
| `project` | `./.claude/trove/entries/*` plus sectioned `{conventions,memory,context}/entries/*` |
| `doc` | top-level `*.md` and `docs/`, one document per heading |
| `symbol`, `module`, `subsystem`, `arch` | the engine index in `.trove/` (run `trove index`) |
| `tool` | the installed tool catalog (`--no-tools` leaves it out) |

Every hit is cited as `file:line` (or a tool path) with the line that matched:

```
 1 personal  ~/.claude/trove/entries/budget-default.md:8  Keep ask budgets small
   Default the ask token budget to about 1500 so answers stay dense.
 2 symbol    crates/trove-core/src/hierarchy.rs:29  default_token_budget
   pub fn default_token_budget(self) -> usize {
-- 2 of 77 matches | ~120/1500 tokens | offline retrieval; add --provider for a written answer
```

Retrieval is fully offline. A written answer, citing sources as `[n]`, is opt-in:

| `--provider` | Needs |
|--------------|-------|
| `none` (default) | nothing |
| `anthropic` | `ANTHROPIC_API_KEY` (model defaults to `claude-opus-5-5`, override with `--model`) |
| `ollama` | a local Ollama server (`OLLAMA_HOST`, `OLLAMA_MODEL`) |
| `cmd` | any command reading the prompt on stdin: `TROVE_LLM_CMD`, default `claude -p` |

`TROVE_PROVIDER` and `TROVE_MODEL` set defaults. HTTP goes through the system `curl`;
API keys are passed to it on stdin, never on the command line.

## Remember

```bash
echo "Bulk inserts must wrap in one transaction." \
  | trove remember "WAL gotcha: bulk inserts" --type gotcha --tags db,sqlite
trove --json remember "Use pgx" --type convention --body "All Postgres code goes through pgx."
```

Writes one entry per fact into the project trove (or the flat personal one),
with the title words as a positional argument, the body on `--body` or piped
stdin, and a required `--type` (convention, decision, gotcha, preference,
reference, project, snippet, context) that routes to a section:
convention/preference -> `conventions`, context -> `context`, everything else
-> `memory`; `--section` overrides. Each call writes
`<section>/entries/<slug>.md` (front matter: title, slug, type, created,
tags) and inserts the newest-first line into the section's `INDEX.md`.
Entries that look like secrets are refused before anything is written, and an
existing slug is refused unless `--update` replaces it (keeping the original
`created` date).

## Tools

```bash
trove tools                       # list every installed CLI with a one-line summary
trove tools search json           # ranked search over names and summaries
trove tools jq                    # describe: path, summary, man page, --help excerpt
trove tools jq --man              # open the man page
trove tools refresh               # show what the incremental refresh did
trove tools refresh --probe       # also collect summaries from --help (see below)
```

Discovery scans `$PATH` first (so shadowing matches your shell), then Homebrew
(`/opt/homebrew`, `/usr/local`, Linuxbrew), `~/.cargo/bin`, `~/.local/bin`, npm
(`$NPM_CONFIG_PREFIX`, `~/.npm-global`, nvm, Volta), pnpm (`$PNPM_HOME`), Bun and Go.
Summaries come from man pages, read directly (plain or gzip, man and mdoc formats).

The catalog is cached in `~/.cache/trove/tools.json` (`~/Library/Caches/trove` on
macOS, `TROVE_CACHE_DIR` to override). Each run restats the bin directories and
rescans only the ones that changed; within those, unchanged binaries keep their
cached summary. `--full` rebuilds from scratch.

`--probe` runs `<tool> --help` for tools without a man summary. Some programs ignore
`--help` and do their normal work, so probing is opt-in, skips a denylist (shells,
editors, `reboot`, `mkfs`, ...), has no stdin, a 1.5s timeout, and runs each tool in
an empty scratch directory with `HOME` pointed there. `trove tools <name>` runs
`--help` for that one tool, since you asked about it.

## Output

Default output is compact, one or two lines per item, with a `--` footer for
totals. `--json` works on every command (`ask`, `tools`, `status`, `index`,
`query`). `--budget` sets the token budget for `ask` and `query`.
