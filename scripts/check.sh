#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/check.sh [--fast|--no-smoke]

Runs the same checks as CI:
  - cargo fmt --all --check
  - cargo test --workspace
  - cargo clippy --workspace --all-targets -- -D warnings
  - scripts/smoke_p2p_local.sh

Options:
  --fast, --no-smoke  Skip the smoke test.
EOF
}

smoke=1
case "${1:-}" in
  --help|-h)
    usage
    exit 0
    ;;
  --fast|--no-smoke)
    smoke=0
    shift
    ;;
  "")
    ;;
  *)
    echo "unknown arg: $1" >&2
    usage >&2
    exit 2
    ;;
esac

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

if [[ "$smoke" -eq 1 ]]; then
  bash scripts/smoke_p2p_local.sh
fi

