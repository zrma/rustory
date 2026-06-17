#!/usr/bin/env bash

rustory_ensure_cargo_on_path() {
  if command -v cargo >/dev/null 2>&1; then
    return 0
  fi

  local rustup_bin="${RUSTUP:-rustup}"
  if ! command -v "$rustup_bin" >/dev/null 2>&1; then
    return 1
  fi

  local cargo_path
  if ! cargo_path="$("$rustup_bin" which cargo 2>/dev/null)"; then
    return 1
  fi
  if [[ -z "$cargo_path" || ! -x "$cargo_path" ]]; then
    return 1
  fi

  local cargo_dir
  cargo_dir="$(cd "$(dirname "$cargo_path")" && pwd)"
  export PATH="$cargo_dir:$PATH"
}

rustory_require_cargo() {
  if rustory_ensure_cargo_on_path && command -v cargo >/dev/null 2>&1; then
    return 0
  fi

  echo "error: missing command: cargo (install Rust or make rustup available on PATH)" >&2
  return 127
}
