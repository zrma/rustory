#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/todo-workspace.sh
source "$ROOT/scripts/lib/todo-workspace.sh"
WORK_ID=""
OWNER="codex"
NO_INIT=0
DRY_RUN=0

usage() {
  cat <<'USAGE'
Bootstrap and validate a todo workspace before implementation.

Usage:
  scripts/start-work.sh --work-id <id> [options]

Options:
  --work-id <id>   todo workspace id (lowercase kebab-case)
  --owner <name>   default checklist owner in generated template (default: codex)
  --no-init        fail when docs/todo-<work-id> does not exist
  --dry-run        print actions without filesystem changes
  -h, --help       show help
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

run_argv() {
  if (( DRY_RUN == 1 )); then
    printf '[DRY]'
    local token=""
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

write_file() {
  local target="$1"
  local body="$2"

  if (( DRY_RUN == 1 )); then
    echo "[DRY] write $target"
    return 0
  fi

  printf '%s\n' "$body" >"$target"
}

build_spec_template() {
  local work_id="$1"
  local owner="$2"
  local todo_rel
  todo_rel="$(todo_workspace_rel_for_work_id "$work_id")"

  cat <<EOF
# Spec: $work_id

## 배경

- 요청 맥락: \`$work_id\` 작업을 시작하기 전에 계획/검증 기준을 고정한다.
- 현재 문제/기회: 시작 단계를 수동으로 처리하면 계획 스냅샷/게이트 누락이 발생할 수 있다.

## 계획 스냅샷

- 목표: \`$work_id\` 작업을 단일 기준(spec)으로 관리하고 안전하게 구현한다.
- 범위: 현재 요청에 포함된 코드/문서/스크립트 변경만 수행한다.
- 검증 명령: \`scripts/run-manifest-checks.sh --mode quick --work-id $work_id\`.
- 완료 기준: C-체크리스트 항목이 \`done\` 상태가 되고 검증 명령이 재현 가능하게 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | $owner | \`scripts/run-manifest-checks.sh --mode quick --work-id $work_id\` | 요청 구현과 검증 수행 |

## 완료/미완료/다음 액션

- 완료: 없음.
- 미완료: C1.
- 다음 액션: 요구사항을 확정하고 구현/검증을 진행한다.
- 검증 증거: \`scripts/check-todo-readiness.sh $todo_rel\`, \`scripts/check-open-questions-schema.sh --require-closed $todo_rel/open-questions.md\`.
EOF
}

build_open_questions_template() {
  cat <<'EOF'
# Open Questions

현재 미결 항목 없음.
EOF
}

ensure_todo_workspace() {
  local work_id="$1"
  local owner="$2"
  local todo_rel
  todo_rel="$(todo_workspace_rel_for_work_id "$work_id")"
  local todo_abs="$ROOT/$todo_rel"
  local spec_abs="$todo_abs/spec.md"
  local open_q_abs="$todo_abs/open-questions.md"

  if [[ ! -d "$todo_abs" ]]; then
    if (( NO_INIT == 1 )); then
      fail "todo workspace not found: $todo_rel (--no-init)"
    fi
    if (( DRY_RUN == 1 )); then
      echo "[DRY] mkdir -p $todo_rel"
    else
      mkdir -p "$todo_abs"
    fi
    ok "created todo workspace: $todo_rel"
  fi

  if [[ ! -f "$spec_abs" ]]; then
    write_file "$spec_abs" "$(build_spec_template "$work_id" "$owner")"
    ok "initialized $todo_rel/spec.md"
  else
    warn "skip existing file: $todo_rel/spec.md"
  fi

  if [[ ! -f "$open_q_abs" ]]; then
    write_file "$open_q_abs" "$(build_open_questions_template)"
    ok "initialized $todo_rel/open-questions.md"
  else
    warn "skip existing file: $todo_rel/open-questions.md"
  fi
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --work-id)
      WORK_ID="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --owner)
      OWNER="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --no-init)
      NO_INIT=1
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

if [[ -z "$WORK_ID" ]]; then
  fail "--work-id is required"
fi

if ! todo_workspace_is_valid_work_id "$WORK_ID"; then
  fail "invalid --work-id: $WORK_ID (expected lowercase kebab-case, e.g. llm-agent-stability-hardening)"
fi

if [[ -z "$OWNER" ]]; then
  fail "--owner must not be empty"
fi

if ! todo_workspace_load_config "$ROOT"; then
  fail "failed to load todo workspace config from docs/REPO_MANIFEST.yaml"
fi

ensure_todo_workspace "$WORK_ID" "$OWNER"

TODO_REL="$(todo_workspace_rel_for_work_id "$WORK_ID")"
run_argv scripts/check-todo-readiness.sh "$TODO_REL"
run_argv scripts/check-open-questions-schema.sh --require-closed "$TODO_REL/open-questions.md"
run_argv scripts/run-manifest-checks.sh --mode quick --work-id "$WORK_ID"

ok "start-work checks passed: $TODO_REL"
