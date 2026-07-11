#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/rust-toolchain.sh
source "$ROOT_DIR/scripts/lib/rust-toolchain.sh"
ACC_DIR="${RUSTORY_RETIREMENT_ACCEPTANCE_DIR:-$ROOT_DIR/target/acceptance/device-retirement-vms}"
MACOS_BASE="${RUSTORY_RETIREMENT_MACOS_BASE:-rustory-macos-base}"
LINUX_BASE="${RUSTORY_RETIREMENT_LINUX_BASE:-rustory-linux-base}"

MACOS_SCENARIOS=(
  rustory-macos-retire-happy
  rustory-macos-retire-offline
  rustory-macos-retire-ack-retry
)
LINUX_SCENARIOS=(rustory-linux-retire-complete-retry)

usage() {
  cat <<'USAGE'
Prepare and operate the disposable VM boundary for device-retirement acceptance.

Usage:
  scripts/acceptance_device_retirement_vms.sh preflight
  scripts/acceptance_device_retirement_vms.sh status
  scripts/acceptance_device_retirement_vms.sh caddy <normal|fail-ack|fail-complete>
  scripts/acceptance_device_retirement_vms.sh cleanup --yes

Commands:
  preflight  Check required local tools, stopped base VMs, and all
             fault-injection Caddy configurations, then build current rr.
             Does not start VMs.
  status     Show only the base and fixed disposable scenario VM names.
  caddy      Run the HTTPS reverse proxy in the selected fault mode.
  cleanup    Stop and delete only the fixed disposable scenario clones. Base VMs
             and unrelated Tart/Lima instances are never deleted.

Required environment for `caddy`:
  RUSTORY_ACCEPTANCE_TLS_NAME
  RUSTORY_ACCEPTANCE_CERT_FILE
  RUSTORY_ACCEPTANCE_KEY_FILE

Optional environment:
  RUSTORY_RETIREMENT_ACCEPTANCE_DIR
  RUSTORY_RETIREMENT_MACOS_BASE
  RUSTORY_RETIREMENT_LINUX_BASE

The scenario order, invariants, and evidence template are documented in
docs/acceptance/device-retirement-vms.md.
USAGE
}

need_cmd() {
  local name="$1"
  if ! command -v "$name" >/dev/null 2>&1; then
    echo "error: missing command: $name" >&2
    exit 127
  fi
}

has_tart_vm() {
  local name="$1"
  tart list --source local --format json 2>/dev/null |
    jq -e --arg name "$name" 'any(.[]; .Name == $name)' >/dev/null
}

has_lima_vm() {
  local name="$1"
  limactl list --json 2>/dev/null | jq -e --arg name "$name" 'select(.name == $name)' >/dev/null
}

caddy_config() {
  case "$1" in
    normal)
      printf '%s\n' "$ROOT_DIR/scripts/acceptance/device-retirement/Caddyfile"
      ;;
    fail-ack)
      printf '%s\n' "$ROOT_DIR/scripts/acceptance/device-retirement/Caddyfile.fail-ack"
      ;;
    fail-complete)
      printf '%s\n' "$ROOT_DIR/scripts/acceptance/device-retirement/Caddyfile.fail-complete"
      ;;
    *)
      echo "error: unknown Caddy mode: $1" >&2
      exit 2
      ;;
  esac
}

validate_caddy_configs() {
  local mode config
  for mode in normal fail-ack fail-complete; do
    config="$(caddy_config "$mode")"
    RUSTORY_ACCEPTANCE_TLS_NAME=acceptance.invalid \
      RUSTORY_ACCEPTANCE_CERT_FILE=/tmp/acceptance.invalid.crt \
      RUSTORY_ACCEPTANCE_KEY_FILE=/tmp/acceptance.invalid.key \
      caddy adapt --config "$config" --adapter caddyfile >/dev/null
  done
}

