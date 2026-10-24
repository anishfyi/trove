# Codex

> Shape follows the Codex MCP documentation; the codex CLI is not
> installed on this machine, so it could not be verified against local
> tooling. Keep it minimal.

Codex configures MCP servers in `~/.codex/config.toml`:

```toml
[mcp_servers.trove]
command = "trove"
args = ["mcp", "serve"]
```

With an explicit repository:

```toml
[mcp_servers.trove]
command = "trove"
args = ["--repo", "/path/to/repo", "mcp", "serve"]
```

If `trove` is not on `PATH`, set `command` to the full binary path.
