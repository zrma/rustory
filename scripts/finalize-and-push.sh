#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/todo-workspace.sh
source "$ROOT/scripts/lib/todo-workspace.sh"
BOOKMARK="main"
MESSAGE=""
WORK_ID=""
MESSAGE_PATTERN='^(feat|fix|perf|refactor|docs|test|build|ci|chore|revert): .+'
DEBUG_GATES_OVERRIDE="${DEBUG_GATES_OVERRIDE:-0}"
MANIFEST_MODE="full"
DRY_RUN=0
REMOTE="origin"
ALLOW_EMPTY_AT=0
ALLOW_MISSING_WORK_ID=0
ALLOW_QUICK_MANIFEST=0
CLOSED_WORK_ID=0

usage() {
  cat <<'USAGE'
Finalize current working copy and push safely.

Usage:
  scripts/finalize-and-push.sh --message "<type>: <summary>" [options]

Options:
  --message <msg>         jj describe message (required)
  --bookmark <name>       bookmark to move and push (default: main)
  --work-id <id>          pass work-id into release/push gates
                          (omit to auto-detect single todo, or run readiness
                          for all docs/todo-* when multiple exist;
                          no todo workspace면 기본 실패.
                          단, 현재 diff 또는 (로컬 변경이 없을 때)
                          직전 커밋(HEAD^..HEAD)에
                          단일 docs/todo-* 삭제가 감지되면
                          해당 work-id를 마감 커밋으로 자동 허용)
  --allow-missing-work-id force no-work-id path (skip single auto-selection;
                          debug only, requires DEBUG_GATES_OVERRIDE=1 and non-CI env)
  --manifest-mode <mode>  release gate manifest mode: quick|full (default: full)
  --allow-quick-manifest  allow --manifest-mode quick (debug only;
                          requires DEBUG_GATES_OVERRIDE=1 and non-CI env)
  --remote <name>         remote for push and SHA verification (default: origin)
  --allow-empty-at        allow empty @ (debug only;
                          requires DEBUG_GATES_OVERRIDE=1 and non-CI env)
  --dry-run               print commands only
  -h, --help              show help
USAGE
}

ok() {
  echo "[ OK ] $*"
}

warn() {
  echo "[WARN] $*"
}

fail() {
  echo "[FAIL] $*" >&2
  exit 1
}

resolve_remote_head_sha() {
  local remote="$1"
  local bookmark="$2"
  local max_attempts="${3:-5}"
  local attempt=1
  local remote_sha=""

  while (( attempt <= max_attempts )); do
    remote_sha="$(cd "$ROOT" && git ls-remote --heads "$remote" "$bookmark" | awk '{print $1}' | tr -d '\r\n')"
    if [[ -n "$remote_sha" ]]; then
      printf '%s' "$remote_sha"
      return 0
    fi
    if (( attempt < max_attempts )); then
      sleep 1
    fi
    attempt=$((attempt + 1))
  done

  return 1
}

run_argv() {
  if (( DRY_RUN == 1 )); then
    printf '[DRY]'
    for token in "$@"; do
      printf ' %q' "$token"
    done
    printf '\n'
    return 0
  fi
  (cd "$ROOT" && "$@")
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

require_debug_override() {
  local reason="$1"
  if [[ "$DEBUG_GATES_OVERRIDE" != "1" ]]; then
    fail "$reason is debug-only (set DEBUG_GATES_OVERRIDE=1 to override)"
  fi
  if [[ -n "${CI:-}" ]]; then
    fail "$reason override is not allowed in CI"
  fi
  warn "$reason override enabled via DEBUG_GATES_OVERRIDE=1"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --message)
      MESSAGE="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --bookmark)
      BOOKMARK="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --work-id)
      WORK_ID="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --allow-missing-work-id)
      ALLOW_MISSING_WORK_ID=1
      shift
      ;;
    --manifest-mode)
      MANIFEST_MODE="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --allow-quick-manifest)
      ALLOW_QUICK_MANIFEST=1
      shift
      ;;
    --remote)
      REMOTE="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --allow-empty-at)
      ALLOW_EMPTY_AT=1
      shift
      ;;
    --dry-run)
      DRY_RUN=1
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

if ! todo_workspace_load_config "$ROOT"; then
  fail "failed to load todo workspace config from docs/REPO_MANIFEST.yaml"
