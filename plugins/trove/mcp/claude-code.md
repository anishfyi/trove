# Claude Code

Verified against `claude mcp add --help` on this machine.

```bash
claude mcp add trove -- trove mcp serve
```

With an explicit repository and environment (flag forms from the same help
text):

```bash
claude mcp add trove -e TROVE_REPO=/path/to/repo -- trove --repo /path/to/repo mcp serve
```

Notes:

- `--` separates the server command (`trove mcp serve`) from claude's own
  flags; stdio is the default transport.
- Use `-s project` (or the scope flag your version offers) to write the
  server into the project instead of the user scope.
- Verify inside a session with `/mcp`, or list with `claude mcp list`.

If `trove` is not on `PATH`, use the full path to the binary, e.g.
`~/.cargo/bin/trove` or `target/debug/trove` from a checkout.
