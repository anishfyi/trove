# pi

Verified against pi's own documentation (`docs/mcp.md` of the pi
installation on this machine).

```bash
pi mcp add trove -- trove mcp serve
```

Notes (from the same docs):

- Servers are user-level by default; add `--local` (or `-l`) to write the
  server into the project config instead.
- `pi mcp list` shows configured servers; inside a session, `/mcp`
  inspects and reconnects them and `/reload` picks up config edits made
  outside the session.

If `trove` is not on `PATH`, use the full path to the binary.
