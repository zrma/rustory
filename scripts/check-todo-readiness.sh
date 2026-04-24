#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/todo-workspace.sh
source "$ROOT/scripts/lib/todo-workspace.sh"

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

validate_plan_snapshot_schema() {
  local spec_file="$1"
  "$PYTHON_BIN" - "$spec_file" <<'PY'
import re
import sys
from pathlib import Path

spec_path = Path(sys.argv[1])
text = spec_path.read_text(encoding="utf-8")

section_match = re.search(
    r"(?ms)^##\s*계획 스냅샷\s*$\n?(.*?)(?=^##\s+|\Z)",
    text,
)
if not section_match:
    print("missing-section")
    raise SystemExit(2)

section = section_match.group(1).strip()
if not section:
    print("empty")
    raise SystemExit(2)

required_patterns = [
    ("목표", r"(?im)^(?:[-*]\s*)?(?:\*\*|__|`)?목표(?:\*\*|__|`)?\s*[:：]\s*(.+?)\s*$"),
    ("범위", r"(?im)^(?:[-*]\s*)?(?:\*\*|__|`)?범위(?:\*\*|__|`)?\s*[:：]\s*(.+?)\s*$"),
    (
        "검증 명령",
        r"(?im)^(?:[-*]\s*)?(?:\*\*|__|`)?(?:검증\s*명령|verify\s*command(?:s)?)"
        r"(?:\*\*|__|`)?\s*[:：]\s*(.+?)\s*$",
    ),
    ("완료 기준", r"(?im)^(?:[-*]\s*)?(?:\*\*|__|`)?완료\s*기준(?:\*\*|__|`)?\s*[:：]\s*(.+?)\s*$"),
]

missing = []
placeholder_pattern = re.compile(
    r"(?i)^(?:-|tbd|todo|n/?a|none|미정|작성\s*예정|추후\s*작성(?:\s*예정)?|pending|to\s*be\s*determined)$"
)
placeholder_contains_pattern = re.compile(
    r"(?i)(?:\btbd\b|미정|작성\s*예정|추후\s*작성(?:\s*예정)?|to\s*be\s*determined)"
)
placeholder_fields = []

def normalize_value(raw: str) -> str:
    value = raw.strip()
    value = re.sub(r"^[`'\"“”‘’]+|[`'\"“”‘’]+$", "", value).strip()
    value = re.sub(r"^\((.*)\)$", r"\1", value).strip()
    return value

for label, pattern in required_patterns:
    match = re.search(pattern, section)
    if not match:
        missing.append(label)
        continue

    field_value = normalize_value(match.group(1))
    if not field_value:
        placeholder_fields.append(label)
        continue

    if placeholder_pattern.fullmatch(field_value.lower()) or placeholder_contains_pattern.search(field_value):
        placeholder_fields.append(label)

if missing:
    print("missing-fields:" + ",".join(missing))
    raise SystemExit(2)

if placeholder_fields:
    print("placeholder-fields:" + ",".join(placeholder_fields))
    raise SystemExit(2)

print("valid")
PY
}

