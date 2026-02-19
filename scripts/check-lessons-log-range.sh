#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REMOTE="origin"
BOOKMARK="main"

usage() {
  cat <<'USAGE'
Enforce lessons-log coupling on a push range.

Usage:
  scripts/check-lessons-log-range.sh [options]

Options:
  --remote <name>     remote name used for range base (default: origin)
  --bookmark <name>   bookmark/branch to validate (default: main)
  -h, --help          show help
USAGE
}

parse_opt_value() {
  local opt_name="$1"
  local opt_value="${2:-}"
  if [[ -z "$opt_value" ]]; then
    echo "missing value for $opt_name" >&2
    usage >&2
    exit 1
  fi
  printf '%s' "$opt_value"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --remote|-r)
      REMOTE="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --bookmark|-b)
      BOOKMARK="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

cd "$ROOT"

if ! command -v jj >/dev/null 2>&1; then
  echo "[FAIL] jj command not found" >&2
  exit 1
fi

if ! command -v git >/dev/null 2>&1; then
  echo "[FAIL] git command not found" >&2
  exit 1
fi

local_sha="$(jj log -r "$BOOKMARK" --no-graph -T 'commit_id' | tr -d '\r\n')"
if [[ -z "$local_sha" ]]; then
  echo "[FAIL] lessons-log range check skipped: local sha not found for bookmark '$BOOKMARK'" >&2
  exit 1
fi

remote_sha="$(git ls-remote --heads "$REMOTE" "$BOOKMARK" | awk '{print $1}' | tr -d '\r\n')"
range=""
if [[ -n "$remote_sha" ]]; then
  range="$remote_sha...$local_sha"
else
  fallback_ref="$REMOTE/main"
  if ! git rev-parse --verify --quiet "$fallback_ref" >/dev/null; then
    fallback_ref="main"
  fi

  fallback_base="$(git merge-base "$local_sha" "$fallback_ref" 2>/dev/null || true)"
  if [[ -z "$fallback_base" ]]; then
    echo "[FAIL] lessons-log range check failed: remote branch not found ($REMOTE/$BOOKMARK) and fallback base is unavailable" >&2
    exit 1
  fi

  range="$fallback_base...$local_sha"
  echo "[WARN] lessons-log range fallback used: $range"
fi

scripts/check-lessons-log-enforcement.sh --range "$range"
echo "[ OK ] lessons-log range enforcement passed: $range"
