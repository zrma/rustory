#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/todo-workspace.sh
source "$ROOT/scripts/lib/todo-workspace.sh"
fail_count=0
warn_count=0

resolve_python_cmd() {
  local candidate
  for candidate in python3 python; do
    if ! command -v "$candidate" >/dev/null 2>&1; then
      continue
    fi
    if "$candidate" - <<'PY' >/dev/null 2>&1
import sys
raise SystemExit(0 if sys.version_info.major >= 3 else 1)
PY
    then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
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

usage() {
  cat <<'EOF'
Check stale completed docs/todo-* workspaces.

Usage:
  scripts/check-todo-closure.sh

Rules:
  - If all C-checklist items are `done` and open-questions is closed,
    the todo workspace must be migrated and deleted.
  - Root-level docs/archive-* directories are forbidden. Keep evidence in
    canonical docs and docs/LESSONS_LOG.md (+ docs/LESSONS_ARCHIVE.md only).
  - If docs/todo-<work-id> deletion is detected in diff/commit, keep
    `todo-<work-id>` reference in docs/LESSONS_LOG.md or docs/LESSONS_ARCHIVE.md.
EOF
}

collect_deleted_todo_work_ids() {
  if ! command -v git >/dev/null 2>&1; then
    fail "git 명령을 찾을 수 없어 삭제된 todo 검증을 수행할 수 없음"
    return 0
  fi

  if ! git -C "$ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    fail "git worktree가 아니어서 삭제된 todo 검증을 수행할 수 없음"
    return 0
  fi

  todo_workspace_collect_deleted_work_ids "$ROOT" auto
}

check_deleted_todo_lessons_refs() {
  local lessons_log="$ROOT/docs/LESSONS_LOG.md"
  local lessons_archive="$ROOT/docs/LESSONS_ARCHIVE.md"
  local work_id
  local token
  local -a deleted_work_ids=()

  readarray -t deleted_work_ids < <(collect_deleted_todo_work_ids)
  if (( ${#deleted_work_ids[@]} == 0 )); then
    return 0
  fi

  for work_id in "${deleted_work_ids[@]}"; do
    [[ -z "$work_id" ]] && continue
    token="todo-$work_id"

    if grep -Fq "$token" "$lessons_log" 2>/dev/null || grep -Fq "$token" "$lessons_archive" 2>/dev/null; then
      ok "$token: LESSONS_LOG/ARCHIVE 참조 확인"
      continue
    fi

    fail "$token: docs/LESSONS_LOG.md 또는 docs/LESSONS_ARCHIVE.md에 참조가 필요함"
  done
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if ! PYTHON_BIN="$(resolve_python_cmd)"; then
  fail "python3-compatible interpreter is required (python3 or python)"
  echo "[FAIL] todo closure check failed with $fail_count issue(s)" >&2
  exit 1
fi

if ! todo_workspace_load_config "$ROOT"; then
  fail "failed to load todo workspace config from docs/REPO_MANIFEST.yaml"
  echo "[FAIL] todo closure check failed with $fail_count issue(s)" >&2
  exit 1
fi

check_deleted_todo_lessons_refs

checklist_state() {
  local spec_file="$1"
  "$PYTHON_BIN" - "$spec_file" <<'PY'
import re
import sys
from collections import Counter
from pathlib import Path

spec_path = Path(sys.argv[1])
text = spec_path.read_text(encoding="utf-8")
allowed = {"todo", "in_progress", "done"}

states = []
ids = []
invalid_statuses = []
for line in text.splitlines():
    line = line.strip()
    if not line.startswith("|"):
        continue
    cols = [c.strip() for c in line.strip("|").split("|")]
    if len(cols) < 2:
        continue
    cid = cols[0]
    status = cols[1].lower()
    if re.fullmatch(r"C\d+", cid):
        ids.append(cid)
        states.append(status)
        if status not in allowed:
            invalid_statuses.append(f"{cid}:{cols[1]}")

if not states:
    print("missing")
    sys.exit(0)

dup_ids = sorted(
    [cid for cid, count in Counter(ids).items() if count > 1],
    key=lambda item: int(item[1:]),
)
if dup_ids:
    print("invalid-duplicate:" + ",".join(dup_ids))
    sys.exit(0)

numbers = sorted(int(cid[1:]) for cid in ids)
number_set = set(numbers)
if 1 not in number_set:
    print("invalid-missing-c1")
    sys.exit(0)

expected = list(range(1, max(numbers) + 1))
missing_numbers = [n for n in expected if n not in number_set]
if missing_numbers:
    print("invalid-non-contiguous:" + ",".join(f"C{n}" for n in missing_numbers))
    sys.exit(0)

if invalid_statuses:
    print("invalid-status:" + ",".join(invalid_statuses))
    sys.exit(0)

if all(state == "done" for state in states):
    print("done")
else:
    print("active")
PY
}

open_questions_closed() {
  local open_q="$1"
  local body
  body="$(
    sed \
      -e '/^[[:space:]]*#/d' \
      -e '/^[[:space:]]*$/d' \
      -e 's/^[[:space:]]*//' \
      -e 's/[[:space:]]*$//' \
      "$open_q"
  )"
  [[ "$body" == "현재 미결 항목 없음." ]]
}

collect_external_refs() {
  local todo_rel="$1"
  local ref
  local -a search_targets=(
    "$ROOT/README.md"
    "$ROOT/AGENTS.md"
    "$ROOT/docs"
    "$ROOT/scripts"
    "$ROOT/.github"
    "$ROOT/modules"
  )
  if command -v rg >/dev/null 2>&1; then
    while IFS= read -r ref; do
      [[ -z "$ref" ]] && continue
      # Ignore self-references inside the same todo directory.
      if [[ "$ref" == "$todo_rel/"* ]]; then
        continue
      fi
      echo "$ref"
    done < <(
      rg -n --no-heading --fixed-strings "$todo_rel" \
        "${search_targets[@]}" \
        -g '*.md' -g '*.sh' -g '*.yml' -g '*.yaml' -g '*.txt' 2>/dev/null || true
    )
    return
  fi

  warn "rg not found; fallback to grep for todo reference scan"
  while IFS= read -r ref; do
    [[ -z "$ref" ]] && continue
    if [[ "$ref" == "$todo_rel/"* ]]; then
      continue
    fi
    echo "$ref"
  done < <(
    grep -Rsn --fixed-strings \
      --include='*.md' --include='*.sh' --include='*.yml' --include='*.yaml' --include='*.txt' \
      "$todo_rel" "${search_targets[@]}" 2>/dev/null || true
  )
}

readarray -t TODO_DIRS < <(todo_workspace_find_dirs "$ROOT")
readarray -t ARCHIVE_DIRS < <(find "$ROOT/docs" -maxdepth 1 -mindepth 1 -type d -name 'archive-*' | sort)

if [[ "${#ARCHIVE_DIRS[@]}" -gt 0 ]]; then
  for archive_dir in "${ARCHIVE_DIRS[@]}"; do
    archive_rel="${archive_dir#$ROOT/}"
    fail "$archive_rel: 사용 금지 경로. todo 완료 산출물은 정식 문서/LESSONS_LOG로 내재화 후 원본 디렉터리를 삭제해야 함"
  done
fi

if [[ "${#TODO_DIRS[@]}" -eq 0 ]]; then
  if (( fail_count > 0 )); then
    echo "[FAIL] todo closure check failed with $fail_count issue(s)" >&2
    exit 1
  fi
  ok "no todo workspace found"
  exit 0
fi

for todo_abs in "${TODO_DIRS[@]}"; do
  todo_rel="${todo_abs#$ROOT/}"
  spec="$todo_abs/spec.md"
  open_q="$todo_abs/open-questions.md"

  if [[ ! -f "$spec" ]]; then
    fail "$todo_rel: missing spec.md"
    continue
  fi
  if [[ ! -f "$open_q" ]]; then
    fail "$todo_rel: missing open-questions.md"
    continue
  fi

  state="$(checklist_state "$spec")"
  if [[ "$state" == "missing" ]]; then
    fail "$todo_rel: C-체크리스트를 찾을 수 없음"
    continue
  fi
  if [[ "$state" == invalid-duplicate:* ]]; then
    fail "$todo_rel: C-체크리스트 ID 중복 (${state#invalid-duplicate:})"
    continue
  fi
  if [[ "$state" == "invalid-missing-c1" ]]; then
    fail "$todo_rel: C-체크리스트는 C1부터 시작해야 함"
    continue
  fi
  if [[ "$state" == invalid-non-contiguous:* ]]; then
    fail "$todo_rel: C-체크리스트 ID 누락 (${state#invalid-non-contiguous:})"
    continue
  fi
  if [[ "$state" == invalid-status:* ]]; then
    fail "$todo_rel: C-체크리스트 상태값 오류 (${state#invalid-status:}); allowed=todo|in_progress|done"
    continue
  fi

  closed_open_q=0
  if open_questions_closed "$open_q"; then
    closed_open_q=1
  fi

  if [[ "$state" == "done" && "$closed_open_q" -eq 1 ]]; then
    readarray -t refs < <(collect_external_refs "$todo_rel")
    if [[ "${#refs[@]}" -gt 0 ]]; then
      fail "$todo_rel: 완료된 todo를 참조하는 외부 경로가 남아 있음 (정리 후 삭제 필요)"
      for ref in "${refs[@]}"; do
        echo "       - $ref"
      done
      continue
    fi
    fail "$todo_rel: C-체크리스트가 모두 done이며 open-questions가 닫힘 상태. 관련 문서로 이관 후 폴더를 삭제해야 함"
    continue
  fi

  if [[ "$state" == "done" && "$closed_open_q" -eq 0 ]]; then
    fail "$todo_rel: C-체크리스트는 done이지만 open-questions가 닫히지 않음"
    continue
  fi

  warn "$todo_rel: 진행 중(todo/in_progress) 상태로 간주"
done

if (( fail_count > 0 )); then
  echo "[FAIL] todo closure check failed with $fail_count issue(s)" >&2
  exit 1
fi

if (( warn_count > 0 )); then
  echo "[WARN] todo closure check passed with $warn_count warning(s)"
else
  ok "todo closure check passed"
fi
