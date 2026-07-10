#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/build-release-assets.sh [--target current|TRIPLE] [--dist-dir DIR] [--linux-builder auto|host|zig|docker|ssh]

Build the rr release binary for the requested target and write:
  DIR/rr-<target>
  DIR/rr-<target>.sha256
  DIR/checksums.txt

The updater and installer expect raw executable assets named rr-<target>.

Linux targets default to the native host when target == host target. On
non-Linux or cross-arch hosts, auto mode prefers Zig when available so release
assets do not inherit the glibc version of an arbitrary remote builder. Set
RUSTORY_RELEASE_ZIG_GLIBC=2.17 to override the default glibc baseline. When Zig
is not available, auto uses RUSTORY_RELEASE_LINUX_REMOTE as an SSH builder when
set, otherwise Docker buildx. Set --linux-builder host, or
RUSTORY_RELEASE_LINUX_BUILDER=host, when a native cross C toolchain such as
x86_64-linux-gnu-gcc is available.
USAGE
}

target="current"
dist_dir="dist"
linux_builder="${RUSTORY_RELEASE_LINUX_BUILDER:-auto}"
linux_remote="${RUSTORY_RELEASE_LINUX_REMOTE:-}"

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
    --linux-builder)
      linux_builder="${2:-}"
      if [[ -z "$linux_builder" ]]; then
        echo "--linux-builder requires a value" >&2
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
# shellcheck source=scripts/lib/rust-toolchain.sh
source "$repo_root/scripts/lib/rust-toolchain.sh"
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

host_target="$(current_target)"

if [[ "$target" == "current" ]]; then
  target="$host_target"
  cargo_args=(build --release --locked)
  binary_path="$repo_root/target/release/rr"
else
  cargo_args=(build --release --locked --target "$target")
  binary_path="$repo_root/target/$target/release/rr"
fi

asset_name="rr-${target}"
mkdir -p "$dist_path"

linux_platform_for_target() {
  case "$1" in
    x86_64-unknown-linux-gnu) echo "linux/amd64" ;;
    aarch64-unknown-linux-gnu) echo "linux/arm64" ;;
    *) return 1 ;;
  esac
}

detect_build_revision() {
  if [[ -n "${RUSTORY_BUILD_REVISION:-}" ]]; then
    printf '%s' "$RUSTORY_BUILD_REVISION"
    return 0
  fi
  if command -v git >/dev/null 2>&1; then
    git -C "$repo_root" rev-parse --short=12 HEAD 2>/dev/null && return 0
  fi
  if command -v jj >/dev/null 2>&1; then
    jj --ignore-working-copy -R "$repo_root" log -r @- --no-graph -T 'commit_id.short(12)' 2>/dev/null \
      | tr -d '\r\n ' && return 0
  fi
  printf '%s' "unknown"
}

build_with_cargo() {
  (
    cd "$repo_root"
    rustory_require_cargo
    cargo "${cargo_args[@]}"
  )
}

build_linux_with_docker() {
  local platform="$1"
  local docker_out=""
  local docker_binary=""
  local docker_config=""
  local build_revision=""
  local build_source=""
  local build_dirty=""

  if ! command -v docker >/dev/null 2>&1; then
    echo "docker is required for Linux release assets on this host; pass --linux-builder host if a cross C toolchain is installed" >&2
    return 1
  fi
  if ! docker buildx version >/dev/null 2>&1; then
    echo "docker buildx is required for Linux release assets on this host" >&2
    return 1
  fi

  docker_out="$(mktemp -d "${TMPDIR:-/tmp}/rustory-release-linux.XXXXXX")"
  docker_config="$(mktemp -d "${TMPDIR:-/tmp}/rustory-release-docker-config.XXXXXX")"
  printf '{}\n' > "$docker_config/config.json"
  build_revision="$(detect_build_revision)"
  build_source="${RUSTORY_BUILD_REVISION_SOURCE:-git}"
  build_dirty="${RUSTORY_BUILD_DIRTY:-false}"

  if ! DOCKER_CONFIG="${RUSTORY_RELEASE_DOCKER_CONFIG:-$docker_config}" docker buildx build \
    --platform "$platform" \
    --target builder \
    --build-arg "RUSTORY_BUILD_REVISION=$build_revision" \
    --build-arg "RUSTORY_BUILD_REVISION_SOURCE=$build_source" \
    --build-arg "RUSTORY_BUILD_DIRTY=$build_dirty" \
    --output "type=local,dest=$docker_out" \
    "$repo_root" >/dev/null; then
    rm -rf "$docker_out" "$docker_config"
    return 1
  fi

  docker_binary="$docker_out/app/target/release/rr"
  if [[ ! -x "$docker_binary" ]]; then
    echo "docker build did not produce expected binary: $docker_binary" >&2
    rm -rf "$docker_out" "$docker_config"
    return 1
  fi
  mkdir -p "$(dirname "$binary_path")"
  install -m 755 "$docker_binary" "$binary_path"
  rm -rf "$docker_out" "$docker_config"
}

