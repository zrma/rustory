#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$ROOT"

# Push 전 release 게이트(내부 push 게이트 포함)를 선차단한다.
PUSH_GATES_MODE="${PUSH_GATES_MODE:-strict}"
PUSH_GATES_WORK_ID="${PUSH_GATES_WORK_ID:-}"
ALLOW_MISSING_WORK_ID="${ALLOW_MISSING_WORK_ID:-0}"
DEBUG_GATES_OVERRIDE="${DEBUG_GATES_OVERRIDE:-0}"

require_debug_override() {
  local reason="$1"
  if [[ "$DEBUG_GATES_OVERRIDE" != "1" ]]; then
    echo "[FAIL] $reason is debug-only (set DEBUG_GATES_OVERRIDE=1 to override)" >&2
    exit 1
  fi
  if [[ -n "${CI:-}" ]]; then
    echo "[FAIL] $reason override is not allowed in CI" >&2
    exit 1
  fi
  echo "[WARN] $reason override enabled via DEBUG_GATES_OVERRIDE=1"
}

if [[ "$PUSH_GATES_MODE" != "strict" ]]; then
  if [[ "${ALLOW_NON_STRICT_PUSH_GATES:-0}" != "1" ]]; then
    echo "[FAIL] PUSH_GATES_MODE must be strict (override: ALLOW_NON_STRICT_PUSH_GATES=1)" >&2
    exit 1
  fi
  require_debug_override "ALLOW_NON_STRICT_PUSH_GATES"
fi

if [[ "$ALLOW_MISSING_WORK_ID" == "1" ]]; then
  require_debug_override "ALLOW_MISSING_WORK_ID"
fi

add_target_bookmark() {
  local candidate="$1"
  local existing=""

  if [[ -z "$candidate" ]]; then
    echo "[FAIL] bookmark value cannot be empty" >&2
    exit 1
  fi

  if (( ${#TARGET_BOOKMARKS[@]} > 0 )); then
    for existing in "${TARGET_BOOKMARKS[@]}"; do
      if [[ "$existing" == "$candidate" ]]; then
        return 0
      fi
    done
  fi

  TARGET_BOOKMARKS+=("$candidate")
}

run_conflict_gate() {
  local -a cmd=(scripts/check-jj-conflicts.sh)
  local bookmark
  for bookmark in "${TARGET_BOOKMARKS[@]}"; do
    cmd+=(--bookmark "$bookmark")
  done
  "${cmd[@]}"
}

REMOTE="origin"
DRY_RUN=0
declare -a TARGET_BOOKMARKS=()
if [[ "$#" -eq 0 ]]; then
  TARGET_BOOKMARKS=("main")
else
  args=("$@")
  idx=0
  while (( idx < ${#args[@]} )); do
    token="${args[$idx]}"
    case "$token" in
      --remote|-r)
        ((idx += 1))
        if (( idx >= ${#args[@]} )); then
          echo "[FAIL] missing value for $token" >&2
          exit 1
        fi
        REMOTE="${args[$idx]}"
        ;;
      --bookmark|-b)
        ((idx += 1))
        if (( idx >= ${#args[@]} )); then
          echo "[FAIL] missing value for $token" >&2
          exit 1
        fi
        add_target_bookmark "${args[$idx]}"
        ;;
      --dry-run)
        DRY_RUN=1
        ;;
      *)
        echo "[FAIL] unsupported push option: $token" >&2
        exit 1
        ;;
    esac
    ((idx += 1))
  done

  if (( ${#TARGET_BOOKMARKS[@]} == 0 )); then
    echo "[FAIL] custom push args require explicit --bookmark/-b (default main is used only when no args)" >&2
    exit 1
  fi
fi

release_gate_cmd=(scripts/check-release-gates.sh --manifest-mode full)
if [[ -n "$PUSH_GATES_WORK_ID" ]]; then
  release_gate_cmd+=(--work-id "$PUSH_GATES_WORK_ID")
elif [[ "$ALLOW_MISSING_WORK_ID" == "1" ]]; then
  release_gate_cmd+=(--allow-missing-work-id)
fi
declare -a VALIDATED_TARGET_SHAS=()
for bookmark in "${TARGET_BOOKMARKS[@]}"; do
  target_sha="$(jj log -r "$bookmark" --no-graph -T 'commit_id' | tr -d '\r\n')"
  if [[ -z "$target_sha" ]]; then
    echo "[FAIL] failed to resolve target SHA for bookmark: $bookmark" >&2
    exit 1
  fi
  TODO_UNPUBLISHED_BASE_REF="refs/remotes/$REMOTE/$bookmark" \
    TODO_UNPUBLISHED_TARGET_REV="$target_sha" \
    "${release_gate_cmd[@]}"
  VALIDATED_TARGET_SHAS+=("$target_sha")
done

run_conflict_gate

for bookmark in "${TARGET_BOOKMARKS[@]}"; do
  scripts/check-lessons-log-range.sh --remote "$REMOTE" --bookmark "$bookmark"
done

declare -a refspecs=()
for idx in "${!TARGET_BOOKMARKS[@]}"; do
  bookmark="${TARGET_BOOKMARKS[$idx]}"
  validated_sha="${VALIDATED_TARGET_SHAS[$idx]}"
  current_sha="$(jj log -r "$bookmark" --no-graph -T 'commit_id' | tr -d '\r\n')"
  if [[ "$current_sha" != "$validated_sha" ]]; then
    echo "[FAIL] bookmark moved after validation: $bookmark validated=$validated_sha current=$current_sha" >&2
    exit 1
  fi
  refspecs+=("$validated_sha:refs/heads/$bookmark")
done

if (( DRY_RUN == 1 )); then
  printf '[DRY] git push %q' "$REMOTE"
  printf ' %q' "${refspecs[@]}"
  printf '\n'
else
  # Push immutable validated objects, not mutable bookmark names. Git's normal
  # non-fast-forward protection remains in force.
  git push "$REMOTE" "${refspecs[@]}"
  jj git fetch --remote "$REMOTE"
fi
