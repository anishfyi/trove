# trove-mcp — read-only MCP server specification

`trove mcp serve` speaks the Model Context Protocol (JSON-RPC 2.0 over stdio)
so MCP-capable agents can read trove memory. This slice is **read-only**:
the write half (remember/reflect as MCP tools) is deliberately out of scope
and arrives in the next slice of issue #6.

## Design rule

`crates/trove-mcp` contains only protocol machinery (framing, dispatch,
error shapes) and depends on no trove crate. All trove behavior lives in
`trove-cli` (a new library target) behind one `Handler` trait, so the
protocol can be tested with stub handlers and the wiring with the real
`trove` binary. The server reuses the exact production code paths of
`trove ask` / `trove query` / `trove status` / `trove tools search` — no
second implementations to drift.

## Transport

- stdio only; no network listener, no sockets.
- Newline-delimited JSON-RPC 2.0: one request or notification per line,
  one response per line. No batching (a JSON array line is an invalid
  request).
- stdout carries **only** protocol responses. Diagnostics and errors the
  server survives go to stderr.
- The server exits cleanly on EOF (client closed stdin). It never prompts,
  never reads a TTY, never spawns subprocesses, never calls an LLM
  provider. `trove_ask` is always offline: the LLM provider flags of the
  CLI do not apply here.

## Lifecycle

1. Client sends `initialize` (params may carry `protocolVersion`). The
   server echoes the client's version when it is one it supports
   (2024-11-05 and 2025-03-26 — the implemented surface is compatible with
   both); any other or missing version is answered with 2024-11-05. The
   reply is
   `{protocolVersion, capabilities: {tools: {}, resources: {}, prompts: {}}, serverInfo: {name: "trove", version: <crate version>}}`.
2. Client sends `notifications/initialized` (no response).
3. Client calls `tools/list`, `tools/call`, `resources/list`,
   `resources/read`, `prompts/list`, `prompts/get`, or `ping` any number
   of times, in any order.

## Repo and trove resolution

Identical to `trove ask`: the global `--repo` flag (default: search
upward from the current directory for `.trove` or `.git`), plus the
personal trove at `$HOME/.claude/trove`. The handler holds only the two
paths; resolution happens once at startup and every retrieval rebuilds
its corpus from disk (see Freshness).

## Tools

### trove_ask

Ask trove memory a question; returns cited sources within a token budget.
This is exactly the offline mode of `trove ask` (provider `none`).

Input schema:
```json
{"question": {"type": "string"},
 "limit":    {"type": "integer", "minimum": 1, "maximum": 50},
 "budget":   {"type": "integer", "minimum": 100, "maximum": 100000}}
```
- `question` required. `limit` default 12, `budget` default 1500 (the
  `ask::DEFAULT_BUDGET`).
- Result content: text summary lines — `mode: offline retrieval`,
  `N of M matches | ~T/B tokens`, then per source
  `- <cite> (<kind>): <title>` plus the same snippet line the offline
  `trove ask` footer shows (`   <snippet>`, when the snippet differs from
  the title). Never an LLM answer.
- Errors (unreadable trove, bad params) are **tool results with
  `isError: true`**, not protocol errors: the server stays usable.

### trove_query

Assemble progressive context for a query within a token budget; this is
`trove query` (level-major items with levels and token estimates).

Input schema:
```json
{"question": {"type": "string"},
 "budget":   {"type": "integer", "minimum": 100, "maximum": 1000000}}
```
- `question` required. `budget` default 200000 (the `trove query`
  production default).
- Result content: text — `Query: <q>` header, `N items | ~T tokens`, then
  the same per-item rendering `trove query` prints:
  `## <level> [<tier>] <object_id>` followed by the item text; item text
  length is governed by the pipeline budget.

### trove_status

Index freshness and counts (what `trove status` prints): `Trove v…`,
`Repo:`, `Indexed:`, `Counts: N symbols, N modules, N subsystems`, or
`No index found. Run: trove index`. No input schema.

### trove_tools_search

Search the installed tool catalog (`trove tools search`): zero or more
`- <name> (<pair>)` lines, or `no tools found`. Input schema:
`{"query": {"type": "string"}}`, `query` required.

## Resources

| URI | Body |
|---|---|
| `trove://index` | the index bodies the session loader would show: project INDEX.md plus personal INDEX.md |
| `trove://skills/conventions` | the rendered conventions skill if present, else the section's INDEX.md body |
| `trove://skills/memory` | same, memory section |
| `trove://skills/context` | same, context section |

`resources/read` returns `{contents: [{uri, mimeType: "text/markdown", text}]}`.
Reads are allowlisted to exactly those files (project/personal trove
INDEX.md and section INDEX.md / rendered skill files inside
`.claude/trove` or `.claude/skills`); any other URI is a protocol error
`-32602`. Missing sections read as empty text rather than erroring.

## Prompts

| Name | Guidance |
|---|---|
| `remember` | how to capture one durable fact: run the guard, route by type, one entry per fact, newest-first index line, never secrets (faithful summary of the remember skill) |
| `reflect` | how to reflect a session: summarize what changed, propose at most three entries, never paste secrets (faithful summary of the reflect hook guidance) |

`prompts/get` returns `{messages: [{role: "user", content: {type: "text", text}}]}`.

## Error shapes

| Case | Response |
|---|---|
| Malformed JSON line | `{jsonrpc: "2.0", id: null, error: {code: -32700, message: "Parse error"}}` |
| Unknown method (with id) | `error.code -32601` |
| Invalid params / unknown tool or resource URI | `error.code -32602` |
| Internal failure | `error.code -32603` |
| Notifications (no id) | no response, never an error |
| Handler-level tool failure | tool result with `isError: true` (see above) |

## Freshness

Every `tools/call` re-runs `Knowledge::load` (and `TroveStore::open` for
`trove_query`) against the resolved paths, so an entry written by the CLI
between two MCP calls is visible to the second call. Resources are read
from disk per request for the same reason.

## Threat model

- **Read-only:** no tool writes, no `tools/call` can mutate any file.
- **No arbitrary file reads:** resource URIs are matched against a fixed
  allowlist before any path is opened.
- **No code execution:** questions are passed to the retrieval corpus and
  tokenizer only; nothing is ever `sh -c`'d.
- **No network:** no listeners, no outbound calls, LLM providers
  permanently off.
- **Stdout purity:** any library used by a handler that might print is
  wrapped so only protocol frames reach stdout.
- Untrusted input: MCP client strings (questions, URIs). They can neither
  escape the trove directories nor trigger side effects.
