#!/usr/bin/env bash
# reflect-trove.sh - SessionEnd hook: prompt reflection into L5 historical memory.
# Read-only and defensive: never fails a session.

emit() { printf '%s\n' "$*"; }

emit "Trove reflection: if this session produced durable engineering decisions, gotchas,"
emit "conventions or architectural lessons, capture them with /trove:remember before ending."
emit "It routes each entry to the project trove's conventions, memory or context section."
emit "For repository structure changes, run /trove:index to refresh the index and the context skill."

exit 0
