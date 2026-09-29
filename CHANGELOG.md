# Changelog

Full history: https://velofy.co/trove/changelog/

## 0.2.1 (2026-09-30)

- Documentation moved to https://velofy.co/trove/. The old single-page site is replaced by a redirect. The README was rewritten and the crates and plugin manifest now carry the new homepage and documentation URLs.
- The manifest version written by `trove index`, and shown by `trove status`, now comes from the crate version instead of a hard-coded "0.2.0".
- No other engine or plugin behavior changed.

### Marketplace renamed: anishfyi-trove to velofy-trove

The marketplace was renamed from `anishfyi-trove` to `velofy-trove` on 2026-09-12, after v0.2.0 was released under the old name. The plugin name is still `trove`, and the repository is still https://github.com/anishfyi/trove.

The install command is now:

```
/plugin install trove@velofy-trove
```

If you added the marketplace before the rename, your copy may still be listed as `anishfyi-trove`. I have not verified how Claude Code handles a marketplace whose declared name changed, so the safe route is to re-add it:

```
/plugin marketplace remove anishfyi-trove
/plugin marketplace add anishfyi/trove
/plugin install trove@velofy-trove
/reload-plugins
```

Your troves (`.trove/` in a project, and the user trove under `~/.claude/trove`) are plain files and are not touched by this.

## 0.2.0 (2026-07-21)

Trove became a two-layer system: a Rust repository memory engine plus the Claude Code plugin. See https://github.com/anishfyi/trove/releases/tag/v0.2.0.

- Engine: repository indexing into `.trove/`, progressive retrieval, and the CLI commands `index`, `status`, `query`, `import-historical`, `record` and `patch`.
- Plugin: new `/trove:index` and `/trove:query` skills and a SessionEnd hook.
