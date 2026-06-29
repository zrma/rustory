#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

usage() {
  cat <<'USAGE'
Usage:
  scripts/release-version.sh [--version X.Y.Z|vX.Y.Z] [options]

Build and publish rr GitHub Release assets for the version in Cargo.toml.

Options:
  --version <version>       Release version/tag. Defaults to Cargo.toml version.
                            Both 1.2.3 and v1.2.3 are accepted.
  --profile <name>          Target profile: current|daily-driver|full (default: current)
                            current: this host only
                            daily-driver: macOS arm64 + Linux x86_64
                            full: macOS arm64/x86_64 + Linux x86_64/arm64
  --target <target>         Build/upload a target. Repeatable. Overrides --profile.
                            Use "current" for this host.
  --dist-dir <dir>          Asset staging directory (default: dist/release-vX.Y.Z)
  --repo <owner/repo>       GitHub repo for gh release/update verify (default: zrma/rustory)
  --target-ref <rev>        jj/git revision used as release target (default: main)
  --remote <name>           Remote branch checked before upload (default: origin)
  --work-id <id>            Work id for release gates.
  --gate <none|quick|full>  Pre-release gate mode (default: quick)
                            quick: scripts/run-manifest-checks.sh --mode quick
                            full: scripts/check-release-gates.sh --manifest-mode full
  --skip-build              Upload/verify already staged assets.
  --skip-upload             Build assets but do not call gh release.
  --skip-update-verify      Do not run rr update --dry-run after upload.
  --allow-dirty             Allow publishing from a dirty working copy.
  --allow-version-mismatch  Allow --version to differ from Cargo.toml version.
  --no-remote-check         Do not require <remote>/main to match --target-ref.
  --draft                   Create a draft release when the release does not exist.
  --prerelease              Mark a newly created release as prerelease.
  --dry-run                 Print commands without executing them.
  -h, --help                Show help.

Examples:
  scripts/release-version.sh --profile current --dry-run
  scripts/release-version.sh --profile daily-driver --work-id release-automation
  scripts/release-version.sh --version v1.0.9 --profile daily-driver --gate none
USAGE
}

fail() {
  echo "[FAIL] $*" >&2
  exit 1
}

ok() {
  echo "[ OK ] $*"
}

warn() {
  echo "[WARN] $*" >&2
}

run_argv() {
  if [[ "$DRY_RUN" -eq 1 ]]; then
    printf '[DRY]'
    local token=""
    for token in "$@"; do
      printf ' %q' "$token"
    done
    printf '\n'
    return 0
  fi
  (cd "$ROOT" && "$@")
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

parse_opt_value() {
  local opt_name="$1"
  local opt_value="${2:-}"
  if [[ -z "$opt_value" ]]; then
    echo "missing value for $opt_name" >&2
    usage >&2
    exit 2
  fi
  printf '%s' "$opt_value"
}

read_cargo_version() {
  sed -nE 's/^[[:space:]]*version[[:space:]]*=[[:space:]]*"([^"]+)".*$/\1/p' \
    "$ROOT/Cargo.toml" | head -n 1
}

normalize_version() {
  local raw="$1"
  raw="${raw#v}"
  if [[ ! "$raw" =~ ^[0-9]+\.[0-9]+\.[0-9]+([._+-][0-9A-Za-z.-]+)?$ ]]; then
    fail "invalid version: $1"
  fi
  printf '%s' "$raw"
}

current_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "${os}:${arch}" in
    Darwin:arm64) echo "aarch64-apple-darwin" ;;
    Darwin:x86_64) echo "x86_64-apple-darwin" ;;
    Linux:x86_64) echo "x86_64-unknown-linux-gnu" ;;
    Linux:aarch64|Linux:arm64) echo "aarch64-unknown-linux-gnu" ;;
    *) fail "unsupported release target: ${os}:${arch}" ;;
  esac
}

resolve_targets_from_profile() {
  case "$PROFILE" in
    current)
      TARGETS=(current)
      ;;
    daily-driver)
      TARGETS=(aarch64-apple-darwin x86_64-unknown-linux-gnu)
      ;;
    full)
      TARGETS=(aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu)
      ;;
    *)
      fail "unknown --profile: $PROFILE"
      ;;
  esac
}