validate_checklist_schema() {
  local spec_file="$1"
  "$PYTHON_BIN" - "$spec_file" <<'PY'
import re
import sys
from collections import Counter
from pathlib import Path

spec_path = Path(sys.argv[1])
text = spec_path.read_text(encoding="utf-8")
allowed = {"todo", "in_progress", "done"}

ids = []
invalid_statuses = []
invalid_verify_commands = []
verify_placeholder_pattern = re.compile(
    r"(?i)^(?:-|tbd|todo|n/?a|none|없음|미정|작성\s*예정|추후\s*작성(?:\s*예정)?|pending)$"
)
verify_placeholder_contains_pattern = re.compile(
    r"(?i)(?:\btbd\b|없음|미정|작성\s*예정|추후\s*작성(?:\s*예정)?|to\s*be\s*determined)"
)
for raw in text.splitlines():
    line = raw.strip()
    if not line.startswith("|"):
        continue
    cols = [c.strip() for c in line.strip("|").split("|")]
    if len(cols) < 2:
        continue
    cid = cols[0]
    status_raw = cols[1]
    status = status_raw.lower()
    if not re.fullmatch(r"C\d+", cid):
        continue
    ids.append(cid)
    if status not in allowed:
        invalid_statuses.append(f"{cid}:{status_raw}")

    if len(cols) < 4:
        invalid_verify_commands.append(f"{cid}:missing-column")
        continue

    verify_raw = cols[3].strip()
    verify_value = re.sub(r"`", "", verify_raw).strip()
    verify_value = re.sub(r"^\((.*)\)$", r"\1", verify_value).strip()
    if not verify_value:
        invalid_verify_commands.append(f"{cid}:empty")
        continue

    if verify_placeholder_pattern.fullmatch(verify_value.lower()) or verify_placeholder_contains_pattern.search(verify_value):
        invalid_verify_commands.append(f"{cid}:placeholder")

if not ids:
    print("missing-checklist-items")
    raise SystemExit(2)

dup_ids = sorted(
    [cid for cid, count in Counter(ids).items() if count > 1],
    key=lambda item: int(item[1:]),
)
if dup_ids:
    print("duplicate-ids:" + ",".join(dup_ids))
    raise SystemExit(2)

numbers = sorted(int(cid[1:]) for cid in ids)
number_set = set(numbers)
if 1 not in number_set:
    print("missing-c1")
    raise SystemExit(2)

expected = list(range(1, max(numbers) + 1))
missing_numbers = [n for n in expected if n not in number_set]
if missing_numbers:
    print("non-contiguous:" + ",".join(f"C{n}" for n in missing_numbers))
    raise SystemExit(2)

if invalid_statuses:
    print("invalid-status:" + ",".join(invalid_statuses))
    raise SystemExit(2)

if invalid_verify_commands:
    print("invalid-verify-command:" + ",".join(invalid_verify_commands))
    raise SystemExit(2)

print("valid")
PY
}

validate_progress_checkpoint_schema() {
  local spec_file="$1"
  "$PYTHON_BIN" - "$spec_file" <<'PY'
import re
import sys
from pathlib import Path

spec_path = Path(sys.argv[1])
text = spec_path.read_text(encoding="utf-8")

section_match = re.search(
    r"(?ms)^##\s*완료\s*/\s*미완료\s*/\s*다음\s*액션\s*$\n?(.*?)(?=^##\s+|\Z)",
    text,
)
if not section_match:
    print("missing-section")
    raise SystemExit(2)

section = section_match.group(1).strip()
if not section:
    print("empty")
    raise SystemExit(2)

required_patterns = [
    ("완료", r"(?im)^(?:[-*]\s*)?(?:\*\*|__|`)?완료(?:\*\*|__|`)?\s*[:：]"),
    ("미완료", r"(?im)^(?:[-*]\s*)?(?:\*\*|__|`)?미완료(?:\*\*|__|`)?\s*[:：]"),
    (
        "다음 액션",
        r"(?im)^(?:[-*]\s*)?(?:\*\*|__|`)?다음\s*액션(?:\*\*|__|`)?\s*[:：]",
    ),
]

missing = []
for label, pattern in required_patterns:
    if not re.search(pattern, section):
        missing.append(label)

if missing:
    print("missing-fields:" + ",".join(missing))
    raise SystemExit(2)

has_evidence_label = bool(
    re.search(
        r"(?im)^(?:[-*]\s*)?(?:\*\*|__|`)?"
        r"(?:검증\s*증거|검증\s*명령|실행\s*명령|산출물(?:\s*식별자)?)"
        r"(?:\*\*|__|`)?\s*[:：]",
        section,
    )
)
has_inline_code = bool(re.search(r"`[^`\n]+`", section))

if not (has_evidence_label or has_inline_code):
    print("missing-evidence")
    raise SystemExit(2)

print("valid")
PY
}

