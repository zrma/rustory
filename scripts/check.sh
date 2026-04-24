#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: scripts/check.sh [--fast|--no-smoke] [--secret-scan]

Runs the same Rust checks as CI:
  - cargo fmt --all --check
  - cargo test --workspace
  - cargo clippy --workspace --all-targets -- -D warnings
  - scripts/smoke_p2p_local.sh

Options:
  --fast, --no-smoke  Skip the smoke test.
  --secret-scan       Run TruffleHog scan (Docker): scripts/secret_scan.sh
EOF
}

smoke=1
secret_scan=0

while [[ $# -gt 0 ]]; do
	case "$1" in
	--help | -h)
		usage
		exit 0
		;;
	--fast | --no-smoke)
		smoke=0
		;;
	--secret-scan)
		secret_scan=1
		;;
	*)
		echo "unknown arg: $1" >&2
		usage >&2
		exit 2
		;;
	esac
	shift
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

if [[ "$secret_scan" -eq 1 ]]; then
	bash scripts/secret_scan.sh
fi

if [[ "$smoke" -eq 1 ]]; then
	bash scripts/smoke_p2p_local.sh
fi