resolve_commit_sha() {
  local ref="$1"
  local sha=""
  if [[ -d "$ROOT/.jj" ]] && command -v jj >/dev/null 2>&1; then
    sha="$(cd "$ROOT" && jj log -r "$ref" --no-graph -T 'commit_id' | tr -d '\r\n ')"
  else
    sha="$(cd "$ROOT" && git rev-parse "$ref" | tr -d '\r\n ')"
  fi
  [[ -n "$sha" ]] || fail "failed to resolve release target ref: $ref"
  printf '%s' "$sha"
}

check_clean_worktree() {
  if [[ "$ALLOW_DIRTY" -eq 1 || "$DRY_RUN" -eq 1 ]]; then
    return 0
  fi
  if [[ -d "$ROOT/.jj" ]] && command -v jj >/dev/null 2>&1; then
    local status=""
    status="$(cd "$ROOT" && jj status)"
    if [[ "$status" != *"The working copy has no changes."* ]]; then
      echo "$status" >&2
      fail "working copy is dirty; commit first or pass --allow-dirty"
    fi
  else
    if [[ -n "$(cd "$ROOT" && git status --short)" ]]; then
      (cd "$ROOT" && git status --short) >&2
      fail "working copy is dirty; commit first or pass --allow-dirty"
    fi
  fi
}

check_remote_main() {
  if [[ "$REMOTE_CHECK" -eq 0 || "$DRY_RUN" -eq 1 || "$SKIP_UPLOAD" -eq 1 ]]; then
    return 0
  fi
  need_cmd git
  local remote_sha=""
  remote_sha="$(cd "$ROOT" && git ls-remote --heads "$REMOTE" main | awk '{print $1}' | tr -d '\r\n ')"
  [[ -n "$remote_sha" ]] || fail "failed to resolve remote main: $REMOTE/main"
  if [[ "$remote_sha" != "$COMMIT_SHA" ]]; then
    fail "$REMOTE/main ($remote_sha) does not match release target $TARGET_REF ($COMMIT_SHA)"
  fi
  ok "remote main matches release target: $REMOTE/main $remote_sha"
}

run_gate() {
  case "$GATE" in
    none)
      warn "skip release gates (--gate none)"
      ;;
    quick)
      if [[ -n "$WORK_ID" ]]; then
        run_argv scripts/run-manifest-checks.sh --mode quick --work-id "$WORK_ID"
      else
        run_argv scripts/run-manifest-checks.sh --mode quick
      fi
      ;;
    full)
      if [[ -n "$WORK_ID" ]]; then
        run_argv scripts/check-release-gates.sh --manifest-mode full --work-id "$WORK_ID"
      else
        run_argv scripts/check-release-gates.sh --manifest-mode full
      fi
      ;;
    *)
      fail "unknown --gate: $GATE"
      ;;
  esac
}

build_or_collect_assets() {
  local target=""
  local resolved_target=""
  local asset=""
  local checksum=""
  ASSETS=()

  for target in "${TARGETS[@]}"; do
    resolved_target="$target"
    if [[ "$target" == "current" ]]; then
      resolved_target="$(current_target)"
    fi

    if [[ "$SKIP_BUILD" -eq 0 ]]; then
      run_argv scripts/build-release-assets.sh --target "$target" --dist-dir "$DIST_DIR"
    else
      warn "skip build for $resolved_target (--skip-build)"
    fi

    asset="$DIST_PATH/rr-$resolved_target"
    checksum="$asset.sha256"
    if [[ "$DRY_RUN" -eq 0 ]]; then
      [[ -x "$asset" ]] || fail "missing release asset: $asset"
      [[ -f "$checksum" ]] || fail "missing release checksum: $checksum"
    fi
    ASSETS+=("$asset" "$checksum")
  done
}

release_exists() {
  gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1
}

publish_release() {
  if [[ "$SKIP_UPLOAD" -eq 1 ]]; then
    warn "skip GitHub release upload (--skip-upload)"
    return 0
  fi

  need_cmd gh
  if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "[DRY] release existence check chooses create vs upload:"
    run_argv gh release view "$TAG" --repo "$REPO"
    local create_cmd=(gh release create "$TAG")
    create_cmd+=("${ASSETS[@]}")
    create_cmd+=(--repo "$REPO" --target "$COMMIT_SHA" --title "$TAG" --notes "Rustory $TAG")
    if [[ "$DRAFT" -eq 1 ]]; then
      create_cmd+=(--draft)
    fi
    if [[ "$PRERELEASE" -eq 1 ]]; then
      create_cmd+=(--prerelease)
    fi
    run_argv "${create_cmd[@]}"
    run_argv gh release upload "$TAG" "${ASSETS[@]}" --repo "$REPO" --clobber
    return 0
  fi

  if release_exists; then
    run_argv gh release upload "$TAG" "${ASSETS[@]}" --repo "$REPO" --clobber
  else
    local cmd=(gh release create "$TAG")
    cmd+=("${ASSETS[@]}")
    cmd+=(--repo "$REPO" --target "$COMMIT_SHA" --title "$TAG" --notes "Rustory $TAG")
    if [[ "$DRAFT" -eq 1 ]]; then
      cmd+=(--draft)
    fi
    if [[ "$PRERELEASE" -eq 1 ]]; then
      cmd+=(--prerelease)
    fi
    run_argv "${cmd[@]}"
  fi
}

