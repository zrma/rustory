#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: scripts/secret_scan.sh

Runs a local secret scan using TruffleHog (Docker).

Notes:
  - This scans committed history (git scan). Untracked build artifacts under
    `target/` won't cause false positives.
  - CI also runs a git-range scan on push/PR.
EOF
}

case "${1:-}" in
--help | -h)
	usage
	exit 0
	;;
"") ;;
*)
	echo "unknown arg: $1" >&2
	usage >&2
	exit 2
	;;
esac

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v docker >/dev/null 2>&1; then
	echo "docker is required for secret_scan (install Docker Desktop)" >&2
	exit 1
fi

IMAGE="ghcr.io/trufflesecurity/trufflehog:3.93.1"

if ! git rev-parse --git-dir >/dev/null 2>&1; then
	echo "not a git repository: $ROOT" >&2
	exit 1
fi

scan_ref="${SECRET_SCAN_REF:-}"
if [[ -z "$scan_ref" ]]; then
	if current_branch="$(git symbolic-ref --quiet --short HEAD 2>/dev/null)"; then
		scan_ref="$current_branch"
	elif git rev-parse --verify -q refs/heads/main >/dev/null; then
		# A colocated jj working copy commonly keeps Git HEAD detached at the
		# working commit's parent while the publishable commit is on main.
		scan_ref="main"
	else
		scan_ref="HEAD"
	fi
fi
if ! git rev-parse --verify -q "$scan_ref" >/dev/null; then
	echo "secret scan ref does not exist: $scan_ref" >&2
	exit 1
fi

base=""
if git rev-parse --verify -q origin/main >/dev/null; then
	if candidate="$(git merge-base origin/main "$scan_ref" 2>/dev/null)"; then
		base="$candidate"
	fi
elif [[ "$scan_ref" != "main" ]] && git rev-parse --verify -q main >/dev/null; then
	if candidate="$(git merge-base main "$scan_ref" 2>/dev/null)"; then
		base="$candidate"
	fi
fi

head="$(git rev-parse "$scan_ref")"
if [[ -n "$base" && "$base" == "$head" ]]; then
	# Nothing to scan in a range; fall back to scanning all history.
	base=""
fi

args=(
	git
	file:///repo/
	--no-update
	--fail
	--results=verified,unknown
	--branch "$scan_ref"
	--exclude-globs 'target/**,.git/**,.jj/**'
)
if [[ -n "$base" ]]; then
	args+=(--since-commit "$base")
fi

docker run --rm \
	-v "$ROOT":/repo \
	-w /repo \
	"$IMAGE" \
	"${args[@]}"