preflight() {
  need_cmd caddy
  need_cmd jq
  need_cmd limactl
  need_cmd tart
  need_cmd tailscale
  rustory_require_cargo

  if [[ "$(uname -s)" != Darwin ]]; then
    echo "error: Tart acceptance requires a macOS host" >&2
    exit 1
  fi
  if [[ "$(uname -m)" != arm64 ]]; then
    echo "error: the checked-in VM profile is validated on Apple silicon only" >&2
    exit 1
  fi
  if ! has_tart_vm "$MACOS_BASE"; then
    echo "error: missing Tart base VM: $MACOS_BASE" >&2
    exit 1
  fi
  if ! has_lima_vm "$LINUX_BASE"; then
    echo "error: missing Lima base VM: $LINUX_BASE" >&2
    exit 1
  fi
  if ! tart list --source local --format json 2>/dev/null |
    jq -e --arg name "$MACOS_BASE" 'any(.[]; .Name == $name and .Running == false)' >/dev/null; then
    echo "error: Tart base VM must be stopped: $MACOS_BASE" >&2
    exit 1
  fi
  if ! limactl list --json 2>/dev/null |
    jq -e --arg name "$LINUX_BASE" 'select(.name == $name and .status == "Stopped")' >/dev/null; then
    echo "error: Lima base VM must be stopped: $LINUX_BASE" >&2
    exit 1
  fi

  validate_caddy_configs
  cargo build --locked

  printf 'preflight=ok macos_base=%s linux_base=%s rr=%s\n' \
    "$MACOS_BASE" "$LINUX_BASE" "$ROOT_DIR/target/debug/rr"
  echo "next: follow docs/acceptance/device-retirement-vms.md from the strict tracker setup"
}

status() {
  need_cmd jq
  need_cmd limactl
  need_cmd tart

  echo "Tart base/scenarios:"
  tart list --source local --format json 2>/dev/null | jq -r --arg base "$MACOS_BASE" '
    .[] | select(.Name == $base or (.Name | startswith("rustory-macos-retire-"))) |
    [.Name, .State, (.Disk | tostring)] | @tsv
  '
  echo "Lima base/scenarios:"
  limactl list --json 2>/dev/null | jq -r --arg base "$LINUX_BASE" '
    select(.name == $base or (.name | startswith("rustory-linux-retire-"))) |
    [.name, .status, (.arch // "unknown")] | @tsv
  '
}

run_caddy() {
  local mode="$1"
  local config
  need_cmd caddy
  config="$(caddy_config "$mode")"

  : "${RUSTORY_ACCEPTANCE_TLS_NAME:?set RUSTORY_ACCEPTANCE_TLS_NAME}"
  : "${RUSTORY_ACCEPTANCE_CERT_FILE:?set RUSTORY_ACCEPTANCE_CERT_FILE}"
  : "${RUSTORY_ACCEPTANCE_KEY_FILE:?set RUSTORY_ACCEPTANCE_KEY_FILE}"
  [[ -f "$RUSTORY_ACCEPTANCE_CERT_FILE" ]] || {
    echo "error: certificate file not found: $RUSTORY_ACCEPTANCE_CERT_FILE" >&2
    exit 1
  }
  [[ -f "$RUSTORY_ACCEPTANCE_KEY_FILE" ]] || {
    echo "error: key file not found: $RUSTORY_ACCEPTANCE_KEY_FILE" >&2
    exit 1
  }

  mkdir -p "$ACC_DIR/caddy-data" "$ACC_DIR/caddy-config"
  export XDG_DATA_HOME="$ACC_DIR/caddy-data"
  export XDG_CONFIG_HOME="$ACC_DIR/caddy-config"
  exec caddy run --config "$config" --adapter caddyfile
}

cleanup() {
  local confirmation="${1:-}"
  local name
  need_cmd jq
  need_cmd limactl
  need_cmd tart

  if [[ "$confirmation" != --yes ]]; then
    echo "error: cleanup requires --yes" >&2
    exit 2
  fi

  for name in "${MACOS_SCENARIOS[@]}"; do
    if has_tart_vm "$name"; then
      tart stop "$name" >/dev/null 2>&1 || true
      tart delete "$name"
    fi
  done
  for name in "${LINUX_SCENARIOS[@]}"; do
    if has_lima_vm "$name"; then
      limactl stop "$name" >/dev/null 2>&1 || true
      limactl delete "$name"
    fi
  done
  echo "cleanup=ok bases_preserved=$MACOS_BASE,$LINUX_BASE"
}

command="${1:-}"
case "$command" in
  preflight)
    [[ $# -eq 1 ]] || { usage >&2; exit 2; }
    preflight
    ;;
  status)
    [[ $# -eq 1 ]] || { usage >&2; exit 2; }
    status
    ;;
  caddy)
    [[ $# -eq 2 ]] || { usage >&2; exit 2; }
    run_caddy "$2"
    ;;
  cleanup)
    [[ $# -eq 2 ]] || { usage >&2; exit 2; }
    cleanup "$2"
    ;;
  -h|--help)
    usage
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
