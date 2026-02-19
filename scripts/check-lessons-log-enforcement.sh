#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "$ROOT" ]]; then
  exit 0
fi

cd "$ROOT"

usage() {
  cat <<'USAGE'
Usage:
  scripts/check-lessons-log-enforcement.sh [--range <git-range>] [--worktree]

Options:
  --range <git-range>  validate changed files in git range (CI mode)
                       e.g. <base_sha>...<head_sha>
  --worktree           validate changed files from working tree + index
                       (jj/manual gate mode)
USAGE
}

RANGE=""
WORKTREE_MODE=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --range)
      RANGE="${2:-}"
      shift 2
      ;;
    --worktree)
      WORKTREE_MODE=1
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

extract_range_head_commit() {
  local range="$1"
  if [[ "$range" == *"..."* ]]; then
    printf '%s' "${range##*...}"
    return 0
  fi
  if [[ "$range" == *".."* ]]; then
    printf '%s' "${range##*..}"
    return 0
  fi
  return 1
}

resolve_diff_output_from_range() {
  local range="$1"
  local diff_output=""
  local head_commit=""

  if diff_output="$(git diff --name-only "$range" 2>/dev/null)"; then
    printf '%s' "$diff_output"
    return 0
  fi

  if head_commit="$(extract_range_head_commit "$range")" \
    && git rev-parse --verify "${head_commit}^{commit}" >/dev/null 2>&1 \
    && diff_output="$(git diff-tree --no-commit-id --name-only -r "$head_commit" 2>/dev/null)"; then
    echo "[WARN] invalid --range. fallback to commit diff: $head_commit" >&2
    printf '%s' "$diff_output"
    return 0
  fi

  if git rev-parse --verify HEAD~1 >/dev/null 2>&1 \
    && diff_output="$(git diff --name-only HEAD~1...HEAD 2>/dev/null)"; then
    echo "[WARN] invalid --range. fallback to HEAD~1...HEAD" >&2
    printf '%s' "$diff_output"
    return 0
  fi

  return 1
}

collect_worktree_files() {
  local -a unstaged_files=()
  local -a staged_files=()
  declare -A seen_files=()

  mapfile -t unstaged_files < <(git diff --name-only 2>/dev/null || true)
  mapfile -t staged_files < <(git diff --cached --name-only 2>/dev/null || true)

  for file in "${unstaged_files[@]}" "${staged_files[@]}"; do
    [[ -z "$file" ]] && continue
    seen_files["$file"]=1
  done

  for file in "${!seen_files[@]}"; do
    echo "$file"
  done
}

if [[ -n "$RANGE" && "$WORKTREE_MODE" -eq 1 ]]; then
  echo "[FAIL] --range and --worktree cannot be used together" >&2
  exit 1
fi

if [[ -n "$RANGE" ]]; then
  if ! diff_output="$(resolve_diff_output_from_range "$RANGE")"; then
    echo "[FAIL] invalid --range for lessons-log enforcement: $RANGE" >&2
    exit 1
  fi
  mapfile -t CHANGED_FILES <<<"$diff_output"
  if (( ${#CHANGED_FILES[@]} == 1 )) && [[ -z "${CHANGED_FILES[0]}" ]]; then
    CHANGED_FILES=()
  fi
elif [[ "$WORKTREE_MODE" -eq 1 ]]; then
  mapfile -t CHANGED_FILES < <(collect_worktree_files)
else
  mapfile -t CHANGED_FILES < <(git diff --cached --name-only)
fi

if (( ${#CHANGED_FILES[@]} == 0 )); then
  exit 0
fi

contains_lessons_log=0
for file in "${CHANGED_FILES[@]}"; do
  if [[ "$file" == "docs/LESSONS_LOG.md" ]]; then
    contains_lessons_log=1
    break
  fi
done

if (( contains_lessons_log == 0 )); then
  exit 0
fi

has_enforcement_change=0
for file in "${CHANGED_FILES[@]}"; do
  case "$file" in
    scripts/*|lefthook.yml|AGENTS.md|docs/EXECUTION_LOOP.md|docs/CHANGE_CONTROL.md|docs/IMPROVEMENT_LOOP.md|docs/OPERATING_MODEL.md|modules/*/AGENTS.md)
      has_enforcement_change=1
      break
      ;;
  esac
done

if (( has_enforcement_change == 1 )); then
  echo "[ OK ] lessons-log enforcement coupling passed"
  exit 0
fi

cat >&2 <<'EOF'
[FAIL] docs/LESSONS_LOG.md 변경은 실행 강제 장치 변경과 함께 커밋해야 합니다.
       (자동화/강제가 없는 교훈 기록은 금지)
       - 허용 예시: scripts/*, lefthook.yml, AGENTS.md,
                   docs/{EXECUTION_LOOP.md,CHANGE_CONTROL.md,IMPROVEMENT_LOOP.md,OPERATING_MODEL.md},
                   modules/*/AGENTS.md
EOF
echo "       changed files:" >&2
for file in "${CHANGED_FILES[@]}"; do
  echo "       - $file" >&2
done

exit 1
