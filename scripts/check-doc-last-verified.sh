#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MAX_AGE_DAYS="${DOC_LAST_VERIFIED_MAX_DAYS:-30}"

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

usage() {
  cat <<'USAGE'
Check Last Verified freshness for core operations documents.

Usage:
  scripts/check-doc-last-verified.sh [--max-age-days <days>]

Options:
  --max-age-days <days>  allowed age in days (default: 30)
  -h, --help             show help

Env:
  DOC_LAST_VERIFIED_MAX_DAYS  same as --max-age-days
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --max-age-days)
      MAX_AGE_DAYS="${2:-}"
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

if ! [[ "$MAX_AGE_DAYS" =~ ^[0-9]+$ ]]; then
  echo "[FAIL] invalid --max-age-days: $MAX_AGE_DAYS (expected non-negative integer)" >&2
  exit 1
fi

if ! PYTHON_BIN="$(resolve_python_cmd)"; then
  echo "[FAIL] python3-compatible interpreter is required (python3 or python)" >&2
  exit 1
fi

TARGET_FILES=(
  "docs/HANDOFF.md"
  "docs/EXECUTION_LOOP.md"
  "docs/CHANGE_CONTROL.md"
  "docs/OPERATING_MODEL.md"
  "docs/IMPROVEMENT_LOOP.md"
  "docs/ESCALATION_POLICY.md"
  "docs/LESSONS_LOG.md"
  "docs/README_OPERATING_POLICY.md"
  "docs/SKILL_OPERATING_GUIDE.md"
)

"$PYTHON_BIN" - "$ROOT" "$MAX_AGE_DAYS" "${TARGET_FILES[@]}" <<'PY'
import re
import subprocess
import sys
from datetime import date
from pathlib import Path

root = Path(sys.argv[1])
max_age_days = int(sys.argv[2])
files = sys.argv[3:]
today = date.today()
failed = False

def git_stdout(repo_root: Path, args):
    result = subprocess.run(
        ["git", "-C", str(repo_root), *args],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return ""
    return result.stdout.strip()

def git_last_change_date(repo_root: Path, rel_path: str):
    raw = git_stdout(repo_root, ["log", "-1", "--format=%cs", "--", rel_path])
    if not raw:
        return None
    try:
        return date.fromisoformat(raw)
    except ValueError:
        return None

def has_worktree_change(repo_root: Path, rel_path: str):
    raw = git_stdout(repo_root, ["status", "--short", "--", rel_path])
    return bool(raw)

for rel in files:
    path = root / rel
    if not path.exists():
        print(f"[FAIL] missing file: {rel}")
        failed = True
        continue

    text = path.read_text(encoding="utf-8")
    match = re.search(r"(?m)^- Last Verified:\s*([0-9]{4}-[0-9]{2}-[0-9]{2})\s*$", text)
    if not match:
        print(f"[FAIL] Last Verified header missing or malformed: {rel}")
        failed = True
        continue

    raw = match.group(1)
    verified_date = date.fromisoformat(raw)
    if verified_date > today:
        print(
            f"[FAIL] Last Verified is in the future: {rel} "
            f"(value={verified_date.isoformat()}, today={today.isoformat()})"
        )
        failed = True
        continue

    age_days = (today - verified_date).days
    if age_days > max_age_days:
        print(
            f"[FAIL] Last Verified is stale: {rel} "
            f"(age={age_days}d, max={max_age_days}d, value={verified_date.isoformat()})"
        )
        failed = True
        continue

    latest_change_date = git_last_change_date(root, rel)
    latest_change_source = "git-history"
    if has_worktree_change(root, rel):
        if latest_change_date is None or today > latest_change_date:
            latest_change_date = today
        latest_change_source = "worktree-change"

    if latest_change_date is not None and latest_change_date > verified_date:
        print(
            f"[FAIL] Last Verified is older than latest document change: {rel} "
            f"(last_verified={verified_date.isoformat()}, latest_change={latest_change_date.isoformat()}, source={latest_change_source})"
        )
        failed = True
        continue

    print(f"[ OK ] {rel}: Last Verified {verified_date.isoformat()} (age={age_days}d)")

if failed:
    raise SystemExit(1)

print(f"[ OK ] doc Last Verified check passed (max-age={max_age_days}d)")
PY