zig_target_for_linux_target() {
  local glibc_baseline="${RUSTORY_RELEASE_ZIG_GLIBC:-2.17}"
  case "$1" in
    x86_64-unknown-linux-gnu) echo "x86_64-linux-gnu.$glibc_baseline" ;;
    aarch64-unknown-linux-gnu) echo "aarch64-linux-gnu.$glibc_baseline" ;;
    *) return 1 ;;
  esac
}

build_linux_with_zig() {
  local zig_bin=""
  local zig_target=""
  local tmp_dir=""
  local zig_cc=""
  local zig_cxx=""
  local zig_ar=""
  local zig_local_cache=""
  local zig_global_cache=""
  local target_env_upper=""
  local target_env_lower=""

  zig_bin="$(command -v zig || true)"
  if [[ -z "$zig_bin" ]]; then
    echo "zig is required for --linux-builder zig" >&2
    return 1
  fi

  if ! zig_target="$(zig_target_for_linux_target "$target")"; then
    echo "zig release builder does not support target: $target" >&2
    return 1
  fi

  tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/rustory-release-zig.XXXXXX")"
  zig_cc="$tmp_dir/zig-cc"
  zig_cxx="$tmp_dir/zig-cxx"
  zig_ar="$tmp_dir/zig-ar"
  zig_local_cache="${RUSTORY_RELEASE_ZIG_LOCAL_CACHE_DIR:-$tmp_dir/zig-local-cache}"
  zig_global_cache="${RUSTORY_RELEASE_ZIG_GLOBAL_CACHE_DIR:-$tmp_dir/zig-global-cache}"
  mkdir -p "$zig_local_cache" "$zig_global_cache"
  cat > "$zig_cc" <<EOF
#!/usr/bin/env bash
args=()
skip_next=0
for arg in "\$@"; do
  if (( skip_next == 1 )); then
    skip_next=0
    continue
  fi
  case "\$arg" in
    --target=*) continue ;;
    --target) skip_next=1; continue ;;
  esac
  args+=("\$arg")
done
exec $(sh_quote "$zig_bin") cc -target $(sh_quote "$zig_target") "\${args[@]}"
EOF
  cat > "$zig_cxx" <<EOF
#!/usr/bin/env bash
args=()
skip_next=0
for arg in "\$@"; do
  if (( skip_next == 1 )); then
    skip_next=0
    continue
  fi
  case "\$arg" in
    --target=*) continue ;;
    --target) skip_next=1; continue ;;
  esac
  args+=("\$arg")
done
exec $(sh_quote "$zig_bin") c++ -target $(sh_quote "$zig_target") "\${args[@]}"
EOF
  cat > "$zig_ar" <<EOF
#!/usr/bin/env bash
exec $(sh_quote "$zig_bin") ar "\$@"
EOF
  chmod +x "$zig_cc" "$zig_cxx" "$zig_ar"

  target_env_upper="$(printf '%s' "$target" | tr '[:lower:]-' '[:upper:]_')"
  target_env_lower="$(printf '%s' "$target" | tr '-' '_')"

  if ! (
    cd "$repo_root"
    rustory_require_cargo
    env \
      "CARGO_TARGET_${target_env_upper}_LINKER=$zig_cc" \
      "CC_${target_env_lower}=$zig_cc" \
      "CXX_${target_env_lower}=$zig_cxx" \
      "AR_${target_env_lower}=$zig_ar" \
      "ZIG_LOCAL_CACHE_DIR=$zig_local_cache" \
      "ZIG_GLOBAL_CACHE_DIR=$zig_global_cache" \
      PKG_CONFIG_ALLOW_CROSS=1 \
      cargo "${cargo_args[@]}"
  ); then
    rm -rf "$tmp_dir"
    return 1
  fi
  rm -rf "$tmp_dir"
}

sh_quote() {
  local value="$1"
  printf "'%s'" "$(printf '%s' "$value" | sed "s/'/'\\\\''/g")"
}

remote_matches_target() {
  local remote_host="$1"
  case "$target:$remote_host" in
    x86_64-unknown-linux-gnu:Linux:x86_64) return 0 ;;
    aarch64-unknown-linux-gnu:Linux:aarch64) return 0 ;;
    aarch64-unknown-linux-gnu:Linux:arm64) return 0 ;;
    *) return 1 ;;
  esac
}

