#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/todo-workspace.sh
source "$ROOT/scripts/lib/todo-workspace.sh"
DOCS_DIR="$ROOT/docs"
INDEX_FILE="$DOCS_DIR/README.md"
fail_count=0
declare -a TODO_WORKSPACE_DIRS=()

usage() {
  cat <<'USAGE'
Check docs index coverage for markdown files.

Usage:
  scripts/check-doc-index.sh

Rules:
  - top-level docs/*.md (except docs/README.md) must be listed in docs/README.md
  - nested docs/**/README.md must be listed in docs/README.md
  - nested docs/**/*.md (except docs/**/README.md) must be listed in the same directory README.md
  - docs todo workspaces (maintenance.todo_workspace_glob) are excluded from index checks
USAGE
}

ok() {
  echo "[ OK ] $*"
}

fail() {
  echo "[FAIL] $*" >&2
  fail_count=$((fail_count + 1))
}

escape_regex() {
  printf '%s' "$1" | sed -E 's/[][(){}.^$*+?|\\/]/\\&/g'
}

link_target_exists() {
  local index_file="$1"
  shift

  local target=""
  local escaped_target=""
  for target in "$@"; do
    [[ -z "$target" ]] && continue
    escaped_target="$(escape_regex "$target")"
    if grep -Eq "\\[[^][]+\\]\\(${escaped_target}([#?][^)]*)?\\)" "$index_file"; then
      return 0
    fi
    if grep -Eq "^[[:space:]]{0,3}\\[[^][]+\\]:[[:space:]]*<?${escaped_target}>?([[:space:]]+.*)?$" "$index_file"; then
      return 0
    fi
  done

  return 1
}

is_todo_workspace_doc() {
  local rel_path="$1"
  local todo_dir=""
  local todo_rel=""
  for todo_dir in "${TODO_WORKSPACE_DIRS[@]}"; do
    todo_rel="${todo_dir#$ROOT/}"
    if [[ "$rel_path" == "$todo_rel/"* ]]; then
      return 0
    fi
  done
  return 1
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if [[ $# -gt 0 ]]; then
  echo "unknown option: $1" >&2
  usage >&2
  exit 1
fi

if [[ ! -f "$INDEX_FILE" ]]; then
  fail "missing docs index: ${INDEX_FILE#$ROOT/}"
  echo "[FAIL] doc index check failed with $fail_count issue(s)" >&2
  exit 1
fi

if ! todo_workspace_load_config "$ROOT"; then
  fail "failed to load todo workspace config from docs/REPO_MANIFEST.yaml"
  echo "[FAIL] doc index check failed with $fail_count issue(s)" >&2
  exit 1
fi

readarray -t TODO_WORKSPACE_DIRS < <(todo_workspace_find_dirs "$ROOT")

readarray -t top_docs < <(find "$DOCS_DIR" -maxdepth 1 -type f -name '*.md' | sort)

for doc_file in "${top_docs[@]}"; do
  doc_name="$(basename "$doc_file")"
  if [[ "$doc_name" == "README.md" ]]; then
    continue
  fi

  if link_target_exists "$INDEX_FILE" "$doc_name" "docs/$doc_name"; then
    ok "indexed: docs/$doc_name"
    continue
  fi

  fail "missing docs/README.md entry for docs/$doc_name"
done

readarray -t nested_docs < <(find "$DOCS_DIR" -mindepth 2 -type f -name '*.md' | sort)

for doc_file in "${nested_docs[@]}"; do
  rel_path="${doc_file#$ROOT/}"
  if is_todo_workspace_doc "$rel_path"; then
    continue
  fi

  doc_name="$(basename "$doc_file")"
  path_without_docs="${rel_path#docs/}"

  if [[ "$doc_name" == "README.md" ]]; then
    if link_target_exists "$INDEX_FILE" "$path_without_docs" "$rel_path"; then
      ok "indexed: $rel_path"
      continue
    fi
    fail "missing docs/README.md entry for $rel_path"
    continue
  fi

  doc_dir="$(dirname "$doc_file")"
  doc_dir_rel="${doc_dir#$ROOT/}"
  doc_dir_index="$doc_dir/README.md"

  if [[ -f "$doc_dir_index" ]]; then
    doc_dir_index_rel="${doc_dir_index#$ROOT/}"
    if link_target_exists "$doc_dir_index" "$doc_name" "$path_without_docs" "$rel_path"; then
      ok "indexed: $rel_path (via $doc_dir_index_rel)"
      continue
    fi
    fail "missing $doc_dir_index_rel entry for $rel_path"
    continue
  fi

  fail "missing directory index: $doc_dir_rel/README.md (required for nested doc $rel_path)"
done

if (( fail_count > 0 )); then
  echo "[FAIL] doc index check failed with $fail_count issue(s)" >&2
  exit 1
fi

ok "doc index check passed"
