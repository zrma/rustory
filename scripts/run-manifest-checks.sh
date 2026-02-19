#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/todo-workspace.sh
source "$ROOT/scripts/lib/todo-workspace.sh"
MANIFEST="$ROOT/docs/REPO_MANIFEST.yaml"
MODE="quick"
WORK_ID=""
WORK_ID_PATTERN='^[a-z0-9]+(-[a-z0-9]+)*$'
REPO_KEY="rustory"
DRY_RUN=0
ALLOW_MISSING_WORK_ID=0
fail_count=0
run_count=0
skip_count=0
declare -a WORK_IDS=()

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
Run local checks declared in docs/REPO_MANIFEST.yaml.

Usage:
  scripts/run-manifest-checks.sh [options]

Options:
  --mode <quick|full>   quick: run scripts/check-* only (default)
                        full: run all local checks except ci:* entries
  --work-id <id>        replace <work-id> placeholder in checks
                        (full mode fails when <work-id> checks cannot be resolved)
  --allow-missing-work-id
                        allow full mode to skip unresolved <work-id> checks
  --repo-key <key>      repository key in REPO_MANIFEST (default: rustory)
  --dry-run             print commands without executing
  -h, --help            show help
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

validate_work_id() {
  local work_id="$1"
  if [[ ! "$work_id" =~ $WORK_ID_PATTERN ]]; then
    fail "invalid work-id: $work_id (expected lowercase kebab-case, e.g. llm-agent-stability-hardening)"
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
    --repo-key)
      REPO_KEY="$(parse_opt_value "$1" "${2:-}")"
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

if [[ "$MODE" != "quick" && "$MODE" != "full" ]]; then
  echo "invalid --mode: $MODE (expected quick|full)" >&2
  exit 1
fi

if ! todo_workspace_load_config "$ROOT" "$MANIFEST"; then
  echo "failed to load todo workspace config from $MANIFEST" >&2
  exit 1
fi

if [[ ! -f "$MANIFEST" ]]; then
  echo "manifest not found: $MANIFEST" >&2
  exit 1
fi

if ! PYTHON_BIN="$(resolve_python_cmd)"; then
  echo "python3-compatible interpreter is required (python3 or python)" >&2
  exit 1
fi

if ! "$PYTHON_BIN" - <<'PY' >/dev/null 2>&1
import yaml
PY
then
  echo "$PYTHON_BIN package 'yaml' is required to parse $MANIFEST" >&2
  exit 1
fi

CHECKS_RAW="$(
  "$PYTHON_BIN" - "$MANIFEST" "$REPO_KEY" <<'PY'
import base64
import sys
import yaml

manifest_path, repo_key = sys.argv[1], sys.argv[2]
with open(manifest_path, "r", encoding="utf-8") as f:
    data = yaml.safe_load(f) or {}

repos = data.get("repositories") or []
repo = next((r for r in repos if r.get("key") == repo_key), None)
if repo is None:
    raise SystemExit(f"repo key not found in manifest: {repo_key}")

checks = repo.get("checks") or []
for check in checks:
    if isinstance(check, str):
        encoded = base64.b64encode(check.encode("utf-8")).decode("ascii")
        print(f"repo\t{encoded}")

for submodule in data.get("submodules") or []:
    sub_key = submodule.get("key") or submodule.get("path") or "submodule"
    for check in submodule.get("checks") or []:
        if isinstance(check, str):
            encoded = base64.b64encode(check.encode("utf-8")).decode("ascii")
            print(f"submodule:{sub_key}\t{encoded}")
PY
)" || {
  echo "failed to parse checks from $MANIFEST for repo key: $REPO_KEY" >&2
  exit 1
}

CHECKS=()
if [[ -n "$CHECKS_RAW" ]]; then
  readarray -t CHECKS <<<"$CHECKS_RAW"
fi

if [[ "${#CHECKS[@]}" -eq 0 ]]; then
  warn "no checks found in manifest for repo key: $REPO_KEY"
  exit 0
fi

execute_check() {
  local source="$1"
  local cmd="$2"

  if [[ -z "$cmd" ]]; then
    fail "empty check command from $source"
    return 1
  fi

  if (( DRY_RUN == 1 )); then
    echo "[DRY] ($source) $cmd"
    return 0
  fi

  if (cd "$ROOT" && eval "$cmd"); then
    ok "check passed ($source): $cmd"
    return 0
  fi

  fail "check failed ($source): $cmd"
  return 1
}

decode_check_cmd() {
  local encoded="$1"
  "$PYTHON_BIN" - "$encoded" <<'PY'
import base64
import binascii
import sys

if len(sys.argv) != 2:
    raise SystemExit(2)

try:
    decoded = base64.b64decode(sys.argv[1], validate=True).decode("utf-8")
except (ValueError, binascii.Error, UnicodeDecodeError):
    raise SystemExit(2)

print(decoded, end="")
PY
}

