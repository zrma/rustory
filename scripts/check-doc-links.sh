#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fail_count=0
SUBMODULE_PATHS=""

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

warn() {
  echo "[WARN] $*"
}

ok() {
  echo "[ OK ] $*"
}

fail() {
  echo "[FAIL] $*" >&2
  fail_count=$((fail_count + 1))
}

should_skip_link() {
  local link="$1"
  [[ -z "$link" ]] && return 0
  [[ "$link" == \#* ]] && return 0
  [[ "$link" == http://* ]] && return 0
  [[ "$link" == https://* ]] && return 0
  [[ "$link" == mailto:* ]] && return 0
  [[ "$link" == tel:* ]] && return 0
  [[ "$link" == data:* ]] && return 0
  [[ "$link" == javascript:* ]] && return 0
  [[ "$link" == \{\{* ]] && return 0
  [[ "$link" == \$\{* ]] && return 0
  return 1
}

normalize_link_target() {
  local link="$1"
  # remove enclosing angle brackets: <path/to/file.md>
  link="${link#<}"
  link="${link%>}"
  # drop anchor/query fragments.
  link="${link%%\#*}"
  link="${link%%\?*}"
  # drop optional markdown title suffix.
  link="${link%% \"*}"
  link="${link%% \'*}"
  echo "$link"
}

resolve_target() {
  local src_file="$1"
  local target="$2"
  if [[ "$target" == /* ]]; then
    echo "$ROOT/${target#/}"
    return
  fi
  echo "$ROOT/$(dirname "$src_file")/$target"
}

submodule_for_path() {
  local path="$1"
  local submodule_path
  while IFS= read -r submodule_path; do
    [[ -z "$submodule_path" ]] && continue
    if [[ "$path" == "$submodule_path" || "$path" == "$submodule_path/"* ]]; then
      printf '%s\n' "$submodule_path"
      return 0
    fi
  done <<< "$SUBMODULE_PATHS"
  return 1
}

collect_markdown_files() {
  local rel_file

  if git -C "$ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    git -C "$ROOT" ls-files -- '*.md' | sort -u
    return
  fi

  local file
  {
    for file in README.md AGENTS.md; do
      if [[ -f "$ROOT/$file" ]]; then
        printf '%s\n' "$file"
      fi
    done
    if [[ -d "$ROOT/docs" ]]; then
      while IFS= read -r -d '' file; do
        rel_file="${file#$ROOT/}"
        printf '%s\n' "$rel_file"
      done < <(find "$ROOT/docs" -type f -name '*.md' -print0)
    fi
  } | sort -u
}

if [[ -f "$ROOT/.gitmodules" ]]; then
  SUBMODULE_PATHS="$(
    git -C "$ROOT" config -f .gitmodules --get-regexp '^submodule\..*\.path$' \
      | awk '{print $2}' || true
  )"
fi

extract_links() {
  local file="$1"
  "$PYTHON_BIN" - "$file" <<'PY'
import re
import sys
from pathlib import Path

text = Path(sys.argv[1]).read_text(encoding="utf-8")

links = []
inline_re = re.compile(r'!?\[[^\[\]\n]+\]\(([^)\r\n]+)\)')
links.extend(match.group(1).strip() for match in inline_re.finditer(text))

def_re = re.compile(
    r'^[ \t]{0,3}\[([^\]\n]+)\]:[ \t]*<?([^>\s]+)>?'
    r'(?:[ \t]+(?:"[^"]*"|\'[^\']*\'|\([^\)]*\)))?[ \t]*$',
    re.MULTILINE,
)
definitions = {
    match.group(1).strip().lower(): match.group(2).strip()
    for match in def_re.finditer(text)
}

ref_re = re.compile(r'!?\[([^\]\n]+)\]\[([^\]\n]*)\]')
for match in ref_re.finditer(text):
    label = (match.group(2) or match.group(1)).strip().lower()
    target = definitions.get(label)
    if target:
        links.append(target)

shortcut_re = re.compile(r'(?<!!)\[([^\]\n]+)\](?![\(\[])')
for match in shortcut_re.finditer(text):
    label = match.group(1).strip().lower()
    target = definitions.get(label)
    if target:
        links.append(target)

seen = set()
for link in links:
    if link in seen:
        continue
    seen.add(link)
    print(link)
PY
}

if ! PYTHON_BIN="$(resolve_python_cmd)"; then
  fail "python3-compatible interpreter is required (python3 or python)"
  echo "[FAIL] markdown link check failed with $fail_count issue(s)" >&2
  exit 1
fi

found_markdown=0
while IFS= read -r file; do
  [[ -z "$file" ]] && continue
  found_markdown=1
done < <(collect_markdown_files)

if (( found_markdown == 0 )); then
  fail "no markdown files found for validation"
fi

while IFS= read -r file; do
  [[ -z "$file" ]] && continue
  abs_file="$ROOT/$file"
  if [[ ! -f "$abs_file" ]]; then
    warn "skip missing file in current checkout: $file"
    continue
  fi

  src_submodule="$(submodule_for_path "$file" || true)"

  while IFS= read -r raw_link; do
    link="$(normalize_link_target "$raw_link")"
    if should_skip_link "$link"; then
      continue
    fi

    resolved_path="$(resolve_target "$file" "$link")"
    if [[ -e "$resolved_path" ]]; then
      continue
    fi

    rel_path="${resolved_path#$ROOT/}"
    dst_submodule="$(submodule_for_path "$rel_path" || true)"
    if [[ -n "$dst_submodule" && "$src_submodule" != "$dst_submodule" ]]; then
      warn "$file -> $raw_link (submodule path outside current scope: $dst_submodule)"
      continue
    fi

    fail "$file -> $raw_link (resolved: ${resolved_path#$ROOT/})"
  done < <(extract_links "$abs_file")
done < <(collect_markdown_files)

if (( fail_count > 0 )); then
  fail "markdown link check failed with $fail_count issue(s)"
  exit 1
fi

ok "markdown link check passed"
