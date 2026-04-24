#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fail_count=0
warn_count=0
REQUIRE_CLOSED=0

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
  cat <<'USAGE'
Validate question-card schema for open-questions files.

Usage:
  scripts/check-open-questions-schema.sh [options] [open-questions.md ...]

Options:
  --require-closed  fail when file is open state (question card exists)
  -h, --help        show help

Rules:
  - Closed state is allowed only when body is exactly: "현재 미결 항목 없음."
  - Open state must include question-card fields:
    - description
    - options
    - pros/cons (or pros + cons)
    - recommended
USAGE
}

declare -a explicit_targets=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --require-closed)
      REQUIRE_CLOSED=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      while (( $# > 0 )); do
        explicit_targets+=("$1")
        shift
      done
      ;;
    -*)
      echo "unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
    *)
      explicit_targets+=("$1")
      shift
      ;;
  esac
done

validate_file() {
  local file="$1"
  "$PYTHON_BIN" - "$file" <<'PY'
import re
import sys
from pathlib import Path

path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")

body_lines = []
for raw in text.splitlines():
    line = raw.strip()
    if not line:
        continue
    if line.startswith("#"):
        continue
    body_lines.append(line)

if body_lines == ["현재 미결 항목 없음."]:
    print("closed")
    raise SystemExit(0)

question_heading_re = re.compile(
    r"(?im)^##+\s*(q\d+|question(?:\s+\d+)?|질문(?:\s*\d+)?)\b.*$"
)

matches = list(question_heading_re.finditer(text))
if not matches:
    print("invalid:question-card-title")
    raise SystemExit(2)

def split_cards(src: str, heading_matches):
    cards = []
    for idx, match in enumerate(heading_matches):
        start = match.start()
        end = heading_matches[idx + 1].start() if idx + 1 < len(heading_matches) else len(src)
        title = match.group(0).strip()
        body = src[start:end]
        cards.append((title, body))
    return cards

def has_labeled_field(block: str, label_pattern: str) -> bool:
    patterns = [
        rf"(?im)^\s*#+\s*{label_pattern}\b",
        rf"(?im)^\s*[-*+]\s*{label_pattern}\s*[:：]",
        rf"(?im)^\s*\d+\.\s*{label_pattern}\s*[:：]",
        rf"(?im)^\s*{label_pattern}\s*[:：]",
    ]
    return any(re.search(p, block) for p in patterns)

cards = split_cards(text, matches)
missing_by_card = []

for title, block in cards:
    missing = []

    if not has_labeled_field(block, r"(description|설명)"):
        missing.append("description")

    if not has_labeled_field(block, r"(options(?:\s*\([^)]*\))?|선택지)"):
        missing.append("options")

    has_pros_cons = has_labeled_field(block, r"(pros\s*/\s*cons|장단점)")
    has_pros = has_labeled_field(block, r"(pros|장점)")
    has_cons = has_labeled_field(block, r"(cons|단점)")
    if not (has_pros_cons or (has_pros and has_cons)):
        missing.append("pros/cons")

    if not has_labeled_field(block, r"(recommended|권장)"):
        missing.append("recommended")

    if missing:
        missing_by_card.append(f"{title}:{'/'.join(missing)}")

if missing_by_card:
    print("invalid:" + ";".join(missing_by_card))
    raise SystemExit(2)

print("open-valid")
PY
}

declare -a targets=()
if (( ${#explicit_targets[@]} > 0 )); then
  for file in "${explicit_targets[@]}"; do
    targets+=("$file")
  done
else
  if [[ -d "$ROOT/docs" ]]; then
    while IFS= read -r -d '' file; do
      targets+=("$file")
    done < <(find "$ROOT/docs" -type f -name 'open-questions.md' -print0)
  fi
fi

if (( ${#targets[@]} == 0 )); then
  warn "no open-questions.md target found"
  exit 0
fi

if ! PYTHON_BIN="$(resolve_python_cmd)"; then
  fail "python3-compatible interpreter is required (python3 or python)"
  echo "[FAIL] open-questions schema check failed with $fail_count issue(s)" >&2
  exit 1
fi

for target in "${targets[@]}"; do
  if [[ ! -f "$target" ]]; then
    fail "file not found: $target"
    continue
  fi

  result="$(validate_file "$target" 2>/dev/null || true)"
  rel="${target#$ROOT/}"

  case "$result" in
    closed)
      ok "$rel: closed"
      ;;
    open-valid)
      if (( REQUIRE_CLOSED == 1 )); then
        fail "$rel: unresolved question cards found (require-closed mode)"
      else
        ok "$rel: open question cards are schema-valid"
      fi
      ;;
    invalid:*)
      fields="${result#invalid:}"
      fail "$rel: missing required fields for open question card ($fields)"
      ;;
    *)
      fail "$rel: schema validation failed"
      ;;
  esac
done

if (( fail_count > 0 )); then
  echo "[FAIL] open-questions schema check failed with $fail_count issue(s)" >&2
  exit 1
fi

if (( warn_count > 0 )); then
  echo "[WARN] open-questions schema check passed with $warn_count warning(s)"
else
  ok "open-questions schema check passed"
fi
