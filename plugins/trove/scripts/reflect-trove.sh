#!/usr/bin/env bash
# reflect-trove.sh - SessionEnd hook: prompt reflection into L5 historical memory.
# Read-only and defensive: never fails a session.

emit() { printf '%s\n' "$*"; }

emit "Trove reflection: if this session produced durable engineering decisions, gotchas,"
emit "or architectural lessons, capture them with /trove:remember before ending."
emit "For repository structure changes, run /trove:index to refresh the symbol index."

exit 0
