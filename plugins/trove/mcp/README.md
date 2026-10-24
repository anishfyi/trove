# Connecting MCP hosts to trove

`trove mcp serve` is a read-only MCP server over stdio (see
`crates/trove-mcp/SPEC.md`): `trove_ask`, `trove_query`, `trove_status`,
`trove_tools_search`, plus index/skill resources and `remember`/`reflect`
prompts. It never writes, never calls an LLM provider, and only speaks
protocol frames on stdout.

Resolve the repo once: `trove` uses the current directory (or `--repo`) and
`$HOME` for the personal trove, exactly like `trove ask`.

Per-host snippets:

| Host | Snippet | Shape verified? |
|---|---|---|
| Claude Code | [claude-code.md](claude-code.md) | yes - `claude mcp add --help` on this machine |
| pi | [pi.md](pi.md) | yes - pi's own docs (`docs/mcp.md` of the pi install) |
| Codex | [codex.md](codex.md) | no - follows the host's MCP documentation, keep minimal |
| Cursor | [cursor.md](cursor.md) | no - follows the host's MCP documentation, keep minimal |

After connecting, `initialize` should report `serverInfo.name = "trove"`
and `tools/list` should show the four read-only tools.
