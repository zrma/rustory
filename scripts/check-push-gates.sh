#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/todo-workspace.sh
source "$ROOT/scripts/lib/todo-workspace.sh"
MODE="strict"
WORK_ID=""
ALLOW_MISSING_WORK_ID=0
DEBUG_GATES_OVERRIDE="${DEBUG_GATES_OVERRIDE:-0}"
DRY_RUN=0
fail_count=0
CLOSED_WORK_ID=0

ok() {
  echo "[ OK ] $*"
}

fail() {
  echo "[FAIL] $*" >&2
  fail_count=$((fail_count + 1))
}

warn() {
  echo "[WARN] $*"
}

usage() {
  cat <<'USAGE'
Run pre-push safety gates.

Usage:
  scripts/check-push-gates.sh [options]

Options:
  --mode <quick|strict>  quick: branch hygiene only
                         strict: branch + jj-conflict + submodules + docs + todo + script-smoke checks (default)
  --work-id <id>         run readiness for docs/todo-<work-id> only
                         (omit to auto-detect single todo, or run readiness
                         for all docs/todo-* when multiple exist;
                         no todo workspace면 readiness 생략)
                         open-questions 닫힘 검사는 --work-id 지정 시
                         docs/todo-<work-id>/open-questions.md만 대상으로 수행,
                         미지정 시 감지된 docs/todo-*의 open-questions.md만 수행
  --allow-missing-work-id
                         force no-work-id path (skip single auto-selection;
                         debug only, requires DEBUG_GATES_OVERRIDE=1 and non-CI env)
  --dry-run              print commands without executing
  -h, --help             show help
USAGE
}

run_cmd() {
  local cmd="$1"
  if (( DRY_RUN == 1 )); then
    echo "[DRY] $cmd"
    return 0
  fi

  if eval "$cmd"; then
    ok "passed: $cmd"
    return 0
  fi

  fail "failed: $cmd"
  return 1
}

safe_run() {
  local cmd="$1"
  if ! run_cmd "$cmd"; then
    true
  fi
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
    return 1
  fi
  if [[ -n "${CI:-}" ]]; then
    fail "$reason override is not allowed in CI"
    return 1
  fi
  warn "$reason override enabled via DEBUG_GATES_OVERRIDE=1"
}

