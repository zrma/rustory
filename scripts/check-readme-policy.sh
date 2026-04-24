#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
README_FILE="$ROOT/README.md"
MAX_LINES="${README_POLICY_MAX_LINES:-220}"
MAX_H2="${README_POLICY_MAX_H2:-8}"
MAX_H3="${README_POLICY_MAX_H3:-6}"
MAX_CODE_FENCES="${README_POLICY_MAX_CODE_FENCES:-6}"
fail_count=0

usage() {
  cat <<'USAGE'
Check README.md stays aligned with docs/README_OPERATING_POLICY.md.

Usage:
  scripts/check-readme-policy.sh [options]

Options:
  --max-lines <n>         maximum allowed total lines (default: 220)
  --max-h2 <n>            maximum allowed H2 headings (default: 8)
  --max-h3 <n>            maximum allowed H3 headings (default: 6)
  --max-code-fences <n>   maximum allowed ``` fences (default: 6)
  -h, --help              show help

Env:
  README_POLICY_MAX_LINES
  README_POLICY_MAX_H2
  README_POLICY_MAX_H3
  README_POLICY_MAX_CODE_FENCES
USAGE
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

ok() {
  echo "[ OK ] $*"
}

fail() {
  echo "[FAIL] $*" >&2
  fail_count=$((fail_count + 1))
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --max-lines)
      MAX_LINES="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --max-h2)
      MAX_H2="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --max-h3)
      MAX_H3="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --max-code-fences)
      MAX_CODE_FENCES="$(parse_opt_value "$1" "${2:-}")"
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

for n in "$MAX_LINES" "$MAX_H2" "$MAX_H3" "$MAX_CODE_FENCES"; do
  if ! [[ "$n" =~ ^[0-9]+$ ]]; then
    echo "[FAIL] numeric options must be non-negative integers" >&2
    exit 1
  fi
done

if [[ ! -f "$README_FILE" ]]; then
  fail "missing README.md"
  echo "[FAIL] README policy check failed with $fail_count issue(s)" >&2
  exit 1
fi

first_line="$(sed -n '1p' "$README_FILE")"
if [[ ! "$first_line" =~ ^#\  ]]; then
  fail "README.md first line must be a level-1 title (# ...)"
fi

line_count="$(wc -l < "$README_FILE" | tr -d '[:space:]')"
h2_count="$(grep -c '^## ' "$README_FILE" || true)"
h3_count="$(grep -c '^### ' "$README_FILE" || true)"
code_fence_count="$(grep -c '^```' "$README_FILE" || true)"

if (( line_count > MAX_LINES )); then
  fail "README.md is too long: ${line_count} lines (max: ${MAX_LINES})"
fi
if (( h2_count > MAX_H2 )); then
  fail "README.md has too many H2 headings: ${h2_count} (max: ${MAX_H2})"
fi
if (( h3_count > MAX_H3 )); then
  fail "README.md has too many H3 headings: ${h3_count} (max: ${MAX_H3})"
fi
if (( code_fence_count > MAX_CODE_FENCES )); then
  fail "README.md has too many code fences: ${code_fence_count} (max: ${MAX_CODE_FENCES})"
fi

required_h2=(
  "## Quick Start"
  "## Agent Navigation"
  "## Product Docs"
  "## Development"
)

for heading in "${required_h2[@]}"; do
  if ! grep -Fqx "$heading" "$README_FILE"; then
    fail "missing required heading: $heading"
  fi
done

required_links=(
  "docs/HANDOFF.md"
  "docs/README_OPERATING_POLICY.md"
  "docs/OPERATING_MODEL.md"
  "docs/EXECUTION_LOOP.md"
  "docs/CHANGE_CONTROL.md"
  "docs/IMPROVEMENT_LOOP.md"
  "docs/ESCALATION_POLICY.md"
  "docs/LESSONS_LOG.md"
  "docs/REPO_MANIFEST.yaml"
  "AGENTS.md"
)

for link in "${required_links[@]}"; do
  if ! grep -Fq "$link" "$README_FILE"; then
    fail "missing required navigation link: $link"
  fi
done

if (( fail_count > 0 )); then
  echo "[FAIL] README policy check failed with $fail_count issue(s)" >&2
  exit 1
fi

ok "README policy check passed"
