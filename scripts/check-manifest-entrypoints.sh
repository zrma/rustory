#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT/docs/REPO_MANIFEST.yaml"

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
Validate path targets declared in docs/REPO_MANIFEST.yaml.

Usage:
  scripts/check-manifest-entrypoints.sh [options]

Options:
  --manifest <path>  manifest path relative to repo root (default: docs/REPO_MANIFEST.yaml)
  -h, --help         show help
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --manifest)
      if [[ -z "${2:-}" ]]; then
        echo "missing value for --manifest" >&2
        usage >&2
        exit 1
      fi
      MANIFEST="$ROOT/$2"
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

if [[ ! -f "$MANIFEST" ]]; then
  echo "[FAIL] manifest file not found: ${MANIFEST#$ROOT/}" >&2
  exit 1
fi

if ! PYTHON_BIN="$(resolve_python_cmd)"; then
  echo "[FAIL] python3-compatible interpreter is required (python3 or python)" >&2
  exit 1
fi

if ! "$PYTHON_BIN" - <<'PY' >/dev/null 2>&1
import yaml
PY
then
  echo "[FAIL] $PYTHON_BIN package 'yaml' is required to parse manifest" >&2
  exit 1
fi

"$PYTHON_BIN" - "$ROOT" "$MANIFEST" <<'PY'
import re
import sys
from pathlib import Path

import yaml

root = Path(sys.argv[1])
manifest_path = Path(sys.argv[2])

raw = manifest_path.read_text(encoding="utf-8")
manifest = yaml.safe_load(raw) or {}

failures: list[str] = []
targets: list[tuple[str, str, bool]] = []
repo_entrypoints: dict[str, set[str]] = {}

placeholder_re = re.compile(r"<[^>]+>")
wildcard_re = re.compile(r"[*?\[]")


def fail(msg: str) -> None:
    failures.append(msg)


def add_target(label: str, value: object, allow_glob: bool = False) -> None:
    if value is None:
        fail(f"{label}: missing")
        return
    if not isinstance(value, str):
        fail(f"{label}: expected string path, got {type(value).__name__}")
        return
    path = value.strip()
    if not path:
        fail(f"{label}: empty path")
        return
    targets.append((label, path, allow_glob))


def add_entrypoint_list(label: str, value: object) -> None:
    if value is None:
        fail(f"{label}: missing")
        return
    if not isinstance(value, list):
        fail(f"{label}: expected list, got {type(value).__name__}")
        return
    if not value:
        fail(f"{label}: empty list")
        return
    for idx, entry in enumerate(value):
        add_target(f"{label}[{idx}]", entry)


# Top-level canonical pointers.
for key in (
    "entrypoint",
    "operating_model",
    "change_control",
    "improvement_loop",
    "escalation_policy",
    "lessons_log",
):
    add_target(key, manifest.get(key))

# Repository and optional submodule entrypoints.
repos = manifest.get("repositories")
if repos is None:
    fail("repositories: missing")
elif not isinstance(repos, list):
    fail(f"repositories: expected list, got {type(repos).__name__}")
else:
    for idx, repo in enumerate(repos):
        if not isinstance(repo, dict):
            fail(f"repositories[{idx}]: expected mapping, got {type(repo).__name__}")
            continue
        key = repo.get("key") or f"index-{idx}"
        entrypoints = repo.get("entrypoints")
        add_entrypoint_list(f"repositories[{key}].entrypoints", entrypoints)
        if isinstance(entrypoints, list):
            repo_entrypoints[key] = {
                item.strip()
                for item in entrypoints
                if isinstance(item, str) and item.strip()
            }

# Prevent navigation drift for the repo-level core guardrail docs.
rustory_required_entrypoints = {
    "AGENTS.md",
    "docs/CHANGE_CONTROL.md",
    "docs/LESSONS_ARCHIVE.md",
    "docs/REPO_MANIFEST.yaml",
}
rustory_entrypoints = repo_entrypoints.get("rustory")
if rustory_entrypoints is None:
    fail("repositories[rustory]: missing")
else:
    for required in sorted(rustory_required_entrypoints):
        if required not in rustory_entrypoints:
            fail(f"repositories[rustory].entrypoints: required entrypoint missing ({required})")

submodules = manifest.get("submodules")
if submodules is not None:
    if not isinstance(submodules, list):
        fail(f"submodules: expected list, got {type(submodules).__name__}")
    else:
        for idx, submodule in enumerate(submodules):
            if not isinstance(submodule, dict):
                fail(f"submodules[{idx}]: expected mapping, got {type(submodule).__name__}")
                continue
            key = submodule.get("key") or submodule.get("path") or f"index-{idx}"
            entrypoints = submodule.get("entrypoints")
            if entrypoints is None:
                continue
            add_entrypoint_list(f"submodules[{key}].entrypoints", entrypoints)

maintenance = manifest.get("maintenance")
if maintenance is None:
    fail("maintenance: missing")
elif not isinstance(maintenance, dict):
    fail(f"maintenance: expected mapping, got {type(maintenance).__name__}")
else:
    for key in (
        "work_start_runner",
        "manifest_check_runner",
        "release_gate_runner",
        "push_gate_runner",
        "submodule_health_script",
        "improvement_log",
    ):
        add_target(f"maintenance.{key}", maintenance.get(key))
    add_target("maintenance.todo_workspace_glob", maintenance.get("todo_workspace_glob"), allow_glob=True)


for label, rel, allow_glob in targets:
    if rel.startswith("/"):
        fail(f"{label}: absolute path is not allowed ({rel})")
        continue
    if placeholder_re.search(rel):
        fail(f"{label}: unresolved placeholder detected ({rel})")
        continue

    abs_path = root / rel
    has_wildcard = bool(wildcard_re.search(rel))
    if allow_glob or has_wildcard:
        parent = abs_path.parent
        if not parent.exists():
            fail(f"{label}: glob parent does not exist ({rel})")
            continue
        print(f"[ OK ] {label}: glob parent exists ({rel})")
        continue

    if not abs_path.exists():
        fail(f"{label}: path not found ({rel})")
        continue
    if abs_path.is_dir():
        fail(f"{label}: expected file but directory found ({rel})")
        continue
    print(f"[ OK ] {label}: {rel}")


if failures:
    for item in failures:
        print(f"[FAIL] {item}", file=sys.stderr)
    raise SystemExit(1)

print("[ OK ] manifest entrypoints check passed")
PY
