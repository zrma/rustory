#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
declare -a BOOKMARKS=()
declare -a UNIQUE_BOOKMARKS=()
CHECK_AT=1
fail_count=0
warn_count=0

usage() {
  cat <<'USAGE'
Check unresolved jj conflicts before push/release.

Usage:
  scripts/check-jj-conflicts.sh [options]

Options:
  --bookmark <name>   check conflict state for bookmark revision (repeatable)
  --skip-at           skip working-copy(@) conflict check
  -h, --help          show help
USAGE
}

ok() {
  echo "[ OK ] $*"
}

warn() {
  echo "[WARN] $*"
  warn_count=$((warn_count + 1))
}

fail() {
  echo "[FAIL] $*" >&2
  fail_count=$((fail_count + 1))
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

add_bookmark() {
  local bookmark="$1"
  local existing=""
  for existing in "${UNIQUE_BOOKMARKS[@]}"; do
    if [[ "$existing" == "$bookmark" ]]; then
      return 0
    fi
  done
  UNIQUE_BOOKMARKS+=("$bookmark")
}

collect_conflicts() {
  local revset="$1"
  cd "$ROOT"
  jj log -r "($revset) & conflicts()" --no-graph \
    -T 'change_id.short() ++ " " ++ commit_id.short() ++ " " ++ description.first_line() ++ "\n"'
}

check_rev_conflicts() {
  local revset="$1"
  local label="$2"
  local conflicts=""

  if ! conflicts="$(collect_conflicts "$revset" 2>/dev/null)"; then
    fail "$label: revset evaluation failed ($revset)"
    return
  fi

  if [[ -n "$conflicts" ]]; then
    fail "$label: unresolved jj conflict commit detected"
    while IFS= read -r line; do
      [[ -z "$line" ]] && continue
      echo "       - $line"
    done <<< "$conflicts"
    return
  fi

  ok "$label: no jj conflicts"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bookmark|-b)
      BOOKMARKS+=("$(parse_opt_value "$1" "${2:-}")")
      shift 2
      ;;
    --skip-at)
      CHECK_AT=0
      shift
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

if [[ ! -d "$ROOT/.jj" ]]; then
  warn "root is not a jj repository (.jj missing). skip conflict check."
  exit 0
fi

if ! command -v jj >/dev/null 2>&1; then
  fail "jj command not found"
  echo "[FAIL] jj conflict check failed with $fail_count issue(s)" >&2
  exit 1
fi

if (( CHECK_AT == 1 )); then
  check_rev_conflicts '@' "working-copy(@)"
fi

for bookmark in "${BOOKMARKS[@]}"; do
  add_bookmark "$bookmark"
done

for bookmark in "${UNIQUE_BOOKMARKS[@]}"; do
  check_rev_conflicts "$bookmark" "bookmark '$bookmark'"
done

if (( fail_count > 0 )); then
  echo "[FAIL] jj conflict check failed with $fail_count issue(s)" >&2
  exit 1
fi

if (( warn_count > 0 )); then
  echo "[WARN] jj conflict check passed with $warn_count warning(s)"
else
  ok "jj conflict check passed"
fi