validate_work_id() {
  local work_id="$1"
  if ! todo_workspace_is_valid_work_id "$work_id"; then
    fail "invalid --work-id: $work_id (expected lowercase kebab-case, e.g. llm-agent-stability-hardening)"
    return 1
  fi
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      MODE="$(parse_opt_value "$1" "${2:-}")"
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
      usage
      exit 1
      ;;
  esac
done

if ! todo_workspace_load_config "$ROOT"; then
  echo "[FAIL] failed to load todo workspace config from docs/REPO_MANIFEST.yaml" >&2
  exit 1
fi

if [[ "$MODE" != "quick" && "$MODE" != "strict" ]]; then
  echo "invalid --mode: $MODE (expected quick|strict)" >&2
  exit 1
fi

if [[ "$ALLOW_MISSING_WORK_ID" -eq 1 ]]; then
  require_debug_override "--allow-missing-work-id"
fi

safe_run "scripts/check-branch-hygiene.sh"

if [[ "$MODE" == "quick" ]]; then
  if (( fail_count > 0 )); then
    echo "[FAIL] push gates failed: $fail_count issue(s)" >&2
    exit 1
  fi
  ok "push gates passed (quick mode)"
  exit 0
fi

if [[ -n "$WORK_ID" ]]; then
  validate_work_id "$WORK_ID"
elif [[ "$ALLOW_MISSING_WORK_ID" -ne 1 ]]; then
  resolved_work_id=""
  discover_status=0
  if resolved_work_id="$(todo_workspace_discover_work_id "$ROOT")"; then
    WORK_ID="$resolved_work_id"
    ok "auto-detected --work-id=$WORK_ID"
  else
    discover_status=$?
    if [[ "$discover_status" -eq 2 ]]; then
      warn "no '$TODO_WORKSPACE_GLOB' directory found. continue without --work-id and skip todo readiness checks."
    elif [[ "$discover_status" -eq 3 ]]; then
      warn "multiple '$TODO_WORKSPACE_GLOB' directories found. continue without --work-id and run readiness for all."
      while IFS= read -r todo_dir; do
        [[ -z "$todo_dir" ]] && continue
        echo "       - $todo_dir"
      done <<< "$resolved_work_id"
    fi
  fi
fi

if (( fail_count > 0 )); then
  echo "[FAIL] push gates failed: $fail_count issue(s)" >&2
  exit 1
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
      warn "$todo_rel not found. skip todo readiness due --allow-missing-work-id override"
    elif [[ "$closed_work_id_status" -eq 3 ]]; then
      fail "$todo_rel not found and multiple deleted workspace candidates exist; cannot resolve closed work-id"
      while IFS= read -r closed_work_id; do
        [[ -z "$closed_work_id" ]] && continue
        echo "       - $closed_work_id"
      done <<< "$closed_work_id_output"
    else
      fail "$todo_rel not found. explicit --work-id requires matching deleted workspace evidence in current diff or (clean tree) HEAD^..HEAD"
    fi
  fi
fi

if (( fail_count > 0 )); then
  echo "[FAIL] push gates failed: $fail_count issue(s)" >&2
  exit 1
fi

safe_run "scripts/check-jj-conflicts.sh"

if [[ -n "$WORK_ID" ]]; then
  if [[ -d "$todo_abs" ]]; then
    printf -v todo_cmd 'scripts/check-todo-readiness.sh %q' "$todo_rel"
    safe_run "$todo_cmd"
  elif [[ "$CLOSED_WORK_ID" -eq 1 ]]; then
    warn "skip todo readiness for closed work-id: $WORK_ID (workspace deleted in current diff)"
  elif [[ "$ALLOW_MISSING_WORK_ID" -eq 1 ]]; then
    warn "$todo_rel not found. skip todo readiness due --allow-missing-work-id override"
  else
    fail "$todo_rel not found while todo readiness is required"
  fi
else
  readarray -t TODO_DIRS < <(todo_workspace_find_dirs "$ROOT")
  if [[ "${#TODO_DIRS[@]}" -eq 0 ]]; then
    warn "no todo directory found. skip todo readiness checks."
  else
    for todo_dir in "${TODO_DIRS[@]}"; do
      printf -v todo_cmd 'scripts/check-todo-readiness.sh %q' "${todo_dir#$ROOT/}"
      safe_run "$todo_cmd"
    done
  fi
fi

declare -a OPEN_QUESTION_TARGETS=()
if [[ -n "$WORK_ID" && -d "$todo_abs" ]]; then
  OPEN_QUESTION_TARGETS+=("$todo_rel/open-questions.md")
elif [[ -n "$WORK_ID" && "$CLOSED_WORK_ID" -eq 1 ]]; then
  warn "skip scoped open-questions check for closed work-id: $WORK_ID (workspace deleted in current diff)"
else
  readarray -t TODO_DIRS_FOR_OPEN_Q < <(todo_workspace_find_dirs "$ROOT")
  if [[ "${#TODO_DIRS_FOR_OPEN_Q[@]}" -eq 0 ]]; then
    warn "no todo directory found. skip open-questions checks."
  else
    for todo_dir in "${TODO_DIRS_FOR_OPEN_Q[@]}"; do
      todo_open_q_rel="${todo_dir#$ROOT/}/open-questions.md"
      if [[ -f "$ROOT/$todo_open_q_rel" ]]; then
        OPEN_QUESTION_TARGETS+=("$todo_open_q_rel")
      else
        fail "missing open-questions: $todo_open_q_rel"
      fi
    done
  fi
fi
if [[ "${#OPEN_QUESTION_TARGETS[@]}" -gt 0 ]]; then
  open_questions_cmd="scripts/check-open-questions-schema.sh --require-closed"
  for open_q_target in "${OPEN_QUESTION_TARGETS[@]}"; do
    printf -v open_questions_cmd '%s %q' "$open_questions_cmd" "$open_q_target"
  done
  safe_run "$open_questions_cmd"
fi
safe_run "scripts/check-todo-closure.sh"
safe_run "scripts/check-lessons-log-enforcement.sh --worktree"
safe_run "scripts/check-doc-last-verified.sh"
safe_run "scripts/check-doc-links.sh"
safe_run "scripts/check-doc-index.sh"
safe_run "scripts/check-readme-policy.sh"
safe_run "scripts/check-manifest-entrypoints.sh"
safe_run "scripts/check-submodules.sh --strict"
script_smoke_cmd="scripts/check-script-smoke.sh"
if [[ -n "$WORK_ID" ]]; then
  printf -v script_smoke_cmd '%s --work-id %q' "$script_smoke_cmd" "$WORK_ID"
fi
safe_run "$script_smoke_cmd"

if (( fail_count > 0 )); then
  echo "[FAIL] push gates failed: $fail_count issue(s)" >&2
  exit 1
fi

ok "push gates passed"
