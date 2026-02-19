#!/usr/bin/env bash

# Backward-compatible shim.
# Prefer sourcing scripts/lib/todo-workspace.sh directly.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/todo-workspace.sh
source "$ROOT/scripts/lib/todo-workspace.sh"
