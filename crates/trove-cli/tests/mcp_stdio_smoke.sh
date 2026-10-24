#!/usr/bin/env bash
# Pi-shaped stdio smoke for `trove mcp serve` (pi itself is not invoked:
# this drives the same newline-delimited JSON-RPC transcript through raw
# stdio pipes, the way an MCP host would).
#
# Usage: mcp_stdio_smoke.sh <trove-binary> <repo> <home>
set -euo pipefail

bin="$1"; repo="$2"; home="$3"

transcript() {
  printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize"}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
    '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"trove_ask","arguments":{"question":"what trips up bulk inserts"}}}' \
    '{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"trove://index"}}' \
    '{"jsonrpc":"2.0","id":5,"method":"ping"}'
}

out="$(HOME="$home" transcript | "$bin" --repo "$repo" mcp serve 2>/dev/null)"

check() { # name, needle
  echo "$out" | grep -q "$2" || { echo "FAIL: $1 (missing: $2)" >&2; exit 1; }
}

check initialize '"name":"trove"'
check tools-list 'trove_ask'
check ask-citation 'wal-bulk-inserts.md'
check resource-index '# Fixture trove'
check ping '"id":5'

# stdout purity: every line of output is a JSON object (protocol frames only).
n_lines=$(echo "$out" | wc -l)
n_json=$(echo "$out" | grep -c '^{')
[ "$n_lines" -eq "$n_json" ] || { echo "FAIL: stdout purity ($n_lines lines, $n_json frames)" >&2; exit 1; }

echo "pi-shaped stdio smoke: 6 checks passed"