if [[ -n "$WORK_ID" ]]; then
  if validate_work_id "$WORK_ID"; then
    todo_rel="$(todo_workspace_rel_for_work_id "$WORK_ID")"
    if [[ -d "$ROOT/$todo_rel" ]]; then
      WORK_IDS+=("$WORK_ID")
    else
      warn "skip <work-id> checks for missing todo workspace: $todo_rel"
    fi
  fi
else
  while IFS= read -r todo_dir; do
    [[ -z "$todo_dir" ]] && continue
    if ! resolved_work_id="$(todo_workspace_extract_work_id "$todo_dir")"; then
      warn "skip todo workspace with unsupported name: ${todo_dir#$ROOT/} (expected prefix: $TODO_WORKSPACE_NAME_PREFIX)"
      continue
    fi
    if validate_work_id "$resolved_work_id"; then
      WORK_IDS+=("$resolved_work_id")
    fi
  done < <(todo_workspace_find_dirs "$ROOT")
fi

if (( fail_count > 0 )); then
  echo "[FAIL] manifest checks failed: run=$run_count, skip=$skip_count, fail=$fail_count" >&2
  exit 1
fi

if [[ "$MODE" == "full" && "${#WORK_IDS[@]}" -eq 0 && "$ALLOW_MISSING_WORK_ID" -ne 1 ]]; then
  for raw in "${CHECKS[@]}"; do
    source="${raw%%$'\t'*}"
    if [[ "$source" == "$raw" ]]; then
      fail "malformed check entry in manifest output: $raw"
      continue
    fi
    cmd_payload="${raw#*$'\t'}"
    if ! cmd="$(decode_check_cmd "$cmd_payload")"; then
      fail "failed to decode manifest check command payload: $cmd_payload"
      continue
    fi

    if [[ "$cmd" == ci:* ]]; then
      continue
    fi

    if [[ "$cmd" == *"<work-id>"* ]]; then
      fail "missing work-id context for manifest check with <work-id>: $cmd (pass --work-id or create '$TODO_WORKSPACE_GLOB')"
      break
    fi
  done
fi

if (( fail_count > 0 )); then
  echo "[FAIL] manifest checks failed: run=$run_count, skip=$skip_count, fail=$fail_count" >&2
  exit 1
fi

for raw in "${CHECKS[@]}"; do
  source="${raw%%$'\t'*}"
  if [[ "$source" == "$raw" ]]; then
    fail "malformed check entry in manifest output: $raw"
    continue
  fi
  cmd_payload="${raw#*$'\t'}"
  if ! cmd="$(decode_check_cmd "$cmd_payload")"; then
    fail "failed to decode manifest check command payload: $cmd_payload"
    continue
  fi

  if [[ "$cmd" == ci:* ]]; then
    warn "skip CI-only check: $cmd"
    skip_count=$((skip_count + 1))
    continue
  fi

  if [[ "$cmd" == *"<work-id>"* ]]; then
    if [[ "${#WORK_IDS[@]}" -eq 0 ]]; then
      if [[ "$MODE" == "full" && "$ALLOW_MISSING_WORK_ID" -ne 1 ]]; then
        fail "missing work-id context for manifest check with <work-id>: $cmd (pass --work-id or create '$TODO_WORKSPACE_GLOB')"
      else
        warn "skip check without todo workspace for <work-id> placeholder: $cmd"
        skip_count=$((skip_count + 1))
      fi
      continue
    fi
    for wid in "${WORK_IDS[@]}"; do
      resolved_cmd="${cmd//<work-id>/$wid}"
      run_count=$((run_count + 1))
      if ! execute_check "$source" "$resolved_cmd"; then
        true
      fi
    done
    continue
  fi

  if [[ "$cmd" == *"<repo-key>"* ]]; then
    if [[ -z "$REPO_KEY" ]]; then
      fail "missing --repo-key for check: $cmd"
      continue
    fi
    cmd="${cmd//<repo-key>/$REPO_KEY}"
  fi

  if [[ "$MODE" == "quick" && "$cmd" != *scripts/check-* ]]; then
    warn "skip non-gate check in quick mode: $cmd"
    skip_count=$((skip_count + 1))
    continue
  fi

  if [[ "$cmd" == "scripts/check-jj-root-git-commit.sh" ]]; then
    warn "skip hook-only guard in standalone run: $cmd"
    skip_count=$((skip_count + 1))
    continue
  fi

  if [[ "$cmd" =~ \<[a-zA-Z0-9._-]+\> ]]; then
    fail "check has unresolved placeholder in manifest command ($source): $cmd"
    continue
  fi

  run_count=$((run_count + 1))
  if ! execute_check "$source" "$cmd"; then
    true
  fi
done

if (( fail_count > 0 )); then
  echo "[FAIL] manifest checks failed: run=$run_count, skip=$skip_count, fail=$fail_count" >&2
  exit 1
fi

ok "manifest checks passed: run=$run_count, skip=$skip_count"
