#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fail_count=0
warn_count=0

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
Preflight guard for production-impact operations (deploy/prod smoke).

Usage:
  scripts/check-prod-preflight.sh

Checks:
  1) temp branch hygiene (root + submodules)
  2) submodule detached/dirty safety gate
  3) strict submodule integrity check

This guard fails fast when a submodule has uncommitted changes, especially
when changes exist on detached HEAD.
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if "$ROOT/scripts/check-branch-hygiene.sh"; then
  ok "temp branch hygiene passed"
else
  fail "temp branch hygiene failed"
fi

if [[ ! -f "$ROOT/.gitmodules" ]]; then
  warn ".gitmodules not found. skip submodule-specific preflight checks."
fi

submodule_count=0
if [[ -f "$ROOT/.gitmodules" ]]; then
  while IFS= read -r path; do
    [[ -z "$path" ]] && continue
    submodule_count=$((submodule_count + 1))
    abs_path="$ROOT/$path"

    if [[ ! -d "$abs_path/.git" && ! -f "$abs_path/.git" ]]; then
      fail "$path: submodule not initialized"
      continue
    fi

    branch="$(git -C "$abs_path" rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
    status_porcelain="$(git -C "$abs_path" status --porcelain || true)"

    if [[ "$branch" == "HEAD" ]]; then
      if [[ -n "$status_porcelain" ]]; then
        fail "$path: detached HEAD with uncommitted changes (commit/switch before prod operation)"
      else
        warn "$path: detached HEAD (clean). This is common for submodules, but avoid editing before switching to a branch."
      fi
      continue
    fi

    if [[ -n "$status_porcelain" ]]; then
      fail "$path: uncommitted changes on branch '$branch'"
      continue
    fi

    ok "$path: clean working tree on branch '$branch'"
  done < <(git -C "$ROOT" config -f .gitmodules --get-regexp '^submodule\..*\.path$' | awk '{print $2}')
fi

if (( submodule_count == 0 )); then
  ok "no configured submodules. skip strict submodule integrity check"
elif (( fail_count == 0 )); then
  if "$ROOT/scripts/check-submodules.sh" --strict; then
    ok "strict submodule integrity passed"
  else
    fail "strict submodule integrity failed"
  fi
else
  warn "skip strict submodule integrity due to earlier failures"
fi

if (( fail_count > 0 )); then
  echo "[FAIL] preflight failed with $fail_count issue(s)" >&2
  exit 1
fi

if (( warn_count > 0 )); then
  echo "[WARN] preflight passed with $warn_count warning(s)"
else
  ok "preflight passed"
fi
