#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

fail() {
  printf 'agent harness interface check failed: %s\n' "$*" >&2
  exit 1
}

for required_file in \
  .ai-first.toml \
  .ai-first.lock \
  .ai-first/check.py \
  AGENTS.md \
  docs/agent-harness.md \
  docs/HANDOFF.md \
  docs/REPO_MANIFEST.yaml \
  scripts/check-publication-boundary.py; do
  [ -s "$required_file" ] || fail "missing or empty $required_file"
done

# 공통 generated interface와 선언/lock 정합성은 pinned standalone checker에 위임한다.

grep -Fq -- 'local-first write, shared grid identity' AGENTS.md ||
  fail "Rustory identity boundary is missing"
grep -Fq -- 'background daemon spawn' AGENTS.md ||
  fail "Rustory daemon lifecycle contract is missing"
grep -Fq -- '"source_kind": "release"' .ai-first.lock ||
  fail "framework release source is missing from lock"

python3 .ai-first/check.py

publication_target=${TODO_UNPUBLISHED_TARGET_REV:-}
publication_base=${TODO_UNPUBLISHED_BASE_REF:-}
if [ -n "$publication_target" ]; then
  if [ -n "$publication_base" ] &&
    git rev-parse --verify "$publication_base^{commit}" >/dev/null 2>&1; then
    scripts/check-publication-boundary.py \
      --target-rev "$publication_target" \
      --base-rev "$publication_base"
  else
    scripts/check-publication-boundary.py --target-rev "$publication_target"
  fi
else
  scripts/check-publication-boundary.py
fi

printf 'agent harness interface is valid: ai-first-harness-v1 / Rustory overlay\n'
