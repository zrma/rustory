#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Verify that a Linux release binary does not require a newer GLIBC than allowed.

Usage:
  scripts/check-linux-glibc-baseline.sh <binary> [max-glibc]

The default maximum is RUSTORY_RELEASE_MAX_GLIBC or 2.17.
USAGE
}

binary="${1:-}"
max_glibc="${2:-${RUSTORY_RELEASE_MAX_GLIBC:-2.17}}"

if [[ -z "$binary" || "${binary:-}" == "-h" || "${binary:-}" == "--help" ]]; then
  usage
  [[ -n "$binary" ]] && exit 0
  exit 2
fi
if [[ $# -gt 2 ]]; then
  usage >&2
  exit 2
fi
if [[ ! -f "$binary" ]]; then
  echo "[FAIL] Linux release binary not found: $binary" >&2
  exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "[FAIL] python3 is required to verify the Linux GLIBC baseline" >&2
  exit 1
fi

python3 - "$binary" "$max_glibc" <<'PY'
import pathlib
import re
import sys

binary = pathlib.Path(sys.argv[1])
allowed_text = sys.argv[2]

if re.fullmatch(r"[0-9]+(?:\.[0-9]+)*", allowed_text) is None:
    print(f"[FAIL] invalid max GLIBC version: {allowed_text}", file=sys.stderr)
    raise SystemExit(2)

allowed = tuple(int(part) for part in allowed_text.split("."))
matches = {
    tuple(int(part) for part in match.decode("ascii").split("."))
    for match in re.findall(rb"GLIBC_([0-9]+(?:\.[0-9]+)*)", binary.read_bytes())
}

if not matches:
    print(f"glibc_compat=ok required_max=none allowed_max={allowed_text}")
    raise SystemExit(0)

required = max(matches)
width = max(len(required), len(allowed))
required_cmp = required + (0,) * (width - len(required))
allowed_cmp = allowed + (0,) * (width - len(allowed))
required_text = ".".join(str(part) for part in required)

if required_cmp > allowed_cmp:
    print(
        f"[FAIL] Linux release binary requires GLIBC_{required_text}; "
        f"allowed maximum is GLIBC_{allowed_text}: {binary}",
        file=sys.stderr,
    )
    raise SystemExit(1)

print(f"glibc_compat=ok required_max={required_text} allowed_max={allowed_text}")
PY
