#!/usr/bin/env python3
"""Install or update Rustory's `rr` CLI from GitHub release assets.

Designed for:

  curl -fsSL https://raw.githubusercontent.com/zrma/rustory/main/install/rustory.py | \
    python3 - --token "$RUSTORY_TRACKER_TOKEN" --tracker "$RUSTORY_TRACKERS"
"""

from __future__ import annotations

import argparse
import hashlib
import os
import platform
import stat
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path


DEFAULT_REPO = "zrma/rustory"
MAX_ASSET_BYTES = 128 * 1024 * 1024
MAX_CHECKSUM_BYTES = 64 * 1024


def main() -> int:
    args = parse_args()
    target = current_release_target()
    asset_name = f"rr-{target}"
    asset_url = resolve_asset_url(args, asset_name)
    checksum_url = args.checksum_url or f"{asset_url}.sha256"
    bin_dir = Path(args.bin_dir).expanduser()
    install_path = bin_dir / "rr"

    print(f"install_path={install_path}")
    print(f"asset_url={asset_url}")

    data = download_bytes(asset_url, MAX_ASSET_BYTES)
    expected = normalize_sha256(args.sha256) if args.sha256 else fetch_checksum(checksum_url, asset_name)
    actual = hashlib.sha256(data).hexdigest()
    if actual != expected:
        raise SystemExit(f"checksum mismatch: expected {expected}, actual {actual}")
    print(f"checksum=ok sha256={actual}")

    install_binary(data, install_path)
    verify_binary(install_path)

    if args.token or args.trackers or args.relay:
        run_init(install_path, args)

    print("rustory install ok")
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Install or update Rustory rr")
    parser.add_argument("--version", default="latest", help="Release version: latest or a tag such as v1.0.2")
    parser.add_argument("--repo", default=DEFAULT_REPO, help="GitHub repository that publishes release assets")
    parser.add_argument("--asset-base-url", help="Override release asset base URL; downloads <base>/rr-<target>")
    parser.add_argument("--asset-url", help="Override exact release asset URL")
    parser.add_argument("--checksum-url", help="Override SHA-256 checksum URL; defaults to <asset-url>.sha256")
    parser.add_argument("--sha256", help="Expected SHA-256 hex; skips checksum URL download")
    parser.add_argument("--bin-dir", default="~/.local/bin", help="Directory to install rr into")
    parser.add_argument("--token", help="Tracker bearer token to write via rr init")
    parser.add_argument(
        "--tracker",
        dest="trackers",
        action="append",
        default=[],
        help="Tracker URL to write via rr init; may be repeated or comma-separated",
    )
    parser.add_argument("--relay", help="Relay multiaddr to write via rr init")
    parser.add_argument("--user-id", help="Logical Rustory user id to write via rr init")
    parser.add_argument("--device-id", help="Device id to write via rr init")
    parser.add_argument("--force", action="store_true", help="Pass --force to rr init")
    args = parser.parse_args()
    if args.asset_base_url and args.asset_url:
        parser.error("pass only one of --asset-base-url or --asset-url")
    return args


def current_release_target() -> str:
    system = platform.system().lower()
    machine = platform.machine().lower()
    if machine == "arm64":
        machine = "aarch64"
    if system == "darwin" and machine == "aarch64":
        return "aarch64-apple-darwin"
    if system == "darwin" and machine == "x86_64":
        return "x86_64-apple-darwin"
    if system == "linux" and machine == "x86_64":
        return "x86_64-unknown-linux-gnu"
    if system == "linux" and machine == "aarch64":
        return "aarch64-unknown-linux-gnu"
    raise SystemExit(f"unsupported install target: {platform.system()} {platform.machine()}")


def resolve_asset_url(args: argparse.Namespace, asset_name: str) -> str:
    if args.asset_url:
        return args.asset_url.strip()
    if args.asset_base_url:
        return f"{args.asset_base_url.rstrip('/')}/{asset_name}"
    repo = normalize_repo(args.repo)
    version = args.version.strip()
    if not version:
        raise SystemExit("--version must not be empty")
    if version == "latest":
        return f"https://github.com/{repo}/releases/latest/download/{asset_name}"
    return f"https://github.com/{repo}/releases/download/{version}/{asset_name}"


def normalize_repo(repo: str) -> str:
    repo = repo.strip()
    if "/" not in repo or ".." in repo or any(ch.isspace() for ch in repo):
        raise SystemExit("--repo must be a GitHub owner/name value")
    return repo


def download_bytes(url: str, limit: int) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "rustory-installer"})
    with urllib.request.urlopen(req, timeout=60) as response:
        data = response.read(limit + 1)
    if len(data) > limit:
        raise SystemExit(f"download too large: {url}")
    return data


def fetch_checksum(url: str, asset_name: str) -> str:
    print(f"checksum_url={url}")
    text = download_bytes(url, MAX_CHECKSUM_BYTES).decode("utf-8")
    return parse_checksum(text, asset_name)


def parse_checksum(text: str, asset_name: str) -> str:
    first_valid: str | None = None
    saw_named_checksum = False
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        candidate = parts[0]
        try:
            normalized = normalize_sha256(candidate)
        except SystemExit:
            continue
        if len(parts) == 1:
            first_valid = first_valid or normalized
            continue
        names = [name.lstrip("*").lstrip("./") for name in parts[1:]]
        saw_named_checksum = True
        if any(name.endswith(asset_name) for name in names):
            return normalized
    if saw_named_checksum:
        raise SystemExit(f"no SHA-256 checksum found for {asset_name}")
    if first_valid:
        return first_valid
    raise SystemExit(f"no SHA-256 checksum found for {asset_name}")


def normalize_sha256(value: str) -> str:
    value = value.strip().lower()
    if len(value) != 64 or any(ch not in "0123456789abcdef" for ch in value):
        raise SystemExit("SHA-256 must be exactly 64 hex characters")
    return value


def install_binary(data: bytes, install_path: Path) -> None:
    install_path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp_name = tempfile.mkstemp(prefix=f".{install_path.name}.", dir=str(install_path.parent))
    tmp_path = Path(tmp_name)
    try:
        with os.fdopen(fd, "wb") as file:
            file.write(data)
            file.flush()
            os.fsync(file.fileno())
        tmp_path.chmod(stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR | stat.S_IRGRP | stat.S_IXGRP | stat.S_IROTH | stat.S_IXOTH)
        os.replace(tmp_path, install_path)
    finally:
        if tmp_path.exists():
            tmp_path.unlink()


def verify_binary(install_path: Path) -> None:
    output = subprocess.run(
        [str(install_path), "version"],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    first_line = output.stdout.splitlines()[0] if output.stdout.splitlines() else "version output unavailable"
    print(f"binary_check={first_line}")


def split_tracker_values(values: list[str]) -> list[str]:
    trackers: list[str] = []
    for value in values:
        trackers.extend(item.strip() for item in value.split(",") if item.strip())
    return trackers


def run_init(install_path: Path, args: argparse.Namespace) -> None:
    cmd = [str(install_path), "init"]
    if args.force:
        cmd.append("--force")
    if args.user_id:
        cmd += ["--user-id", args.user_id]
    if args.device_id:
        cmd += ["--device-id", args.device_id]
    for tracker in split_tracker_values(args.trackers):
        cmd += ["--tracker", tracker]
    if args.relay:
        cmd += ["--relay", args.relay]
    if args.token:
        cmd += ["--token", args.token]

    print("init=running token_configured={} trackers={}".format(bool(args.token), len(split_tracker_values(args.trackers))))
    subprocess.run(cmd, check=True)


if __name__ == "__main__":
    raise SystemExit(main())
