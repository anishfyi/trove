# Cursor

> Shape follows the Cursor MCP documentation; Cursor is not installed on
> this machine, so it could not be verified against local tooling. Keep it
> minimal.

Cursor configures stdio MCP servers in `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "trove": {
      "command": "trove",
      "args": ["mcp", "serve"]
    }
  }
}
```

With an explicit repository:

```json
{
  "mcpServers": {
    "trove": {
      "command": "trove",
      "args": ["--repo", "/path/to/repo", "mcp", "serve"]
    }
  }
}
```

If `trove` is not on `PATH`, set `"command"` to the full binary path.