fi

if [[ -z "$MESSAGE" ]]; then
  fail "--message is required"
fi

if [[ ! "$MESSAGE" =~ $MESSAGE_PATTERN ]]; then
  fail "invalid --message format: '$MESSAGE' (expected '<type>: <summary>' with type in feat|fix|perf|refactor|docs|test|build|ci|chore|revert; scope parentheses are not allowed)"
fi

if [[ "$ALLOW_MISSING_WORK_ID" -eq 1 ]]; then
  require_debug_override "--allow-missing-work-id"
fi

if [[ "$ALLOW_QUICK_MANIFEST" -eq 1 ]]; then
  require_debug_override "--allow-quick-manifest"
fi

if [[ "$ALLOW_EMPTY_AT" -eq 1 ]]; then
  require_debug_override "--allow-empty-at"
fi

if [[ -n "$WORK_ID" ]] && ! todo_workspace_is_valid_work_id "$WORK_ID"; then
  fail "invalid --work-id: $WORK_ID (expected lowercase kebab-case, e.g. llm-agent-stability-hardening)"
fi

if [[ -z "$WORK_ID" && "$ALLOW_MISSING_WORK_ID" -ne 1 ]]; then
  resolved_work_id=""
  discover_status=0
  if resolved_work_id="$(todo_workspace_discover_work_id "$ROOT")"; then
    WORK_ID="$resolved_work_id"
    ok "auto-detected --work-id=$WORK_ID"
  else
    discover_status=$?
    if [[ "$discover_status" -eq 2 ]]; then
      closed_work_id_output=""
      closed_work_id_status=0
      if closed_work_id_output="$(todo_workspace_discover_closed_work_id "$ROOT" auto)"; then
        WORK_ID="$closed_work_id_output"
        CLOSED_WORK_ID=1
        warn "auto-detected closed --work-id=$WORK_ID from deleted $TODO_WORKSPACE_GLOB entries"
      else
        closed_work_id_status=$?
        if [[ "$closed_work_id_status" -eq 3 ]]; then
          fail "multiple closed work-id candidates detected from deleted $TODO_WORKSPACE_GLOB entries; specify --work-id explicitly"
        fi
        fail "no '$TODO_WORKSPACE_GLOB' directory found. finalize-and-push requires todo workspace (override: --allow-missing-work-id with DEBUG_GATES_OVERRIDE=1)"
      fi
    elif [[ "$discover_status" -eq 3 ]]; then
      warn "multiple '$TODO_WORKSPACE_GLOB' directories found. continue without --work-id and run readiness for all."
      while IFS= read -r todo_dir; do
        [[ -z "$todo_dir" ]] && continue
        echo "       - $todo_dir"
      done <<< "$resolved_work_id"
    fi
  fi
fi

todo_rel=""
todo_abs=""
if [[ -n "$WORK_ID" ]]; then
  todo_rel="$(todo_workspace_rel_for_work_id "$WORK_ID")"
  todo_abs="$ROOT/$todo_rel"
fi

if [[ -n "$WORK_ID" && ! -d "$todo_abs" && "$CLOSED_WORK_ID" -ne 1 ]]; then
  closed_work_id_output=""
  closed_work_id_status=0
  if closed_work_id_output="$(todo_workspace_discover_closed_work_id "$ROOT" auto)"; then
    if [[ "$closed_work_id_output" == "$WORK_ID" ]]; then
      CLOSED_WORK_ID=1
      warn "$todo_rel not found; treat as closed-work commit from deleted workspace diff"
    else
      fail "$todo_rel not found and deleted workspace candidate is '$closed_work_id_output'"
    fi
  else
    closed_work_id_status=$?
    if [[ "$ALLOW_MISSING_WORK_ID" -eq 1 ]]; then
      warn "$todo_rel not found. continue due --allow-missing-work-id override"
    elif [[ "$closed_work_id_status" -eq 3 ]]; then
      fail "$todo_rel not found and multiple deleted workspace candidates exist; specify --work-id explicitly"
    else
      fail "$todo_rel not found. explicit --work-id requires matching deleted workspace evidence in current diff or (clean tree) HEAD^..HEAD"
    fi
  fi
fi

if [[ "$MANIFEST_MODE" != "quick" && "$MANIFEST_MODE" != "full" ]]; then
  fail "invalid --manifest-mode: $MANIFEST_MODE (expected quick|full)"