build_linux_with_ssh() {
  if [[ -z "$linux_remote" ]]; then
    echo "RUSTORY_RELEASE_LINUX_REMOTE is required when --linux-builder ssh is used" >&2
    return 1
  fi
  command -v ssh >/dev/null 2>&1 || {
    echo "ssh is required for --linux-builder ssh" >&2
    return 1
  }
  command -v scp >/dev/null 2>&1 || {
    echo "scp is required for --linux-builder ssh" >&2
    return 1
  }
  command -v git >/dev/null 2>&1 || {
    echo "git is required to package the source for --linux-builder ssh" >&2
    return 1
  }

  local remote_host=""
  local remote_dir=""
  local remote_dir_q=""
  local source_list=""
  local build_revision=""
  local build_source=""
  local build_dirty=""

  remote_host="$(ssh "$linux_remote" 'printf "%s:%s" "$(uname -s)" "$(uname -m)"')"
  if ! remote_matches_target "$remote_host"; then
    echo "remote builder target mismatch: target=$target remote=$linux_remote host=$remote_host" >&2
    return 1
  fi

  remote_dir="${RUSTORY_RELEASE_LINUX_REMOTE_DIR:-/tmp/rustory-release-${target}-$$}"
  remote_dir_q="$(sh_quote "$remote_dir")"
  source_list="$(mktemp "${TMPDIR:-/tmp}/rustory-release-sources.XXXXXX")"
  build_revision="$(detect_build_revision)"
  build_source="${RUSTORY_BUILD_REVISION_SOURCE:-git}"
  build_dirty="${RUSTORY_BUILD_DIRTY:-false}"

  (
    cd "$repo_root"
    git ls-files -co --exclude-standard -z > "$source_list"
    ssh "$linux_remote" "rm -rf $remote_dir_q && mkdir -p $remote_dir_q"
    COPYFILE_DISABLE=1 tar --no-xattrs --null -T "$source_list" -czf - \
      | ssh "$linux_remote" "tar -xzf - -C $remote_dir_q"
  )
  rm -f "$source_list"

  ssh "$linux_remote" \
    "cd $remote_dir_q && RUSTORY_BUILD_REVISION=$(sh_quote "$build_revision") RUSTORY_BUILD_REVISION_SOURCE=$(sh_quote "$build_source") RUSTORY_BUILD_DIRTY=$(sh_quote "$build_dirty") cargo build --release --locked --bin rr"

  mkdir -p "$(dirname "$binary_path")"
  scp "$linux_remote:$remote_dir/target/release/rr" "$binary_path" >/dev/null

  if [[ "${RUSTORY_RELEASE_LINUX_REMOTE_KEEP:-0}" != "1" ]]; then
    ssh "$linux_remote" "rm -rf $remote_dir_q" >/dev/null 2>&1 || true
  fi
}

linux_platform=""
if linux_platform="$(linux_platform_for_target "$target")"; then
  case "$linux_builder" in
    auto)
      if [[ "$host_target" == "$target" ]]; then
        build_with_cargo
      elif command -v zig >/dev/null 2>&1; then
        echo "linux_builder=zig target=$(zig_target_for_linux_target "$target")"
        build_linux_with_zig
      elif [[ -n "$linux_remote" ]]; then
        echo "linux_builder=ssh remote=$linux_remote"
        build_linux_with_ssh
      else
        echo "linux_builder=docker platform=$linux_platform"
        build_linux_with_docker "$linux_platform"
      fi
      ;;
    host)
      build_with_cargo
      ;;
    zig)
      echo "linux_builder=zig target=$(zig_target_for_linux_target "$target")"
      build_linux_with_zig
      ;;
    docker)
      echo "linux_builder=docker platform=$linux_platform"
      build_linux_with_docker "$linux_platform"
      ;;
    ssh)
      echo "linux_builder=ssh remote=$linux_remote"
      build_linux_with_ssh
      ;;
    *)
      echo "unknown --linux-builder: $linux_builder" >&2
      exit 2
      ;;
  esac
else
  build_with_cargo
fi

install -m 755 "$binary_path" "$dist_path/$asset_name"

if [[ -n "$linux_platform" ]]; then
  max_glibc="${RUSTORY_RELEASE_MAX_GLIBC:-${RUSTORY_RELEASE_ZIG_GLIBC:-2.17}}"
  "$repo_root/scripts/check-linux-glibc-baseline.sh" \
    "$dist_path/$asset_name" "$max_glibc"
fi

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