ok() {
  echo "[ OK ] $*"
}

fail() {
  echo "[FAIL] $*" >&2
  exit 1
}

match_in_file() {
  local pattern="$1"
  local file="$2"

  if command -v rg >/dev/null 2>&1; then
    rg -q "$pattern" "$file"
    return $?
  fi

  grep -Eq "$pattern" "$file"
}

usage() {
  cat <<'EOF'
Check todo artifact readiness before implementation.

Usage:
  scripts/check-todo-readiness.sh [docs/todo-<work-id>]

Rules:
  - spec.md must exist and include:
    - "## 계획 스냅샷"
    - 계획 스냅샷 필수 항목:
      - 목표:
      - 범위:
      - 검증 명령: (또는 Verify command:)
      - 완료 기준:
      - 각 필드 값은 placeholder(`작성 예정`, `TBD`, `-` 등) 금지
    - "## 완료/미완료/다음 액션"
    - 체크포인트 필수 항목:
      - 완료:
      - 미완료:
      - 다음 액션:
      - 검증 증거/실행 명령/산출물 식별자(또는 인라인 코드 1개 이상)
    - "## C-체크리스트"
    - at least one `C1..Cn` row in the checklist table
    - checklist IDs start at `C1` and are contiguous without gaps
    - status values are only: `todo | in_progress | done`
    - each `C1..Cn` row must include non-placeholder Verify command
  - open-questions.md must exist and be closed:
    - body should be effectively "현재 미결 항목 없음."
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if ! todo_workspace_load_config "$ROOT"; then
  fail "failed to load todo workspace config from docs/REPO_MANIFEST.yaml"
fi

TARGET_DIR="${1:-}"
if [[ -z "$TARGET_DIR" ]]; then
  mapfile -t TODO_DIRS < <(todo_workspace_find_dirs "$ROOT")
  if (( ${#TODO_DIRS[@]} == 0 )); then
    fail "no todo directory found (expected '$TODO_WORKSPACE_GLOB')"
  elif (( ${#TODO_DIRS[@]} > 1 )); then
    echo "[FAIL] multiple todo directories found. specify one explicitly:" >&2
    printf '  - %s\n' "${TODO_DIRS[@]#$ROOT/}" >&2
    exit 1
  fi
  TARGET_DIR="${TODO_DIRS[0]#$ROOT/}"
fi

if [[ ! -d "$ROOT/$TARGET_DIR" ]]; then
  fail "todo directory not found: $TARGET_DIR"
fi

if ! PYTHON_BIN="$(resolve_python_cmd)"; then
  fail "python3-compatible interpreter is required (python3 or python)"
fi

SPEC="$ROOT/$TARGET_DIR/spec.md"
OPEN_Q="$ROOT/$TARGET_DIR/open-questions.md"

[[ -f "$SPEC" ]] || fail "missing spec: $TARGET_DIR/spec.md"
[[ -f "$OPEN_Q" ]] || fail "missing open-questions: $TARGET_DIR/open-questions.md"

if ! match_in_file '^## 계획 스냅샷' "$SPEC"; then
  fail "spec missing required section: '## 계획 스냅샷' ($TARGET_DIR/spec.md)"
fi
if ! match_in_file '^##[[:space:]]*완료[[:space:]]*/[[:space:]]*미완료[[:space:]]*/[[:space:]]*다음[[:space:]]*액션[[:space:]]*$' "$SPEC"; then
  fail "spec missing required section: '## 완료/미완료/다음 액션' ($TARGET_DIR/spec.md)"
fi
if ! match_in_file '^## C-체크리스트' "$SPEC"; then
  fail "spec missing required section: '## C-체크리스트' ($TARGET_DIR/spec.md)"
fi

plan_result="$(validate_plan_snapshot_schema "$SPEC" 2>/dev/null || true)"
case "$plan_result" in
  valid)
    ;;
  missing-section)
    fail "spec missing required section: '## 계획 스냅샷' ($TARGET_DIR/spec.md)"
    ;;
  empty)
    fail "spec 계획 스냅샷 내용이 비어 있음 (필수: 목표/범위/검증 명령/완료 기준): $TARGET_DIR/spec.md"
    ;;
  missing-fields:*)
    fail "spec 계획 스냅샷 필수 항목 누락 (${plan_result#missing-fields:}): $TARGET_DIR/spec.md"
    ;;
  placeholder-fields:*)
    fail "spec 계획 스냅샷 필드 값에 placeholder가 있음 (${plan_result#placeholder-fields:}); 목표/범위/검증 명령/완료 기준은 실행 가능한 값이어야 함: $TARGET_DIR/spec.md"
    ;;
  *)
    fail "failed to parse 계획 스냅샷 schema: $TARGET_DIR/spec.md"
    ;;
esac

checkpoint_result="$(validate_progress_checkpoint_schema "$SPEC" 2>/dev/null || true)"
case "$checkpoint_result" in
  valid)
    ;;
  missing-section)
    fail "spec missing required section: '## 완료/미완료/다음 액션' ($TARGET_DIR/spec.md)"
    ;;
  empty)
    fail "spec 완료/미완료/다음 액션 체크포인트가 비어 있음 (필수: 완료/미완료/다음 액션 + 증적): $TARGET_DIR/spec.md"
    ;;
  missing-fields:*)
    fail "spec 완료/미완료/다음 액션 필수 항목 누락 (${checkpoint_result#missing-fields:}): $TARGET_DIR/spec.md"
    ;;
  missing-evidence)
    fail "spec 완료/미완료/다음 액션에 검증 증거(명령/산출물)가 없음: $TARGET_DIR/spec.md"
    ;;
  *)
    fail "failed to parse 완료/미완료/다음 액션 schema: $TARGET_DIR/spec.md"
    ;;