fi

if [[ "$MANIFEST_MODE" == "quick" && "$ALLOW_QUICK_MANIFEST" -ne 1 ]]; then
  fail "--manifest-mode quick is debug-only (override: --allow-quick-manifest)"
fi

if ! command -v jj >/dev/null 2>&1; then
  fail "jj command not found"
fi

if ! command -v git >/dev/null 2>&1; then
  fail "git command not found"
fi

if (( DRY_RUN == 0 )); then
  if ! (cd "$ROOT" && git remote get-url "$REMOTE" >/dev/null 2>&1); then
    fail "remote not configured: $REMOTE"
  fi
fi

nonempty_at="$(cd "$ROOT" && jj log -r '@ & ~empty()' --no-graph -T 'commit_id' | tr -d '\r\n')"
if [[ -z "$nonempty_at" ]]; then
  if (( ALLOW_EMPTY_AT == 1 )); then
    warn "working-copy(@) is empty; continue due to --allow-empty-at"
  else
    fail "working-copy(@) is empty. finalize 대상 변경이 없음"
  fi
fi

gate_args=(scripts/check-release-gates.sh --manifest-mode "$MANIFEST_MODE")
if [[ "$MANIFEST_MODE" == "quick" && "$ALLOW_QUICK_MANIFEST" -eq 1 ]]; then
  gate_args+=(--allow-quick-manifest)
fi
if [[ -n "$WORK_ID" ]]; then
  gate_args+=(--work-id "$WORK_ID")
elif [[ "$ALLOW_MISSING_WORK_ID" -eq 1 ]]; then
  gate_args+=(--allow-missing-work-id)
fi
run_argv "${gate_args[@]}"
ok "release gates passed"

run_argv jj describe -m "$MESSAGE"
ok "jj describe updated"

run_argv jj bookmark move "$BOOKMARK" --to @
ok "bookmark moved: $BOOKMARK"

push_args=()
if [[ "$REMOTE" != "origin" ]]; then
  push_args+=(--remote "$REMOTE")
fi
if [[ "$BOOKMARK" != "main" || "$REMOTE" != "origin" ]]; then
  push_args+=(--bookmark "$BOOKMARK")
fi

push_gate_env=(env PUSH_GATES_MODE=strict)
if [[ -n "$WORK_ID" ]]; then
  push_gate_env+=(PUSH_GATES_WORK_ID="$WORK_ID")
elif [[ "$ALLOW_MISSING_WORK_ID" -eq 1 ]]; then
  push_gate_env+=(ALLOW_MISSING_WORK_ID=1)
fi

remote_sha_before=""
if (( DRY_RUN == 0 )); then
  remote_sha_before="$(resolve_remote_head_sha "$REMOTE" "$BOOKMARK" 1 || true)"
fi

run_argv "${push_gate_env[@]}" scripts/jj-git-push-safe.sh "${push_args[@]}"
ok "push completed"

local_sha=""
remote_sha=""
if (( DRY_RUN == 0 )); then
  local_sha="$(cd "$ROOT" && jj log -r "$BOOKMARK" --no-graph -T 'commit_id' | tr -d '\r\n')"
  remote_sha="$(resolve_remote_head_sha "$REMOTE" "$BOOKMARK" 5 || true)"

  if [[ -z "$local_sha" ]]; then
    fail "failed to resolve local SHA for $BOOKMARK"
  fi

  if [[ -z "$remote_sha" ]]; then
    if [[ -z "$remote_sha_before" ]]; then
      fail "failed to resolve remote SHA after first push ($REMOTE/$BOOKMARK)"
    fi
    fail "failed to resolve remote SHA for $REMOTE/$BOOKMARK"
  fi

  if [[ "$local_sha" != "$remote_sha" ]]; then
    fail "remote SHA mismatch ($REMOTE/$BOOKMARK): local=$local_sha remote=$remote_sha"
  fi

  if [[ -z "$remote_sha_before" ]]; then
    ok "remote SHA verified (first push): $REMOTE/$BOOKMARK = $remote_sha"
  else
    ok "remote SHA verified: $REMOTE/$BOOKMARK = $remote_sha"
  fi
fi

run_argv jj st
ok "finalize-and-push completed"
