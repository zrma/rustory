#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/build-release-assets.sh [--target current|TRIPLE] [--dist-dir DIR]

Build the rr release binary for the requested target and write:
  DIR/rr-<target>
  DIR/rr-<target>.sha256
  DIR/checksums.txt

The updater and installer expect raw executable assets named rr-<target>.
USAGE
}

target="current"
dist_dir="dist"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      target="${2:-}"
      if [[ -z "$target" ]]; then
        echo "--target requires a value" >&2
        exit 2
      fi
      shift 2
      ;;
    --dist-dir)
      dist_dir="${2:-}"
      if [[ -z "$dist_dir" ]]; then
        echo "--dist-dir requires a value" >&2
        exit 2
      fi
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "$dist_dir" = /* ]]; then
  dist_path="$dist_dir"
else
  dist_path="$repo_root/$dist_dir"
fi

current_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "${os}:${arch}" in
    Darwin:arm64) echo "aarch64-apple-darwin" ;;
    Darwin:x86_64) echo "x86_64-apple-darwin" ;;
    Linux:x86_64) echo "x86_64-unknown-linux-gnu" ;;
    Linux:aarch64|Linux:arm64) echo "aarch64-unknown-linux-gnu" ;;
    *)
      echo "unsupported release target: ${os}:${arch}" >&2
      return 1
      ;;
  esac
}

if [[ "$target" == "current" ]]; then
  target="$(current_target)"
  cargo_args=(build --release --locked)
  binary_path="$repo_root/target/release/rr"
else
  cargo_args=(build --release --locked --target "$target")
  binary_path="$repo_root/target/$target/release/rr"
fi

asset_name="rr-${target}"
mkdir -p "$dist_path"

(
  cd "$repo_root"
  cargo "${cargo_args[@]}"
)

install -m 755 "$binary_path" "$dist_path/$asset_name"

checksum_file="$dist_path/$asset_name.sha256"
checksums_file="$dist_path/checksums.txt"
touch "$checksums_file"
previous_checksums="$(mktemp)"
grep -v -E "[[:space:]]\\*?\\.?/?${asset_name}$" "$checksums_file" > "$previous_checksums" || true
cat "$previous_checksums" > "$checksums_file"
rm -f "$previous_checksums"

if command -v sha256sum >/dev/null 2>&1; then
  (
    cd "$dist_path"
    sha256sum "$asset_name" > "$asset_name.sha256"
    sha256sum "$asset_name" >> "$checksums_file"
  )
else
  digest="$(shasum -a 256 "$dist_path/$asset_name" | awk '{print $1}')"
  printf '%s  %s\n' "$digest" "$asset_name" > "$checksum_file"
  printf '%s  %s\n' "$digest" "$asset_name" >> "$checksums_file"
fi

echo "asset=$dist_path/$asset_name"
echo "checksum=$dist_path/$asset_name.sha256"
echo "checksums=$dist_path/checksums.txt"
