#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STRICT=0

if [[ "${1:-}" == "--strict" ]]; then
  STRICT=1
fi

if [[ ! -f "$ROOT/.gitmodules" ]]; then
  echo "[ OK ] no .gitmodules found. skip submodule checks."
  exit 0
fi

fail_count=0
warn_count=0

issue() {
  local level="$1"
  local msg="$2"
  if [[ "$level" == "fail" ]]; then
    if (( STRICT == 1 )); then
      echo "[FAIL] $msg" >&2
      fail_count=$((fail_count + 1))
      return
    fi
    echo "[WARN] $msg"
    warn_count=$((warn_count + 1))
    return
  fi
  echo "[WARN] $msg"
  warn_count=$((warn_count + 1))
}

ok() {
  echo "[ OK ] $*"
}

info() {
  echo "[INFO] $*"
}

fetch_remote_non_interactive() {
  local repo_path="$1"
  local remote="$2"

  # 무인 실행에서 인증 프롬프트로 멈추지 않도록 대화형 입력을 강제 차단한다.
  GIT_TERMINAL_PROMPT=0 GIT_ASKPASS= git -C "$repo_path" fetch --quiet "$remote"
}

normalize_url() {
  local raw="$1"
  local scheme_removed=""
  local host_path=""

  # normalize transport variants:
  # - https://user:token@github.com/org/repo(.git)
  # - ssh://git@github.com/org/repo(.git)
  # - git@github.com:org/repo(.git)
  # into a comparable "host/path" form.
  if [[ "$raw" =~ ^[a-zA-Z][a-zA-Z0-9+.-]*:// ]]; then
    scheme_removed="${raw#*://}"
    host_path="$scheme_removed"
    if [[ "$scheme_removed" == *@* ]]; then
      host_path="${scheme_removed#*@}"
    fi
  elif [[ "$raw" == *@*:* ]]; then
    # SCP-like syntax (git@github.com:org/repo)
    host_path="${raw#*@}"
    host_path="${host_path/:/\/}"
  else
    host_path="$raw"
  fi

  host_path="${host_path%/}"
  host_path="${host_path%.git}"
  echo "$host_path"
}

matching_remotes_for_url() {
  local repo_path="$1"
  local expected_url="$2"
  local expected_norm
  local remote remote_url remote_norm

  expected_norm="$(normalize_url "$expected_url")"

  while read -r remote; do
    [[ -z "$remote" ]] && continue
    remote_url="$(git -C "$repo_path" remote get-url "$remote" 2>/dev/null || true)"
    [[ -z "$remote_url" ]] && continue
    remote_norm="$(normalize_url "$remote_url")"
    if [[ "$remote_norm" == "$expected_norm" ]]; then
      echo "$remote"
    fi
  done < <(git -C "$repo_path" remote)
}

submodule_names=()
while IFS= read -r name; do
  [[ -z "$name" ]] && continue
  submodule_names+=("$name")
done < <(
  git config -f "$ROOT/.gitmodules" --name-only --get-regexp '^submodule\..*\.path$' \
  | sed -E 's/^submodule\.([^.]+)\.path$/\1/'
)

if [[ "${#submodule_names[@]}" -eq 0 ]]; then
  ok "no submodule entries in .gitmodules. skip submodule checks."
  exit 0
fi

for name in "${submodule_names[@]}"; do
  path="$(git config -f "$ROOT/.gitmodules" "submodule.${name}.path")"
  url="$(git config -f "$ROOT/.gitmodules" "submodule.${name}.url")"
  abs_path="$ROOT/$path"

  if [[ ! -d "$abs_path/.git" && ! -f "$abs_path/.git" ]]; then
    issue fail "$path: submodule not initialized"
    continue
  fi

  pointer_sha="$(git -C "$ROOT" ls-tree HEAD "$path" | awk '{print $3}')"
  if [[ -z "$pointer_sha" ]]; then
    issue fail "$path: pointer SHA missing from root HEAD"
    continue
  fi

  head_sha="$(git -C "$abs_path" rev-parse HEAD 2>/dev/null || true)"
  if [[ -z "$head_sha" ]]; then
    issue fail "$path: cannot resolve submodule HEAD"
    continue
  fi

  if [[ "$pointer_sha" != "$head_sha" ]]; then
    issue fail "$path: pointer SHA ($pointer_sha) != submodule HEAD ($head_sha)"
  else
    ok "$path: pointer matches submodule HEAD ($head_sha)"
  fi

  if [[ -n "$(git -C "$abs_path" status --porcelain)" ]]; then
    issue fail "$path: dirty working tree"
  fi

  if ! git -C "$abs_path" cat-file -e "$pointer_sha^{commit}" 2>/dev/null; then
    issue fail "$path: pointer SHA not found locally ($pointer_sha)"
    continue
  fi

  if (( STRICT == 1 )); then
    remotes_for_check=()
    while IFS= read -r remote; do
      [[ -z "$remote" ]] && continue
      remotes_for_check+=("$remote")
    done < <(matching_remotes_for_url "$abs_path" "$url")
    if [[ "${#remotes_for_check[@]}" -eq 0 ]]; then
      issue fail "$path: no local remote matches .gitmodules URL ($url)"
      continue
    fi

    reachable=0
    fetch_ok=0
    for remote in "${remotes_for_check[@]}"; do
      if ! fetch_remote_non_interactive "$abs_path" "$remote"; then
        issue fail "$path: failed to fetch remote '$remote' (non-interactive auth/network failure)"
        continue
      fi
      fetch_ok=1
      if [[ -z "$(git -C "$abs_path" rev-list -n 1 "$pointer_sha" --not --remotes="$remote")" ]]; then
        ok "$path: pointer SHA reachable from $remote refs"
        reachable=1
        break
      fi
    done

    if (( fetch_ok == 0 )); then
      continue
    fi
    if (( reachable == 0 )); then
      issue fail "$path: pointer SHA is not reachable from matched remote refs ($pointer_sha)"
    fi
  else
    info "$path: skip remote reachability check (run with --strict)"
  fi

  if [[ -n "$url" ]]; then
    ok "$path: remote=$url"
  fi
done

if (( fail_count > 0 )); then
  echo "[FAIL] submodule check failed: $fail_count issue(s)" >&2
  exit 1
fi

if (( warn_count > 0 )); then
  echo "[WARN] submodule check completed with $warn_count warning(s)"
  if (( STRICT == 0 )); then
    echo "[INFO] run with --strict to fail on warnings"
  fi
else
  ok "submodule check passed"
fi
