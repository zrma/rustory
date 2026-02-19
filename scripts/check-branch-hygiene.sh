#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fail_count=0

ok() {
  echo "[ OK ] $*"
}

fail() {
  echo "[FAIL] $*" >&2
  fail_count=$((fail_count + 1))
}

check_temp_branches() {
  local repo_path="$1"
  local label="$2"
  local branches=""

  branches="$(git -C "$repo_path" branch --format '%(refname:short)' --list 'rewrite/*' 'backup/*' || true)"
  if [[ -n "$branches" ]]; then
    fail "$label: temporary branches remain"
    while IFS= read -r branch; do
      [[ -z "$branch" ]] && continue
      echo "       - $branch"
    done <<< "$branches"
    return
  fi

  ok "$label: no temporary branches"
}

check_jj_orphan_heads() {
  local repo_path="$1"
  local label="$2"
  local heads=""

  if [[ ! -d "$repo_path/.jj" ]]; then
    return
  fi

  heads="$(
    cd "$repo_path"
    jj log -r 'heads(all()) & ~bookmarks() & ~empty() & ~@' --no-graph \
      -T 'change_id.short() ++ " " ++ commit_id.short() ++ " " ++ description.first_line() ++ "\n"' || true
  )"

  if [[ -n "$heads" ]]; then
    fail "$label: unbookmarked non-empty jj head(s) detected"
    echo "       hint: run 'jj log -r \"heads(all()) & ~bookmarks() & ~empty()\"' to inspect"
    echo "       hint: bookmark the intended head or run 'jj abandon <change-id>'"
    while IFS= read -r head; do
      [[ -z "$head" ]] && continue
      echo "       - $head"
    done <<< "$heads"
    return
  fi

  ok "$label: no unbookmarked non-empty jj heads"
}

check_temp_branches "$ROOT" "root"
check_jj_orphan_heads "$ROOT" "root"

if [[ -f "$ROOT/.gitmodules" ]]; then
  while IFS= read -r path; do
    [[ -z "$path" ]] && continue
    abs_path="$ROOT/$path"
    if [[ ! -d "$abs_path/.git" && ! -f "$abs_path/.git" ]]; then
      fail "$path: submodule not initialized"
      continue
    fi
    check_temp_branches "$abs_path" "$path"
    check_jj_orphan_heads "$abs_path" "$path"
  done < <(git -C "$ROOT" config -f .gitmodules --get-regexp '^submodule\..*\.path$' | awk '{print $2}')
fi

if (( fail_count > 0 )); then
  fail "branch hygiene check failed with $fail_count issue(s)"
  exit 1
fi

ok "branch hygiene check passed"