verify_update_plan() {
  if [[ "$VERIFY_UPDATE" -eq 0 || "$SKIP_UPLOAD" -eq 1 ]]; then
    return 0
  fi
  if command -v rr >/dev/null 2>&1; then
    run_argv rr update --repo "$REPO" --version "$TAG" --dry-run
  else
    warn "skip rr update verification: rr not found in PATH"
  fi
}

VERSION_INPUT=""
PROFILE="current"
TARGETS=()
TARGET_COUNT=0
DIST_DIR=""
REPO="zrma/rustory"
TARGET_REF="main"
REMOTE="origin"
WORK_ID=""
GATE="quick"
SKIP_BUILD=0
SKIP_UPLOAD=0
VERIFY_UPDATE=1
ALLOW_DIRTY=0
ALLOW_VERSION_MISMATCH=0
REMOTE_CHECK=1
DRAFT=0
PRERELEASE=0
DRY_RUN=0
ASSETS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      VERSION_INPUT="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --profile)
      PROFILE="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --target)
      TARGETS+=("$(parse_opt_value "$1" "${2:-}")")
      TARGET_COUNT=$((TARGET_COUNT + 1))
      shift 2
      ;;
    --dist-dir)
      DIST_DIR="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --repo)
      REPO="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --target-ref)
      TARGET_REF="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --remote)
      REMOTE="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --work-id)
      WORK_ID="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --gate)
      GATE="$(parse_opt_value "$1" "${2:-}")"
      shift 2
      ;;
    --skip-build)
      SKIP_BUILD=1
      shift
      ;;
    --skip-upload)
      SKIP_UPLOAD=1
      shift
      ;;
    --skip-update-verify)
      VERIFY_UPDATE=0
      shift
      ;;
    --allow-dirty)
      ALLOW_DIRTY=1
      shift
      ;;
    --allow-version-mismatch)
      ALLOW_VERSION_MISMATCH=1
      shift
      ;;
    --no-remote-check)
      REMOTE_CHECK=0
      shift
      ;;
    --draft)
      DRAFT=1
      shift
      ;;
    --prerelease)
      PRERELEASE=1
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

CARGO_VERSION="$(read_cargo_version)"
[[ -n "$CARGO_VERSION" ]] || fail "failed to read Cargo.toml package version"

if [[ -z "$VERSION_INPUT" ]]; then
  VERSION_INPUT="$CARGO_VERSION"
fi

VERSION="$(normalize_version "$VERSION_INPUT")"
TAG="v$VERSION"

if [[ "$VERSION" != "$CARGO_VERSION" && "$ALLOW_VERSION_MISMATCH" -eq 0 ]]; then
  fail "release version $VERSION does not match Cargo.toml version $CARGO_VERSION"
fi

if [[ "$TARGET_COUNT" -eq 0 ]]; then
  resolve_targets_from_profile
fi

if [[ -z "$DIST_DIR" ]]; then
  DIST_DIR="dist/release-$TAG"
fi

if [[ "$DIST_DIR" = /* ]]; then
  DIST_PATH="$DIST_DIR"
else
  DIST_PATH="$ROOT/$DIST_DIR"
fi

COMMIT_SHA="$(resolve_commit_sha "$TARGET_REF")"

echo "release plan: tag=$TAG version=$VERSION target_ref=$TARGET_REF commit=$COMMIT_SHA"
echo "profile=$PROFILE targets=${TARGETS[*]} dist_dir=$DIST_DIR repo=$REPO gate=$GATE"

check_clean_worktree
run_gate
check_remote_main
build_or_collect_assets
publish_release
verify_update_plan

ok "release flow completed: $TAG"