esac

checklist_result="$(validate_checklist_schema "$SPEC" 2>/dev/null || true)"
case "$checklist_result" in
  valid)
    ;;
  missing-checklist-items)
    fail "spec has no C-checklist items (expected at least one C1..Cn row): $TARGET_DIR/spec.md"
    ;;
  duplicate-ids:*)
    fail "spec has duplicate checklist IDs (${checklist_result#duplicate-ids:}): $TARGET_DIR/spec.md"
    ;;
  missing-c1)
    fail "spec checklist must start from C1: $TARGET_DIR/spec.md"
    ;;
  non-contiguous:*)
    fail "spec checklist has missing IDs (${checklist_result#non-contiguous:}): $TARGET_DIR/spec.md"
    ;;
  invalid-status:*)
    fail "spec checklist has invalid status values (${checklist_result#invalid-status:}); allowed=todo|in_progress|done"
    ;;
  invalid-verify-command:*)
    fail "spec checklist has invalid Verify command values (${checklist_result#invalid-verify-command:}); 각 C항목 Verify command는 비어있거나 placeholder일 수 없음"
    ;;
  *)
    fail "failed to parse C-checklist schema: $TARGET_DIR/spec.md"
    ;;
esac

OPEN_BODY="$(
  sed \
    -e '/^[[:space:]]*#/d' \
    -e '/^[[:space:]]*$/d' \
    -e 's/^[[:space:]]*//' \
    -e 's/[[:space:]]*$//' \
    "$OPEN_Q"
)"

if [[ "$OPEN_BODY" != "현재 미결 항목 없음." ]]; then
  echo "[FAIL] unresolved questions found in $TARGET_DIR/open-questions.md" >&2
  echo "       expected body: '현재 미결 항목 없음.'" >&2
  echo "       actual body:" >&2
  sed 's/^/         /' <<<"$OPEN_BODY" >&2
  exit 1
fi

ok "todo readiness passed: $TARGET_DIR"
