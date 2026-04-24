#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/todo-workspace.sh
source "$ROOT/scripts/lib/todo-workspace.sh"
fail_count=0
tmp_root=""
WORK_ID=""
WORK_ID_PATTERN='^[a-z0-9]+(-[a-z0-9]+)*$'
DEFAULT_WORK_ID="script-smoke-default"
RESOLVED_WORK_ID=""
MISSING_WORK_ID_SUFFIX="script-smoke-missing-$$"
multi_todo_dir_a=""
multi_todo_dir_b=""
multi_todo_work_id_a=""
multi_todo_work_id_b=""
open_question_valid_file=""
open_question_noise_file=""
smoke_work_todo_dir=""
smoke_work_todo_created=0
closed_work_repo_dir=""
CLOSED_WORK_FIXTURE_WORK_ID="script-smoke-closed"
todo_closure_repo_dir=""
TODO_CLOSURE_WORK_ID="script-smoke-closure"

usage() {
  cat <<'USAGE'
Run smoke checks for gate scripts.

Usage:
  scripts/check-script-smoke.sh [--work-id <id>]

Options:
  --work-id <id>  reuse explicit work-id for nested strict/release smoke checks
  -h, --help      show help
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

discover_single_work_id() {
  readarray -t TODO_DIRS < <(todo_workspace_find_dirs "$ROOT")

  if (( ${#TODO_DIRS[@]} == 1 )); then
    local resolved_work_id=""
    if ! resolved_work_id="$(todo_workspace_extract_work_id "${TODO_DIRS[0]}")"; then
      return 1
    fi
    printf '%s\n' "$resolved_work_id"
    return 0
  fi

  if (( ${#TODO_DIRS[@]} == 0 )); then
    return 2
  fi

  return 1
}

ok() {
  echo "[ OK ] $*"
}

fail() {
  echo "[FAIL] $*" >&2
  fail_count=$((fail_count + 1))
}

run_cmd() {
  local cmd="$1"
  if (cd "$ROOT" && eval "$cmd"); then
    ok "passed: $cmd"
  else
    fail "failed: $cmd"
  fi
}

expect_fail_cmd() {
  local cmd="$1"
  if (cd "$ROOT" && eval "$cmd"); then
    fail "unexpected success (expected failure): $cmd"
  else
    ok "expected failure: $cmd"
  fi
}

run_debug_override_cmd() {
  local cmd="$1"
  if [[ -n "${CI:-}" ]]; then
    expect_fail_cmd "$cmd"
    return
  fi
  run_cmd "$cmd"
}

ensure_tmp_root() {
  if [[ -n "$tmp_root" && -d "$tmp_root" ]]; then
    return 0
  fi
  tmp_root="$(mktemp -d "$ROOT/.tmp-script-smoke.XXXXXX")"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --work-id)
      WORK_ID="$(parse_opt_value "$1" "${2:-}")"
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

if [[ -n "$WORK_ID" ]]; then
  if [[ ! "$WORK_ID" =~ $WORK_ID_PATTERN ]]; then
    fail "invalid --work-id: $WORK_ID (expected lowercase kebab-case)"
    echo "[FAIL] script smoke check failed with $fail_count issue(s)" >&2
    exit 1
  fi
  RESOLVED_WORK_ID="$WORK_ID"
else
  if discovered_work_id="$(discover_single_work_id 2>/dev/null || true)"; then
    if [[ -n "$discovered_work_id" ]]; then
      RESOLVED_WORK_ID="$discovered_work_id"
    fi
  fi
fi

if [[ -z "$RESOLVED_WORK_ID" ]]; then
  RESOLVED_WORK_ID="$DEFAULT_WORK_ID"
fi

if ! todo_workspace_load_config "$ROOT"; then
  fail "failed to load todo workspace config from docs/REPO_MANIFEST.yaml"
  echo "[FAIL] script smoke check failed with $fail_count issue(s)" >&2
  exit 1
fi

ensure_work_todo_workspace() {
  smoke_work_todo_dir="$ROOT/$(todo_workspace_rel_for_work_id "$RESOLVED_WORK_ID")"
  if [[ -d "$smoke_work_todo_dir" ]]; then
    return 0
  fi

  smoke_work_todo_created=1
  mkdir -p "$smoke_work_todo_dir"

  cat > "$smoke_work_todo_dir/open-questions.md" <<'EOF'
# Open Questions

현재 미결 항목 없음.
EOF

  cat > "$smoke_work_todo_dir/spec.md" <<'EOF'
# Spec: script-smoke default workspace

## 계획 스냅샷

- 목표: 기본 work-id 기반 게이트 smoke 통과
- 범위: check/release/finalize dry-run 경로 점검
- 검증 명령: `echo smoke`
- 완료 기준: nested dry-run 게이트가 실패하지 않는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | smoke | `echo smoke` | 기본 work-id smoke 점검 |

## 완료/미완료/다음 액션

- 완료: 없음
- 미완료: C1
- 다음 액션: dry-run 게이트 실행
- 검증 증거: `echo smoke`
EOF
}

setup_todo_readiness_fixtures() {
  ensure_tmp_root

  valid_dir="$tmp_root/readiness-valid"
  missing_checkpoint_dir="$tmp_root/readiness-missing-checkpoint"
  missing_evidence_dir="$tmp_root/readiness-missing-evidence"
  placeholder_plan_dir="$tmp_root/readiness-placeholder-plan"
  missing_verify_cmd_dir="$tmp_root/readiness-missing-verify-command"
  open_question_valid_file="$tmp_root/open-questions-valid.md"
  open_question_noise_file="$tmp_root/open-questions-noise.md"

  mkdir -p \
    "$valid_dir" \
    "$missing_checkpoint_dir" \
    "$missing_evidence_dir" \
    "$placeholder_plan_dir" \
    "$missing_verify_cmd_dir"

  cat > "$valid_dir/open-questions.md" <<'EOF'
# Open Questions

현재 미결 항목 없음.
EOF

  cat > "$valid_dir/spec.md" <<'EOF'
# Spec: readiness smoke valid

## 계획 스냅샷

- 목표: smoke 통과
- 범위: readiness 유효 샘플
- 검증 명령: `echo smoke`
- 완료 기준: readiness가 성공한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | smoke | `echo smoke` | 최소 샘플 |

## 완료/미완료/다음 액션

- 완료: 없음
- 미완료: C1
- 다음 액션: 샘플 갱신
- 검증 증거: `echo smoke`
EOF

  cat > "$missing_checkpoint_dir/open-questions.md" <<'EOF'
# Open Questions

현재 미결 항목 없음.
EOF

  cat > "$missing_checkpoint_dir/spec.md" <<'EOF'
# Spec: readiness smoke missing checkpoint

## 계획 스냅샷

- 목표: smoke 실패(체크포인트 누락)
- 범위: readiness 누락 케이스
- 검증 명령: `echo smoke`
- 완료 기준: readiness가 실패한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | smoke | `echo smoke` | 최소 샘플 |
EOF

  cat > "$missing_evidence_dir/open-questions.md" <<'EOF'
# Open Questions

현재 미결 항목 없음.
EOF

  cat > "$missing_evidence_dir/spec.md" <<'EOF'
# Spec: readiness smoke missing evidence

## 계획 스냅샷

- 목표: smoke 실패(증거 누락)
- 범위: readiness 누락 케이스
- 검증 명령: `echo smoke`
- 완료 기준: readiness가 실패한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | smoke | `echo smoke` | 최소 샘플 |

## 완료/미완료/다음 액션

- 완료: 없음
- 미완료: C1
- 다음 액션: 샘플 갱신
EOF

  cat > "$placeholder_plan_dir/open-questions.md" <<'EOF'
# Open Questions

현재 미결 항목 없음.
EOF

  cat > "$placeholder_plan_dir/spec.md" <<'EOF'
# Spec: readiness smoke placeholder plan

## 계획 스냅샷

- 목표: 작성 예정
- 범위: readiness 누락 케이스
- 검증 명령: `echo smoke`
- 완료 기준: readiness가 실패한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | smoke | `echo smoke` | 최소 샘플 |

## 완료/미완료/다음 액션

- 완료: 없음
- 미완료: C1
- 다음 액션: 샘플 갱신
- 검증 증거: `echo smoke`
EOF

  cat > "$missing_verify_cmd_dir/open-questions.md" <<'EOF'
# Open Questions

현재 미결 항목 없음.
EOF

  cat > "$missing_verify_cmd_dir/spec.md" <<'EOF'
# Spec: readiness smoke missing verify command

## 계획 스냅샷

- 목표: smoke 실패(Verify command 누락)
- 범위: readiness 누락 케이스
- 검증 명령: `echo smoke`
- 완료 기준: readiness가 실패한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | smoke | - | 최소 샘플 |

## 완료/미완료/다음 액션

- 완료: 없음
- 미완료: C1
- 다음 액션: 샘플 갱신
- 검증 증거: `echo smoke`
EOF

  cat > "$open_question_valid_file" <<'EOF'
# Open Questions

## Q1

- description: 스키마 검증 강화 범위를 확정한다.
- options:
  - A) 카드 단위 라벨 강제
  - B) 기존 키워드 탐지만 유지
- pros: 우회 가능성이 줄어든다.
- cons: 문서 작성 형식이 조금 더 엄격해진다.
- recommended: A) 카드 단위 라벨 강제를 채택한다.
EOF

  cat > "$open_question_noise_file" <<'EOF'
# Open Questions

## Q1

description options pros cons recommended
EOF
}

cleanup() {
  if [[ "$smoke_work_todo_created" -eq 1 && -n "$smoke_work_todo_dir" && -d "$smoke_work_todo_dir" ]]; then
    rm -rf "$smoke_work_todo_dir"
  fi
  if [[ -n "$tmp_root" && -d "$tmp_root" ]]; then
    rm -rf "$tmp_root"
  fi
  if [[ -n "$multi_todo_dir_a" && -d "$multi_todo_dir_a" ]]; then
    rm -rf "$multi_todo_dir_a"
  fi
  if [[ -n "$multi_todo_dir_b" && -d "$multi_todo_dir_b" ]]; then
    rm -rf "$multi_todo_dir_b"
  fi
  if [[ -n "$todo_closure_repo_dir" && -d "$todo_closure_repo_dir" ]]; then
    rm -rf "$todo_closure_repo_dir"
  fi

  # `jj` 명령이 중간에 워킹카피를 스냅샷하면, 임시 todo 디렉터리 삭제가
  # 다음 스냅샷 전까지 삭제(diff D)로 남을 수 있다. cleanup 직후 한 번 더
  # 스냅샷해 후속 게이트(check-todo-closure)의 오탐을 방지한다.
  if command -v jj >/dev/null 2>&1; then
    (cd "$ROOT" && jj status >/dev/null 2>&1) || true
  fi
}

trap cleanup EXIT

ensure_work_todo_workspace

setup_multi_todo_workspaces() {
  local suffix
  suffix="$RANDOM"
  multi_todo_work_id_a="script-smoke-multi-${suffix}-a"
  multi_todo_work_id_b="script-smoke-multi-${suffix}-b"
  multi_todo_dir_a="$ROOT/$(todo_workspace_rel_for_work_id "$multi_todo_work_id_a")"
  multi_todo_dir_b="$ROOT/$(todo_workspace_rel_for_work_id "$multi_todo_work_id_b")"

  mkdir -p "$multi_todo_dir_a" "$multi_todo_dir_b"

  for todo_dir in "$multi_todo_dir_a" "$multi_todo_dir_b"; do
    cat > "$todo_dir/open-questions.md" <<'EOF'
# Open Questions

현재 미결 항목 없음.
EOF
    cat > "$todo_dir/spec.md" <<'EOF'
# Spec: script-smoke multi todo

## 계획 스냅샷

- 목표: 복수 todo 자동 점검 smoke
- 범위: strict/release dry-run 점검
- 검증 명령: `echo smoke`
- 완료 기준: 자동 점검 경로가 실패하지 않는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | smoke | `echo smoke` | 복수 todo 경로 점검 |

## 완료/미완료/다음 액션

- 완료: 없음
- 미완료: C1
- 다음 액션: 자동 점검 실행
- 검증 증거: `echo smoke`
EOF
  done
}

setup_closed_work_commit_fixture() {
  ensure_tmp_root

  closed_work_repo_dir="$tmp_root/closed-work-commit"
  mkdir -p \
    "$closed_work_repo_dir/scripts/lib" \
    "$closed_work_repo_dir/docs/todo-$CLOSED_WORK_FIXTURE_WORK_ID"

  cp "$ROOT/scripts/check-release-gates.sh" "$closed_work_repo_dir/scripts/check-release-gates.sh"
  cp "$ROOT/scripts/lib/todo-workspace.sh" "$closed_work_repo_dir/scripts/lib/todo-workspace.sh"
  chmod +x "$closed_work_repo_dir/scripts/check-release-gates.sh"

  cat > "$closed_work_repo_dir/docs/todo-$CLOSED_WORK_FIXTURE_WORK_ID/spec.md" <<'EOF'
# Spec: script-smoke closed-work fixture
EOF

  cat > "$closed_work_repo_dir/docs/todo-$CLOSED_WORK_FIXTURE_WORK_ID/open-questions.md" <<'EOF'
# Open Questions

현재 미결 항목 없음.
EOF

  git -C "$closed_work_repo_dir" init -q
  git -C "$closed_work_repo_dir" config user.name "script-smoke"
  git -C "$closed_work_repo_dir" config user.email "script-smoke@example.com"
  git -C "$closed_work_repo_dir" add .
  git -C "$closed_work_repo_dir" commit -qm "feat: initialize closed-work fixture"

  rm -rf "$closed_work_repo_dir/docs/todo-$CLOSED_WORK_FIXTURE_WORK_ID"
  git -C "$closed_work_repo_dir" add -A
  git -C "$closed_work_repo_dir" commit -qm "chore: close closed-work fixture"
}

setup_todo_closure_fixture() {
  ensure_tmp_root

  todo_closure_repo_dir="$tmp_root/todo-closure"
  mkdir -p \
    "$todo_closure_repo_dir/scripts/lib" \
    "$todo_closure_repo_dir/docs/todo-$TODO_CLOSURE_WORK_ID"

  cp "$ROOT/scripts/check-todo-closure.sh" "$todo_closure_repo_dir/scripts/check-todo-closure.sh"
  cp "$ROOT/scripts/lib/todo-workspace.sh" "$todo_closure_repo_dir/scripts/lib/todo-workspace.sh"
  chmod +x "$todo_closure_repo_dir/scripts/check-todo-closure.sh"

  cat > "$todo_closure_repo_dir/docs/LESSONS_LOG.md" <<'EOF'
# LESSONS LOG
EOF

  cat > "$todo_closure_repo_dir/docs/LESSONS_ARCHIVE.md" <<'EOF'
# LESSONS ARCHIVE
EOF

  cat > "$todo_closure_repo_dir/docs/todo-$TODO_CLOSURE_WORK_ID/spec.md" <<'EOF'
# Spec: script-smoke todo-closure fixture
EOF

  cat > "$todo_closure_repo_dir/docs/todo-$TODO_CLOSURE_WORK_ID/open-questions.md" <<'EOF'
# Open Questions

현재 미결 항목 없음.
EOF

  git -C "$todo_closure_repo_dir" init -q
  git -C "$todo_closure_repo_dir" config user.name "script-smoke"
  git -C "$todo_closure_repo_dir" config user.email "script-smoke@example.com"
  git -C "$todo_closure_repo_dir" add .
  git -C "$todo_closure_repo_dir" commit -qm "feat: initialize todo-closure fixture"

  rm -rf "$todo_closure_repo_dir/docs/todo-$TODO_CLOSURE_WORK_ID"
  git -C "$todo_closure_repo_dir" add -A
}

while IFS= read -r script; do
  [[ -z "$script" ]] && continue
  run_cmd "bash -n $script"
done < <(find "$ROOT/scripts" -maxdepth 1 -type f -name 'check-*.sh' | sort)

push_strict_cmd="scripts/check-push-gates.sh --mode strict --dry-run --work-id $RESOLVED_WORK_ID"
release_full_cmd="scripts/check-release-gates.sh --manifest-mode full --dry-run --work-id $RESOLVED_WORK_ID"
finalize_auto_cmd="DEBUG_GATES_OVERRIDE=1 scripts/finalize-and-push.sh --message \"chore: script-smoke auto-work-id\" --allow-empty-at --dry-run --work-id $RESOLVED_WORK_ID"

run_cmd "scripts/check-open-questions-schema.sh"
run_cmd "scripts/check-open-questions-schema.sh --require-closed"
run_cmd "scripts/check-jj-conflicts.sh"
run_cmd "scripts/check-doc-last-verified.sh"
run_cmd "scripts/check-doc-links.sh"
run_cmd "scripts/check-doc-index.sh"
run_cmd "scripts/check-readme-policy.sh"
run_cmd "scripts/check-manifest-entrypoints.sh"
run_cmd "bash -n scripts/finalize-and-push.sh"
run_cmd "bash -n scripts/jj-git-push-safe.sh"
run_cmd "bash -n scripts/start-work.sh"
run_cmd "scripts/check-push-gates.sh --mode quick --dry-run"
run_cmd "$push_strict_cmd"
run_debug_override_cmd "DEBUG_GATES_OVERRIDE=1 scripts/check-push-gates.sh --mode strict --allow-missing-work-id --dry-run"
expect_fail_cmd "CI=1 DEBUG_GATES_OVERRIDE=1 scripts/check-push-gates.sh --mode strict --allow-missing-work-id --dry-run"
run_cmd "$release_full_cmd"
run_debug_override_cmd "DEBUG_GATES_OVERRIDE=1 scripts/check-release-gates.sh --manifest-mode quick --allow-quick-manifest --allow-missing-work-id --dry-run"
expect_fail_cmd "CI=1 DEBUG_GATES_OVERRIDE=1 scripts/check-release-gates.sh --manifest-mode quick --allow-quick-manifest --allow-missing-work-id --dry-run"
run_cmd "scripts/run-manifest-checks.sh --mode quick --dry-run"
expect_fail_cmd "scripts/run-manifest-checks.sh --mode full --work-id $MISSING_WORK_ID_SUFFIX"
expect_fail_cmd "scripts/check-release-gates.sh --manifest-mode full --dry-run --work-id $MISSING_WORK_ID_SUFFIX"
expect_fail_cmd "scripts/check-push-gates.sh --mode strict --dry-run --work-id $MISSING_WORK_ID_SUFFIX"
expect_fail_cmd "DEBUG_GATES_OVERRIDE=1 scripts/finalize-and-push.sh --message \"chore: script-smoke missing-work-id\" --allow-empty-at --dry-run --work-id $MISSING_WORK_ID_SUFFIX"
run_cmd "scripts/start-work.sh --work-id script-smoke-start-work --dry-run"
run_debug_override_cmd "$finalize_auto_cmd"
run_debug_override_cmd "DEBUG_GATES_OVERRIDE=1 scripts/finalize-and-push.sh --message \"chore: script-smoke dry-run\" --manifest-mode quick --allow-quick-manifest --allow-missing-work-id --allow-empty-at --dry-run"
run_debug_override_cmd "output=\$(DEBUG_GATES_OVERRIDE=1 scripts/finalize-and-push.sh --message \"chore: script-smoke quick-flag\" --manifest-mode quick --allow-quick-manifest --allow-missing-work-id --allow-empty-at --dry-run); [[ \"\$output\" == *\"scripts/check-release-gates.sh --manifest-mode quick --allow-quick-manifest --allow-missing-work-id\"* ]]"
run_debug_override_cmd "output=\$(DEBUG_GATES_OVERRIDE=1 scripts/finalize-and-push.sh --message \"chore: script-smoke remote\" --manifest-mode quick --allow-quick-manifest --allow-missing-work-id --allow-empty-at --remote upstream --dry-run); [[ \"\$output\" == *\"scripts/jj-git-push-safe.sh --remote upstream --bookmark main\"* ]]"
expect_fail_cmd "scripts/finalize-and-push.sh --message \"feat(scope): script-smoke invalid scope\" --dry-run"
expect_fail_cmd "scripts/finalize-and-push.sh --message \"invalid-message-format\" --dry-run"
expect_fail_cmd "scripts/finalize-and-push.sh --message \"chore: script-smoke invalid remote\" --remote __script_smoke_missing_remote__"
expect_fail_cmd "CI=1 DEBUG_GATES_OVERRIDE=1 scripts/finalize-and-push.sh --message \"chore: script-smoke ci-block\" --manifest-mode quick --allow-quick-manifest --allow-missing-work-id --allow-empty-at --dry-run"
expect_fail_cmd "DEBUG_GATES_OVERRIDE=1 PUSH_GATES_MODE=quick ALLOW_NON_STRICT_PUSH_GATES=1 scripts/jj-git-push-safe.sh --dry-run"
expect_fail_cmd "CI=1 DEBUG_GATES_OVERRIDE=1 PUSH_GATES_MODE=quick ALLOW_NON_STRICT_PUSH_GATES=1 scripts/jj-git-push-safe.sh --bookmark main --dry-run"
expect_fail_cmd "scripts/check-push-gates.sh --mode"
expect_fail_cmd "scripts/check-release-gates.sh --manifest-mode"
expect_fail_cmd "scripts/check-jj-conflicts.sh --bookmark"
expect_fail_cmd "scripts/run-manifest-checks.sh --repo-key"
expect_fail_cmd "scripts/start-work.sh --work-id"
expect_fail_cmd "scripts/finalize-and-push.sh --message"

setup_multi_todo_workspaces
run_cmd "scripts/check-push-gates.sh --mode strict --dry-run"
run_cmd "scripts/check-release-gates.sh --manifest-mode full --dry-run"
run_debug_override_cmd "DEBUG_GATES_OVERRIDE=1 scripts/finalize-and-push.sh --message \"chore: script-smoke multi-auto\" --allow-empty-at --dry-run"
cat > "$multi_todo_dir_b/open-questions.md" <<'EOF'
# Open Questions

## Q1

- description: 다중 todo 환경에서 --work-id 스코프 게이트 동작을 점검한다.
- options:
  - A) --work-id 지정 시 타깃 todo만 검사
  - B) 전체 todo를 항상 검사
- pros: 병렬 작업 중 타깃 작업 출고를 불필요하게 막지 않는다.
- cons: 비타깃 todo의 질문은 해당 작업 턴에서 별도 점검이 필요하다.
- recommended: A) --work-id 지정 시 타깃 todo만 검사한다.
EOF
run_cmd "output=\$(scripts/check-push-gates.sh --mode strict --dry-run --work-id $multi_todo_work_id_a); [[ \"\$output\" == *\"scripts/check-open-questions-schema.sh --require-closed docs/todo-$multi_todo_work_id_a/open-questions.md\"* ]]"
run_cmd "output=\$(scripts/check-release-gates.sh --manifest-mode full --dry-run --work-id $multi_todo_work_id_a); [[ \"\$output\" == *\"scripts/check-open-questions-schema.sh --require-closed docs/todo-$multi_todo_work_id_a/open-questions.md\"* ]]"
run_cmd "output=\$(scripts/check-push-gates.sh --mode strict --dry-run); [[ \"\$output\" == *\"scripts/check-open-questions-schema.sh --require-closed\"* ]] && [[ \"\$output\" == *\"docs/todo-$multi_todo_work_id_a/open-questions.md\"* ]] && [[ \"\$output\" == *\"docs/todo-$multi_todo_work_id_b/open-questions.md\"* ]]"
run_cmd "output=\$(scripts/check-release-gates.sh --manifest-mode full --dry-run); [[ \"\$output\" == *\"scripts/check-open-questions-schema.sh --require-closed\"* ]] && [[ \"\$output\" == *\"docs/todo-$multi_todo_work_id_a/open-questions.md\"* ]] && [[ \"\$output\" == *\"docs/todo-$multi_todo_work_id_b/open-questions.md\"* ]]"

setup_closed_work_commit_fixture
run_cmd "(cd \"$closed_work_repo_dir\" && scripts/check-release-gates.sh --manifest-mode full --dry-run)"
expect_fail_cmd "(cd \"$closed_work_repo_dir\" && printf 'dirty fixture\\n' > README.md && scripts/check-release-gates.sh --manifest-mode full --dry-run --work-id $CLOSED_WORK_FIXTURE_WORK_ID)"

setup_todo_closure_fixture
expect_fail_cmd "(cd \"$todo_closure_repo_dir\" && scripts/check-todo-closure.sh)"
run_cmd "(cd \"$todo_closure_repo_dir\" && printf '\\n- todo-$TODO_CLOSURE_WORK_ID\\n' >> docs/LESSONS_LOG.md && git add docs/LESSONS_LOG.md && scripts/check-todo-closure.sh)"
run_cmd "(cd \"$todo_closure_repo_dir\" && git commit -qm \"docs: record todo-$TODO_CLOSURE_WORK_ID closure\" && scripts/check-todo-closure.sh)"

setup_todo_readiness_fixtures
valid_rel="${valid_dir#$ROOT/}"
missing_checkpoint_rel="${missing_checkpoint_dir#$ROOT/}"
missing_evidence_rel="${missing_evidence_dir#$ROOT/}"
placeholder_plan_rel="${placeholder_plan_dir#$ROOT/}"
missing_verify_cmd_rel="${missing_verify_cmd_dir#$ROOT/}"
open_question_valid_rel="${open_question_valid_file#$ROOT/}"
open_question_noise_rel="${open_question_noise_file#$ROOT/}"

run_cmd "scripts/check-todo-readiness.sh $valid_rel"
expect_fail_cmd "scripts/check-todo-readiness.sh $missing_checkpoint_rel"
expect_fail_cmd "scripts/check-todo-readiness.sh $missing_evidence_rel"
expect_fail_cmd "scripts/check-todo-readiness.sh $placeholder_plan_rel"
expect_fail_cmd "scripts/check-todo-readiness.sh $missing_verify_cmd_rel"
run_cmd "scripts/check-open-questions-schema.sh $open_question_valid_rel"
run_cmd "scripts/check-open-questions-schema.sh --require-closed $valid_rel/open-questions.md"
expect_fail_cmd "scripts/check-open-questions-schema.sh --require-closed $open_question_valid_rel"
expect_fail_cmd "scripts/check-open-questions-schema.sh $open_question_noise_rel"

if (( fail_count > 0 )); then
  echo "[FAIL] script smoke check failed with $fail_count issue(s)" >&2
  exit 1
fi

ok "script smoke check passed"
