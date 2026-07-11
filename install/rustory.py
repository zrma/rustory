#!/usr/bin/env python3
"""Install or update Rustory's `rr` CLI from GitHub release assets.

Designed for:

  curl -fsSL https://raw.githubusercontent.com/zrma/rustory/main/install/rustory.py | \
    python3 - --token "$RUSTORY_TRACKER_TOKEN" --tracker "$RUSTORY_TRACKERS" \
      --relay "$RUSTORY_RELAY_ADDR" --user-id "$RUSTORY_USER_ID" \
      --swarm-key-b64 "$RUSTORY_SWARM_KEY_B64" --install-hook --import-hishtory
"""

from __future__ import annotations

import argparse
import base64
import binascii
import fcntl
import html
import hashlib
import ipaddress
import json
import os
import platform
import re
import signal
import stat
import subprocess
import sys
import tempfile
import time
import urllib.parse
import urllib.request
from pathlib import Path


DEFAULT_REPO = "zrma/rustory"
MAX_ASSET_BYTES = 128 * 1024 * 1024
MAX_CHECKSUM_BYTES = 64 * 1024
HOOK_START = "# >>> rustory hook >>>"
HOOK_END = "# <<< rustory hook <<<"
LEGACY_HOOK_START = "# >>> rustory >>>"
LEGACY_HOOK_END = "# <<< rustory <<<"
DAEMON_AUTOSTART_START = "# >>> rustory daemon autostart >>>"
DAEMON_AUTOSTART_END = "# <<< rustory daemon autostart <<<"
MANAGED_STATE_HOME_FILE = "managed-state-home"
MANAGED_STATE_HOMES_FILE = "managed-state-homes.json"
MANAGED_STATE_HOMES_LOCK_FILE = ".managed-state-homes.lock"
MANAGED_RC_LOCK_FILE = ".managed-rc-files.lock"
MAX_MANAGED_STATE_HOMES = 32
SUPPORTED_HOOK_SHELLS = ("bash", "zsh")
USER_STARTUP_FILES = (
    ".zshrc",
    ".zprofile",
    ".zshenv",
    ".zlogin",
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".profile",
)


def main() -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(line_buffering=True)

    args = parse_args()
    target = current_release_target()
    asset_name = f"rr-{target}"
    asset_url = resolve_asset_url(args, asset_name)
    checksum_url = args.checksum_url or f"{asset_url}.sha256"
    validate_download_urls(
        asset_url=asset_url,
        checksum_url=None if args.sha256 else checksum_url,
        has_pinned_sha256=bool(args.sha256),
        allow_insecure_download=args.allow_insecure_download,
    )
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

    install_binary(data, install_path, requested_version=args.version)

    if args.swarm_key_source or args.swarm_key_b64:
        install_swarm_key(install_path, args)

    if init_requested(args):
        run_init(install_path, args)

    if args.install_hook:
        install_shell_hook(install_path, args)

    if args.import_hishtory:
        run_import_hishtory(install_path, args)
        if not args.keep_hishtory_hooks:
            remove_hishtory_hooks()

    if args.install_daemon:
        install_daemon_service(install_path, args)

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
    parser.add_argument(
        "--allow-insecure-download",
        action="store_true",
        help="Allow non-HTTPS asset/checksum URLs for a trusted private mirror",
    )
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
    parser.add_argument(
        "--swarm-key-source",
        help="Existing shared private swarm key file to copy into the Rustory config directory",
    )
    parser.add_argument(
        "--swarm-key-b64",
        help="Base64-encoded shared private swarm key to write into the Rustory config directory",
    )
    parser.add_argument(
        "--swarm-key-dest",
        default="~/.config/rustory/swarm.key",
        help="Destination path for --swarm-key-source or --swarm-key-b64",
    )
    parser.add_argument("--user-id", help="Logical Rustory user id to write via rr init")
    parser.add_argument("--device-id", help="Device id to write via rr init")
    parser.add_argument(
        "--require-device-membership",
        action="store_true",
        help="Require authoritative enrolled-device membership for P2P sync",
    )
    parser.add_argument(
        "--allow-remote-retirement",
        action="store_true",
        help="Opt this managed daemon into cooperative remote full uninstall",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Pass --force to rr init and replace an existing differing --swarm-key-dest",
    )
    parser.add_argument("--install-hook", action="store_true", help="Install or update a managed Rustory block in the user's shell rc file")
    parser.add_argument(
        "--hook-shell",
        choices=("auto", *SUPPORTED_HOOK_SHELLS),
        default="auto",
        help="Shell rc file to update for --install-hook",
    )
    parser.add_argument("--rc-file", help="Override shell rc file path for --install-hook and daemon shell autostart")
    parser.add_argument(
        "--import-hishtory",
        action="store_true",
        help="Import ~/.hishtory/.hishtory.db after install when present",
    )
    parser.add_argument("--hishtory-path", help="Hishtory SQLite DB path for --import-hishtory")
    parser.add_argument("--hishtory-limit", help="Maximum newest Hishtory rows to import")
    parser.add_argument(
        "--keep-hishtory-hooks",
        action="store_true",
        help="Do not remove Hishtory hook lines after --import-hishtory",
    )
    parser.add_argument(
        "--install-daemon",
        action="store_true",
        help="Install and start a user-level rr daemon service after install",
    )
    parser.add_argument(
        "--no-start-daemon",
        action="store_true",
        help="Write the daemon service file but do not load/start it",
    )
    parser.add_argument(
        "--no-daemon-shell-autostart",
        action="store_true",
        help="Do not install a shell-start fallback block when Linux systemd user bus is unavailable",
    )
    parser.add_argument(
        "--daemon-interval-sec",
        type=positive_int,
        default=60,
        help="rr daemon sync interval for --install-daemon",
    )
    parser.add_argument(
        "--daemon-start-jitter-sec",
        type=non_negative_int,
        default=10,
        help="rr daemon startup jitter for --install-daemon",
    )
    args = parser.parse_args()
    if args.asset_base_url and args.asset_url:
        parser.error("pass only one of --asset-base-url or --asset-url")
    if args.swarm_key_source and args.swarm_key_b64:
        parser.error("pass only one of --swarm-key-source or --swarm-key-b64")
    if args.token and token_has_literal_quote_wrapper(args.token):
        parser.error(
            "--token appears to include literal quote characters; pass the raw token value, "
            'for example --token "$RUSTORY_TRACKER_TOKEN"'
        )
    if args.allow_remote_retirement and not args.require_device_membership:
        parser.error("--allow-remote-retirement requires --require-device-membership")
    if args.allow_remote_retirement and len(split_tracker_values(args.trackers)) != 1:
        parser.error("--allow-remote-retirement requires exactly one --tracker")
    return args


def positive_int(value: str) -> int:
    parsed = int(value)
    if parsed < 1:
        raise argparse.ArgumentTypeError("must be >= 1")
    return parsed


def non_negative_int(value: str) -> int:
    parsed = int(value)
    if parsed < 0:
        raise argparse.ArgumentTypeError("must be >= 0")
    return parsed


def token_has_literal_quote_wrapper(value: str) -> bool:
    token = value.strip()
    return len(token) >= 2 and (
        (token.startswith("'") and token.endswith("'"))
        or (token.startswith('"') and token.endswith('"'))
    )


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


def validate_download_urls(
    *,
    asset_url: str,
    checksum_url: str | None,
    has_pinned_sha256: bool,
    allow_insecure_download: bool,
) -> None:
    if allow_insecure_download:
        return

    checksum_is_trusted = bool(checksum_url and is_trusted_download_url(checksum_url))
    if not is_trusted_download_url(asset_url) and not has_pinned_sha256 and not checksum_is_trusted:
        raise SystemExit(
            "refusing insecure release asset URL; use HTTPS, localhost HTTP, --sha256, "
            "or --allow-insecure-download for a trusted private mirror"
        )

    if checksum_url and not is_trusted_download_url(checksum_url):
        raise SystemExit(
            "refusing insecure checksum URL; use HTTPS, localhost HTTP, --sha256, "
            "or --allow-insecure-download for a trusted private mirror"
        )


def is_trusted_download_url(raw: str) -> bool:
    parsed = urllib.parse.urlparse(raw.strip())
    if parsed.scheme == "https":
        return True
    if parsed.scheme != "http":
        return False
    hostname = parsed.hostname or ""
    if hostname.lower() == "localhost":
        return True
    try:
        return ipaddress.ip_address(hostname).is_loopback
    except ValueError:
        return False


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


def install_binary(data: bytes, install_path: Path, requested_version: str | None = None) -> None:
    install_path.parent.mkdir(parents=True, exist_ok=True)
    try:
        if install_path.exists() and install_path.read_bytes() == data:
            verify_binary(install_path, requested_version=requested_version)
            print(f"binary=unchanged path={install_path}")
            return
    except OSError:
        pass

    fd, tmp_name = tempfile.mkstemp(prefix=f".{install_path.name}.", dir=str(install_path.parent))
    tmp_path = Path(tmp_name)
    try:
        with os.fdopen(fd, "wb") as file:
            file.write(data)
            file.flush()
            os.fsync(file.fileno())
        tmp_path.chmod(stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR | stat.S_IRGRP | stat.S_IXGRP | stat.S_IROTH | stat.S_IXOTH)
        verify_binary(tmp_path, requested_version=requested_version)
        os.replace(tmp_path, install_path)
        fsync_directory(install_path.parent)
        print(f"binary=updated path={install_path}")
    finally:
        if tmp_path.exists():
            tmp_path.unlink()


def verify_binary(install_path: Path, requested_version: str | None = None) -> None:
    try:
        output = subprocess.run(
            [str(install_path), "version"],
            check=True,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=15,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise SystemExit(f"binary_check=failed path={install_path} detail={exc}") from exc
    except subprocess.CalledProcessError as exc:
        print(f"binary_check=failed exit_code={exc.returncode}", file=sys.stderr)
        if exc.stdout:
            print("binary_check_stdout:", file=sys.stderr)
            print(exc.stdout.rstrip(), file=sys.stderr)
        if exc.stderr:
            print("binary_check_stderr:", file=sys.stderr)
            print(exc.stderr.rstrip(), file=sys.stderr)
        raise SystemExit(exc.returncode) from exc

    first_line = output.stdout.splitlines()[0] if output.stdout.splitlines() else "version output unavailable"
    expected_version = pinned_semver(requested_version)
    if expected_version is not None:
        actual_version = parse_rr_version_output(output.stdout)
        if actual_version != expected_version:
            raise SystemExit(
                "binary_check=failed reason=version_mismatch "
                f"expected={expected_version} actual={actual_version or 'missing'}"
            )
    print(f"binary_check={first_line}")


def pinned_semver(value: str | None) -> str | None:
    if value is None:
        return None
    value = value.strip()
    if value == "latest":
        return None
    normalized = value[1:] if value.startswith("v") else value
    if re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?", normalized) is None:
        return None
    return normalized


def parse_rr_version_output(output: str) -> str | None:
    for line in output.splitlines():
        if line.startswith("version:"):
            version = line.removeprefix("version:").strip()
            return version or None
    return None


def fsync_directory(path: Path) -> None:
    try:
        fd = os.open(path, os.O_RDONLY)
    except OSError:
        return
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def install_swarm_key(install_path: Path, args: argparse.Namespace) -> None:
    dest_path = Path(args.swarm_key_dest).expanduser()
    data = read_swarm_key_data(args)

    dest_path.parent.mkdir(parents=True, exist_ok=True)
    try:
        os.chmod(dest_path.parent, 0o700)
    except OSError:
        pass

    if dest_path.exists():
        existing = dest_path.read_bytes()
        if existing == data:
            print(f"swarm_key=installed path={dest_path} status=unchanged")
            print_swarm_key_fingerprint(install_path, dest_path)
            return
        if not args.force:
            raise SystemExit(
                f"swarm_key=failed reason=destination_differs path={dest_path} "
                "hint=rerun_with_--force_to_replace"
            )
        backup_path = next_backup_path(dest_path)
        os.replace(dest_path, backup_path)
        os.chmod(backup_path, 0o600)
        print(f"swarm_key=backup path={backup_path}")

    fd, tmp_name = tempfile.mkstemp(prefix=f".{dest_path.name}.", dir=str(dest_path.parent))
    tmp_path = Path(tmp_name)
    try:
        with os.fdopen(fd, "wb") as file:
            file.write(data)
            file.flush()
            os.fsync(file.fileno())
        os.chmod(tmp_path, 0o600)
        os.replace(tmp_path, dest_path)
    finally:
        if tmp_path.exists():
            tmp_path.unlink()

    print(f"swarm_key=installed path={dest_path} status=updated")
    print_swarm_key_fingerprint(install_path, dest_path)


def read_swarm_key_data(args: argparse.Namespace) -> bytes:
    if args.swarm_key_source:
        source_path = Path(args.swarm_key_source).expanduser()
        if not source_path.exists() or not source_path.is_file():
            raise SystemExit(f"swarm_key=failed reason=missing_source path={source_path}")
        data = source_path.read_bytes()
        if not data.strip():
            raise SystemExit(f"swarm_key=failed reason=empty_source path={source_path}")
        return data

    encoded = "".join(str(args.swarm_key_b64 or "").split())
    if not encoded:
        raise SystemExit("swarm_key=failed reason=empty_base64")
    padded = encoded + ("=" * (-len(encoded) % 4))
    try:
        data = base64.b64decode(padded.encode("ascii"), altchars=b"-_", validate=True)
    except (UnicodeEncodeError, binascii.Error):
        raise SystemExit("swarm_key=failed reason=invalid_base64") from None
    if not data.strip():
        raise SystemExit("swarm_key=failed reason=decoded_empty")
    return data


def next_backup_path(path: Path) -> Path:
    suffix = int(time.time())
    candidate = path.with_name(f"{path.name}.bak.{suffix}")
    if not candidate.exists():
        return candidate
    for index in range(1, 1000):
        candidate = path.with_name(f"{path.name}.bak.{suffix}.{index}")
        if not candidate.exists():
            return candidate
    raise SystemExit(f"swarm_key=failed reason=backup_path_exhausted path={path}")


def print_swarm_key_fingerprint(install_path: Path, swarm_key_path: Path) -> None:
    output = subprocess.run(
        [str(install_path), "swarm-key", "--swarm-key", str(swarm_key_path)],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    for line in output.stdout.splitlines():
        if "fingerprint:" in line:
            fingerprint = line.split("fingerprint:", 1)[1].strip()
            print(f"swarm_key_fingerprint={fingerprint}")
            return
    print("swarm_key_fingerprint=unknown")


def split_tracker_values(values: list[str]) -> list[str]:
    trackers: list[str] = []
    for value in values:
        trackers.extend(item.strip() for item in value.split(",") if item.strip())
    return trackers


def init_requested(args: argparse.Namespace) -> bool:
    return any(
        (
            args.token,
            args.trackers,
            args.relay,
            args.user_id,
            args.device_id,
            args.require_device_membership,
            args.allow_remote_retirement,
        )
    )


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
    if args.require_device_membership:
        cmd.append("--require-device-membership")
    if args.allow_remote_retirement:
        cmd.append("--allow-remote-retirement")
    if (args.require_device_membership or args.allow_remote_retirement) and not args.force:
        cmd.append("--update-existing-security")

    if args.relay:
        warning = relay_addr_reachability_warning(args.relay)
        if warning:
            print(f"relay_warning={warning}")

    print("init=running token_configured={} trackers={}".format(bool(args.token), len(split_tracker_values(args.trackers))))
    subprocess.run(cmd, check=True)


def relay_addr_reachability_warning(relay: str) -> str | None:
    protocols = relay.strip().split("/")
    for idx, proto in enumerate(protocols[:-1]):
        if proto not in ("ip4", "ip6"):
            continue
        raw = protocols[idx + 1]
        try:
            ip = ipaddress.ip_address(raw)
        except ValueError:
            return f"invalid_ip_in_relay_addr value={raw}"
        if isinstance(ip, ipaddress.IPv4Address) and ip in ipaddress.ip_network("100.64.0.0/10"):
            return "relay_addr_uses_100.64.0.0/10_shared_space; peers_outside_that_tailnet_or_cgnat_path_cannot_dial_it"
        if ip.is_loopback:
            return "relay_addr_is_loopback; only_local_processes_can_dial_it"
        if ip.is_private:
            return "relay_addr_is_private; internet_peers_cannot_dial_it_without_vpn_or_port_forwarding"
        if ip.is_link_local:
            return "relay_addr_is_link_local; peers_off_link_cannot_dial_it"
        if ip.is_multicast or ip.is_unspecified:
            return "relay_addr_is_not_dialable_from_other_hosts"
        return None
    return None


def install_daemon_service(install_path: Path, args: argparse.Namespace) -> None:
    system = platform.system().lower()
    state_home = rustory_state_home()
    record_managed_state_home(state_home)
    daemon_args = [
        str(install_path),
        "daemon",
        "--interval-sec",
        str(args.daemon_interval_sec),
        "--start-jitter-sec",
        str(args.daemon_start_jitter_sec),
    ]
    if system == "darwin":
        install_launchd_daemon(daemon_args, state_home, not args.no_start_daemon)
        return
    if system == "linux":
        install_systemd_user_daemon(
            daemon_args,
            state_home,
            not args.no_start_daemon,
            args,
        )
        return
    raise SystemExit(f"daemon=failed reason=unsupported_platform platform={platform.system()}")


def install_launchd_daemon(
    daemon_args: list[str], state_home: Path, start: bool
) -> None:
    label = "com.rustory.daemon"
    plist_path = Path.home() / "Library" / "LaunchAgents" / f"{label}.plist"
    log_dir = Path.home() / "Library" / "Logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    plist_path.parent.mkdir(parents=True, exist_ok=True)
    plist_path.write_text(
        render_launchd_plist(label, daemon_args, log_dir, state_home),
        encoding="utf-8",
    )
    os.chmod(plist_path, 0o644)
    print(f"daemon=installed manager=launchd plist={plist_path}")

    if not start:
        print("daemon=start_skipped reason=--no-start-daemon")
        return

    target = f"gui/{os.getuid()}"
    subprocess.run(["launchctl", "bootout", target, str(plist_path)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    subprocess.run(["launchctl", "bootstrap", target, str(plist_path)], check=True)
    subprocess.run(["launchctl", "enable", f"{target}/{label}"], check=True)
    subprocess.run(["launchctl", "kickstart", "-k", f"{target}/{label}"], check=True)
    print(f"daemon=started manager=launchd label={label}")


def render_launchd_plist(
    label: str, daemon_args: list[str], log_dir: Path, state_home: Path
) -> str:
    arg_lines = "\n".join(f"    <string>{html.escape(arg)}</string>" for arg in daemon_args)
    stdout_path = html.escape(str(log_dir / "rustory-daemon.out.log"))
    stderr_path = html.escape(str(log_dir / "rustory-daemon.err.log"))
    path_value = html.escape(os.environ.get("PATH", ""))
    state_home_value = html.escape(str(state_home))
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{html.escape(label)}</string>
  <key>ProgramArguments</key>
  <array>
{arg_lines}
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{stdout_path}</string>
  <key>StandardErrorPath</key>
  <string>{stderr_path}</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>{path_value}</string>
    <key>RUSTORY_DAEMON_MANAGER</key>
    <string>launchd</string>
    <key>XDG_STATE_HOME</key>
    <string>{state_home_value}</string>
  </dict>
</dict>
</plist>
"""


def install_systemd_user_daemon(
    daemon_args: list[str],
    state_home: Path,
    start: bool,
    args: argparse.Namespace,
) -> None:
    unit_path = Path.home() / ".config" / "systemd" / "user" / "rustory.service"
    unit_path.parent.mkdir(parents=True, exist_ok=True)
    unit_path.write_text(
        render_systemd_user_unit(daemon_args, state_home), encoding="utf-8"
    )
    os.chmod(unit_path, 0o644)
    print(f"daemon=installed manager=systemd-user unit={unit_path}")

    if start:
        for step in (["daemon-reload"], ["enable", "rustory.service"], ["restart", "rustory.service"]):
            try:
                run_systemd_user(step)
            except subprocess.CalledProcessError as exc:
                if systemd_user_bus_unavailable(exc):
                    print_systemd_user_start_deferred(step[0], exc)
                    start_background_daemon(
                        daemon_args, state_home, restart=True
                    )
                    install_background_daemon_autostart(daemon_args, state_home, args)
                    return
                print_systemd_user_failure(step[0], exc)
                raise SystemExit(exc.returncode) from exc
        print("daemon=started manager=systemd-user unit=rustory.service")
        return

    print("daemon=start_skipped reason=--no-start-daemon")


def run_systemd_user(args: list[str]) -> None:
    subprocess.run(
        ["systemctl", "--user", *args],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def systemd_user_bus_unavailable(exc: subprocess.CalledProcessError) -> bool:
    text = f"{exc.stdout or ''}\n{exc.stderr or ''}\n{exc}".lower()
    return (
        "failed to connect to bus" in text
        or "dbus_session_bus_address" in text
        or "xdg_runtime_dir" in text
        or "no medium found" in text
    )


def print_systemd_user_start_deferred(step: str, exc: subprocess.CalledProcessError) -> None:
    detail = one_line_process_output(exc)
    print(
        "daemon=start_deferred manager=systemd-user unit=rustory.service "
        f"reason=user_bus_unavailable step={step}"
    )
    if detail:
        print(f"daemon=start_deferred_detail {detail}")
    print("daemon=start_hint command=systemctl --user daemon-reload")
    print("daemon=start_hint command=systemctl --user enable --now rustory.service")
    print("daemon=start_hint command=systemctl --user status rustory.service")
    print("daemon=start_hint linger=loginctl enable-linger <user>")
    print("daemon=start_hint fallback=background process started because systemd user bus is unavailable")
    print("daemon=start_hint fallback_autostart=shell rc block will restart it on the next interactive shell")


def print_systemd_user_failure(step: str, exc: subprocess.CalledProcessError) -> None:
    detail = one_line_process_output(exc)
    print(
        "daemon=failed manager=systemd-user unit=rustory.service "
        f"step={step} exit_code={exc.returncode}",
        file=sys.stderr,
    )
    if detail:
        print(f"daemon=failed_detail {detail}", file=sys.stderr)


def one_line_process_output(exc: subprocess.CalledProcessError) -> str:
    return " ".join((exc.stderr or exc.stdout or str(exc)).split())


def start_background_daemon(
    daemon_args: list[str], state_home: Path, restart: bool = False
) -> None:
    state_dir = state_home / "rustory"
    state_dir.mkdir(parents=True, exist_ok=True)
    pid_path = state_dir / "daemon.pid"
    log_path = state_dir / "daemon.log"

    existing_pid = validated_background_pid(pid_path, Path(daemon_args[0]))
    if existing_pid is not None:
        if not restart:
            print(f"daemon=started manager=background status=already_running pid={existing_pid} log={log_path}")
            return
        print(f"daemon=stopping manager=background pid={existing_pid}")
        stop_background_daemon(existing_pid, Path(daemon_args[0]))
    if restart:
        stopped = stop_stale_background_daemon_processes(Path(daemon_args[0]))
        if stopped:
            print(f"daemon=stale_processes_stopped manager=background count={stopped}")

    with log_path.open("ab") as log_file:
        proc = subprocess.Popen(
            daemon_args,
            cwd=str(Path.home()),
            stdin=subprocess.DEVNULL,
            stdout=log_file,
            stderr=subprocess.STDOUT,
            start_new_session=True,
            close_fds=True,
            env={
                **os.environ,
                "XDG_STATE_HOME": str(state_home),
                "RUSTORY_DAEMON_MANAGER": "background",
            },
        )

    pid_path.write_text(f"{proc.pid}\n", encoding="utf-8")
    os.chmod(pid_path, 0o600)
    time.sleep(0.2)
    exit_code = proc.poll()
    if exit_code is not None:
        detail = tail_file_one_line(log_path)
        print(
            f"daemon=failed manager=background exit_code={exit_code} log={log_path}",
            file=sys.stderr,
        )
        if detail:
            print(f"daemon=failed_detail {detail}", file=sys.stderr)
        raise SystemExit(exit_code)

    action = "restarted" if restart else "started"
    print(f"daemon={action} manager=background pid={proc.pid} log={log_path}")
    print("daemon=start_note manager=background persistence=until_process_exit_or_reboot")


def stop_background_daemon(pid: int, install_path: Path) -> None:
    signal_background_process(pid, signal.SIGTERM)

    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if not background_pid_matches_install(pid, install_path):
            return
        time.sleep(0.1)

    if not background_pid_matches_install(pid, install_path):
        return
    signal_background_process(pid, signal.SIGKILL)
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        if not background_pid_matches_install(pid, install_path):
            return
        time.sleep(0.1)

    raise SystemExit(f"daemon=failed manager=background step=stop pid={pid} detail=timeout_after_sigterm_sigkill")


def signal_background_process(pid: int, sig: signal.Signals) -> None:
    try:
        os.killpg(pid, sig)
        return
    except ProcessLookupError:
        pass
    except PermissionError:
        pass

    try:
        os.kill(pid, sig)
    except ProcessLookupError:
        return
    except PermissionError as exc:
        raise SystemExit(f"daemon=failed manager=background step=stop pid={pid} detail={exc}") from exc


def stop_stale_background_daemon_processes(install_path: Path) -> int:
    if sys.platform != "linux":
        return 0
    proc_dir = Path("/proc")
    if not proc_dir.exists():
        return 0

    current_pid = os.getpid()
    child_targets: list[int] = []
    daemon_targets: list[int] = []
    for child in proc_dir.iterdir():
        if not child.name.isdigit():
            continue
        pid = int(child.name)
        if pid == current_pid or not proc_is_current_user(pid):
            continue
        cmdline = read_proc_cmdline(pid)
        if not proc_exe_matches(pid, install_path) or not proc_has_background_manager(pid):
            continue
        kind = managed_background_cmdline_kind(cmdline)
        if kind == "daemon":
            daemon_targets.append(pid)
        elif kind == "child" and process_has_managed_daemon_ancestor(pid, install_path):
            child_targets.append(pid)

    stopped = 0
    targets = sorted(set(child_targets)) + sorted(set(daemon_targets))
    for pid in targets:
        if not managed_process_matches_install(pid, install_path):
            continue
        signal_background_process(pid, signal.SIGTERM)
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline and managed_process_matches_install(pid, install_path):
            time.sleep(0.1)
        if managed_process_matches_install(pid, install_path):
            try:
                signal_background_process(pid, signal.SIGKILL)
            except SystemExit:
                pass
        stopped += 1
    return stopped


def proc_is_current_user(pid: int) -> bool:
    try:
        status = Path(f"/proc/{pid}/status").read_text(encoding="utf-8")
    except OSError:
        return False
    for line in status.splitlines():
        if line.startswith("Uid:"):
            fields = line.split()
            return len(fields) > 1 and fields[1].isdigit() and int(fields[1]) == os.getuid()
    return False


def read_proc_cmdline(pid: int) -> list[str]:
    try:
        raw = Path(f"/proc/{pid}/cmdline").read_bytes()
    except OSError:
        return []
    return [part.decode("utf-8", "replace") for part in raw.split(b"\0") if part]


def proc_exe_matches(pid: int, install_path: Path) -> bool:
    try:
        exe = Path(os.readlink(f"/proc/{pid}/exe"))
    except OSError:
        return False
    return paths_match_after_deleted_suffix(exe, install_path)


def proc_has_background_manager(pid: int) -> bool:
    try:
        environ = Path(f"/proc/{pid}/environ").read_bytes()
    except OSError:
        return False
    return b"RUSTORY_DAEMON_MANAGER=background" in environ.split(b"\0")


def cmdline_has_default_rustory_db_path(cmdline: list[str]) -> bool:
    for idx, arg in enumerate(cmdline[:-1]):
        if arg == "--db-path" and is_default_rustory_db_path(cmdline[idx + 1]):
            return True
    return False


def is_default_rustory_db_path(path: str) -> bool:
    return (
        path == "~/.rustory/history.db"
        or path == "$HOME/.rustory/history.db"
        or path.endswith("/.rustory/history.db")
    )


def paths_match_after_deleted_suffix(left: Path, right: Path) -> bool:
    return strip_deleted_suffix(str(left)) == strip_deleted_suffix(str(right))


def strip_deleted_suffix(value: str) -> str:
    return value[:-10] if value.endswith(" (deleted)") else value


def managed_background_cmdline_kind(cmdline: list[str]) -> str | None:
    if "daemon" in cmdline:
        if "--interval-sec" in cmdline and "--start-jitter-sec" in cmdline:
            return "daemon"
        return None
    if "p2p-sync" in cmdline:
        if "--watch" in cmdline and cmdline_has_default_rustory_db_path(cmdline):
            return "child"
        return None
    if "p2p-serve" in cmdline:
        if cmdline_has_default_rustory_db_path(cmdline):
            return "child"
        return None
    return None


def is_managed_background_cmdline(
    cmdline: list[str], has_managed_daemon_ancestor: bool = False
) -> bool:
    kind = managed_background_cmdline_kind(cmdline)
    return kind == "daemon" or (kind == "child" and has_managed_daemon_ancestor)


def install_background_daemon_autostart(
    daemon_args: list[str], state_home: Path, args: argparse.Namespace
) -> None:
    if args.no_daemon_shell_autostart:
        print("daemon=autostart_skipped manager=background reason=--no-daemon-shell-autostart")
        return

    shell = resolve_hook_shell(args.hook_shell)
    rc_file = Path(args.rc_file).expanduser() if args.rc_file else default_rc_file(shell)
    block = render_daemon_autostart_block(daemon_args, state_home)
    record_managed_rc_file(rc_file)
    update_managed_block(rc_file, block, DAEMON_AUTOSTART_START, DAEMON_AUTOSTART_END)
    print(f"daemon=autostart_installed manager=background shell={shell} rc_file={rc_file}")


def render_daemon_autostart_block(
    daemon_args: list[str], state_home: Path
) -> str:
    daemon_command = " ".join(shell_quote_arg(arg) for arg in daemon_args)
    state_home_command = shell_quote_arg(str(state_home))
    state_dir_command = shell_quote_arg(str(state_home / "rustory"))
    return "\n".join(
        [
            DAEMON_AUTOSTART_START,
            "# Managed by rustory installer. Re-run with --install-daemon to update.",
            "case $- in",
            "  *i*)",
            f"    __rustory_daemon_state_dir={state_dir_command}",
            '    __rustory_daemon_pid_file="$__rustory_daemon_state_dir/daemon.pid"',
            '    __rustory_daemon_log_file="$__rustory_daemon_state_dir/daemon.log"',
            "    __rustory_daemon_running=0",
            "    if command -v systemctl >/dev/null 2>&1 && systemctl --user is-active --quiet rustory.service >/dev/null 2>&1; then",
            "      __rustory_daemon_running=1",
            '    elif [ -r "$__rustory_daemon_pid_file" ]; then',
            '      __rustory_daemon_pid="$(cat "$__rustory_daemon_pid_file" 2>/dev/null)"',
            '      case "$__rustory_daemon_pid" in',
            "        ''|*[!0-9]*) __rustory_daemon_running=0 ;;",
            '        *) kill -0 "$__rustory_daemon_pid" >/dev/null 2>&1 && __rustory_daemon_running=1 ;;',
            "      esac",
            "    fi",
            '    if [ "$__rustory_daemon_running" != "1" ]; then',
            '      mkdir -p "$__rustory_daemon_state_dir"',
            "      if command -v setsid >/dev/null 2>&1; then",
            f'        XDG_STATE_HOME={state_home_command} RUSTORY_DAEMON_MANAGER=background setsid {daemon_command} >> "$__rustory_daemon_log_file" 2>&1 </dev/null &',
            "      else",
            f'        XDG_STATE_HOME={state_home_command} RUSTORY_DAEMON_MANAGER=background nohup {daemon_command} >> "$__rustory_daemon_log_file" 2>&1 </dev/null &',
            "      fi",
            '      echo $! > "$__rustory_daemon_pid_file"',
            '      chmod 600 "$__rustory_daemon_pid_file" "$__rustory_daemon_log_file" 2>/dev/null || true',
            "    fi",
            "    unset __rustory_daemon_state_dir __rustory_daemon_pid_file __rustory_daemon_log_file",
            "    unset __rustory_daemon_running __rustory_daemon_pid",
            "    ;;",
            "esac",
            DAEMON_AUTOSTART_END,
            "",
        ]
    )


def shell_quote_arg(value: str) -> str:
    if value and all(ch.isalnum() or ch in "/._:=@+-" for ch in value):
        return value
    return "'" + value.replace("'", "'\"'\"'") + "'"


def rustory_state_home() -> Path:
    base = os.environ.get("XDG_STATE_HOME")
    if base:
        base_path = Path(base).expanduser()
        if not base_path.is_absolute():
            raise SystemExit("XDG_STATE_HOME must be absolute")
        return base_path
    return Path.home() / ".local" / "state"


def rustory_state_dir() -> Path:
    return rustory_state_home() / "rustory"


def managed_rc_state_path() -> Path:
    return Path.home() / ".config" / "rustory" / "managed-rc-files.json"


def managed_state_home_path() -> Path:
    return Path.home() / ".config" / "rustory" / MANAGED_STATE_HOME_FILE


def managed_state_homes_path() -> Path:
    return Path.home() / ".config" / "rustory" / MANAGED_STATE_HOMES_FILE


def record_managed_state_home(state_home: Path) -> None:
    if not state_home.is_absolute():
        raise SystemExit(f"managed state home must be absolute: {state_home}")
    state_home_text = str(state_home)
    if any(ord(ch) < 32 or ord(ch) == 127 for ch in state_home_text):
        raise SystemExit("managed state home must not contain control characters")
    if len(state_home_text.encode("utf-8")) > 4095:
        raise SystemExit("managed state home path is too long")
    state_path = managed_state_home_path()
    state_dir = state_path.parent
    state_dir.mkdir(parents=True, exist_ok=True)
    if state_dir.is_symlink() or not state_dir.is_dir():
        raise SystemExit(f"managed state dir must be a regular directory: {state_dir}")
    os.chmod(state_dir, 0o700)
    try:
        metadata = state_path.lstat()
    except FileNotFoundError:
        metadata = None
    if metadata is not None:
        if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
            raise SystemExit(f"managed state home must be a regular non-symlink file: {state_path}")
        if stat.S_IMODE(metadata.st_mode) & 0o077:
            raise SystemExit(f"managed state home permissions are too broad: {state_path}")
        if metadata.st_size > 4096:
            raise SystemExit(f"managed state home file is too large: {state_path}")

    payload = f"{state_home_text}\n"
    fd, tmp_name = tempfile.mkstemp(prefix=".managed-state-home.", dir=str(state_dir))
    tmp_path = Path(tmp_name)
    try:
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8", newline="") as file:
            file.write(payload)
            file.flush()
            os.fsync(file.fileno())
        os.replace(tmp_path, state_path)
        os.chmod(state_path, 0o600)
        fsync_directory(state_dir)
    finally:
        if tmp_path.exists():
            tmp_path.unlink()
    record_managed_state_home_history(state_home)


def record_managed_state_home_history(state_home: Path) -> None:
    state_path = managed_state_homes_path()
    state_dir = state_path.parent
    flags = os.O_RDWR | os.O_CREAT
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    lock_path = state_dir / MANAGED_STATE_HOMES_LOCK_FILE
    try:
        lock_fd = os.open(lock_path, flags, 0o600)
    except OSError as exc:
        raise SystemExit(f"open managed state homes lock: {lock_path}: {exc}") from exc
    try:
        lock_metadata = os.fstat(lock_fd)
        if not stat.S_ISREG(lock_metadata.st_mode):
            raise SystemExit(f"managed state homes lock must be a regular file: {lock_path}")
        os.fchmod(lock_fd, 0o600)
        fcntl.flock(lock_fd, fcntl.LOCK_EX)
        record_managed_state_home_history_locked(state_home, state_path, state_dir)
    finally:
        os.close(lock_fd)


def record_managed_state_home_history_locked(
    state_home: Path, state_path: Path, state_dir: Path
) -> None:
    paths: list[str] = []
    try:
        metadata = state_path.lstat()
    except FileNotFoundError:
        metadata = None
    if metadata is not None:
        if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
            raise SystemExit(f"managed state homes must be a regular non-symlink file: {state_path}")
        if stat.S_IMODE(metadata.st_mode) & 0o077:
            raise SystemExit(f"managed state homes permissions are too broad: {state_path}")
        if metadata.st_size > 64 * 1024:
            raise SystemExit(f"managed state homes file is too large: {state_path}")
        try:
            existing = json.loads(state_path.read_text(encoding="utf-8"))
            if existing.get("version") != 1 or not isinstance(existing.get("paths"), list):
                raise ValueError("unsupported schema")
            for value in existing["paths"]:
                if (
                    not isinstance(value, str)
                    or not Path(value).is_absolute()
                    or len(value.encode("utf-8")) > 4095
                    or any(ord(ch) < 32 or ord(ch) == 127 for ch in value)
                ):
                    raise ValueError("managed state homes must be absolute safe strings")
                if value not in paths:
                    paths.append(value)
        except (OSError, ValueError, TypeError, json.JSONDecodeError) as exc:
            raise SystemExit(f"invalid managed state homes: {state_path}: {exc}") from exc

    value = str(state_home)
    if value not in paths:
        paths.append(value)
    if len(paths) > MAX_MANAGED_STATE_HOMES:
        raise SystemExit("too many managed state homes; uninstall old Rustory state before changing it again")
    payload = json.dumps({"version": 1, "paths": paths}, indent=2) + "\n"
    if len(payload.encode("utf-8")) > 64 * 1024:
        raise SystemExit("managed state homes payload is too large")
    fd, tmp_name = tempfile.mkstemp(prefix=".managed-state-homes.", dir=str(state_dir))
    tmp_path = Path(tmp_name)
    try:
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8", newline="") as file:
            file.write(payload)
            file.flush()
            os.fsync(file.fileno())
        os.replace(tmp_path, state_path)
        os.chmod(state_path, 0o600)
        fsync_directory(state_dir)
    finally:
        if tmp_path.exists():
            tmp_path.unlink()


def record_managed_rc_file(rc_file: Path) -> None:
    state_dir = managed_rc_state_path().parent
    state_dir.mkdir(parents=True, exist_ok=True)
    if state_dir.is_symlink() or not state_dir.is_dir():
        raise SystemExit(f"managed rc state dir must be a regular directory: {state_dir}")
    os.chmod(state_dir, 0o700)
    flags = os.O_RDWR | os.O_CREAT
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    lock_path = state_dir / MANAGED_RC_LOCK_FILE
    try:
        lock_fd = os.open(lock_path, flags, 0o600)
    except OSError as exc:
        raise SystemExit(f"open managed rc state lock: {lock_path}: {exc}") from exc
    try:
        lock_metadata = os.fstat(lock_fd)
        if not stat.S_ISREG(lock_metadata.st_mode):
            raise SystemExit(f"managed rc state lock must be a regular file: {lock_path}")
        os.fchmod(lock_fd, 0o600)
        fcntl.flock(lock_fd, fcntl.LOCK_EX)
        record_managed_rc_file_locked(rc_file)
    finally:
        os.close(lock_fd)


def record_managed_rc_file_locked(rc_file: Path) -> None:
    state_path = managed_rc_state_path()
    state_dir = state_path.parent
    state_dir.mkdir(parents=True, exist_ok=True)
    if state_dir.is_symlink() or not state_dir.is_dir():
        raise SystemExit(f"managed rc state dir must be a regular directory: {state_dir}")
    os.chmod(state_dir, 0o700)
    paths: list[str] = []
    try:
        metadata = state_path.lstat()
    except FileNotFoundError:
        metadata = None
    if metadata is not None:
        if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
            raise SystemExit(f"managed rc state must be a regular non-symlink file: {state_path}")
        if stat.S_IMODE(metadata.st_mode) & 0o077:
            raise SystemExit(f"managed rc state permissions are too broad: {state_path}")
        if metadata.st_size > 64 * 1024:
            raise SystemExit(f"managed rc state is too large: {state_path}")
        try:
            existing = json.loads(state_path.read_text(encoding="utf-8"))
            if existing.get("version") != 1 or not isinstance(existing.get("paths"), list):
                raise ValueError("unsupported schema")
            for value in existing["paths"]:
                if not isinstance(value, str) or not Path(value).is_absolute():
                    raise ValueError("managed rc paths must be absolute strings")
                paths.append(value)
        except (OSError, ValueError, TypeError, json.JSONDecodeError) as exc:
            raise SystemExit(f"invalid managed rc state: {state_path}: {exc}") from exc

    managed_path = str(rc_file.resolve(strict=False))
    if managed_path not in paths:
        paths.append(managed_path)
    if len(set(paths)) > 32:
        raise SystemExit("too many managed rc paths")
    payload = json.dumps({"version": 1, "paths": sorted(set(paths))}, indent=2) + "\n"
    if len(payload.encode("utf-8")) > 64 * 1024:
        raise SystemExit("managed rc state payload is too large")
    fd, tmp_name = tempfile.mkstemp(prefix=".managed-rc-files.", dir=str(state_dir))
    tmp_path = Path(tmp_name)
    try:
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8", newline="") as file:
            file.write(payload)
            file.flush()
            os.fsync(file.fileno())
        os.replace(tmp_path, state_path)
        os.chmod(state_path, 0o600)
        fsync_directory(state_dir)
    finally:
        if tmp_path.exists():
            tmp_path.unlink()


def read_pid_file(path: Path) -> int | None:
    try:
        raw = path.read_text(encoding="utf-8").strip()
    except OSError:
        return None
    if not raw:
        return None
    try:
        return int(raw)
    except ValueError:
        return None


def validated_background_pid(pid_path: Path, install_path: Path) -> int | None:
    pid = read_pid_file(pid_path)
    if pid is None or not pid_is_running(pid):
        return None
    if background_pid_matches_install(pid, install_path):
        return pid
    print(
        f"warn: daemon pid file is stale; refusing to signal unrelated pid={pid} path={pid_path}"
    )
    try:
        pid_path.unlink()
    except FileNotFoundError:
        pass
    return None


def background_pid_matches_install(pid: int, install_path: Path) -> bool:
    if sys.platform != "linux" or not proc_is_current_user(pid):
        return False
    cmdline = read_proc_cmdline(pid)
    if managed_background_cmdline_kind(cmdline) != "daemon":
        return False
    # This PID came from the private installer-owned pid file, which is also the
    # migration path for older updater-spawned daemons that lacked the marker.
    return proc_exe_matches(pid, install_path)


def managed_process_matches_install(pid: int, install_path: Path) -> bool:
    if sys.platform != "linux" or not proc_is_current_user(pid):
        return False
    cmdline = read_proc_cmdline(pid)
    if not proc_exe_matches(pid, install_path) or not proc_has_background_manager(pid):
        return False
    kind = managed_background_cmdline_kind(cmdline)
    if kind == "daemon":
        return True
    return kind == "child" and process_has_managed_daemon_ancestor(pid, install_path)


def process_has_managed_daemon_ancestor(pid: int, install_path: Path) -> bool:
    for _ in range(64):
        parent_pid = read_proc_parent_pid(pid)
        if parent_pid is None or parent_pid <= 1 or parent_pid == pid:
            return False
        parent_cmdline = read_proc_cmdline(parent_pid)
        if (
            managed_background_cmdline_kind(parent_cmdline) == "daemon"
            and proc_exe_matches(parent_pid, install_path)
            and proc_has_background_manager(parent_pid)
        ):
            return True
        pid = parent_pid
    return False


def read_proc_parent_pid(pid: int) -> int | None:
    try:
        stat_text = Path(f"/proc/{pid}/stat").read_text(encoding="utf-8")
        after_comm = stat_text.rsplit(") ", 1)[1]
        return int(after_comm.split()[1])
    except (OSError, IndexError, ValueError):
        return None


def pid_is_running(pid: int) -> bool:
    if pid <= 0:
        return False
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def tail_file_one_line(path: Path, max_bytes: int = 4096) -> str:
    try:
        with path.open("rb") as file:
            file.seek(0, os.SEEK_END)
            size = file.tell()
            file.seek(max(0, size - max_bytes), os.SEEK_SET)
            data = file.read().decode("utf-8", errors="replace")
    except OSError:
        return ""
    return " ".join(data.split())


def render_systemd_user_unit(daemon_args: list[str], state_home: Path) -> str:
    exec_start = " ".join(systemd_quote_arg(arg) for arg in daemon_args)
    state_home_environment = systemd_quote_arg(f"XDG_STATE_HOME={state_home}")
    return f"""[Unit]
Description=Rustory daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={exec_start}
Environment=RUSTORY_DAEMON_MANAGER=systemd-user
Environment={state_home_environment}
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
"""


def systemd_quote_arg(value: str) -> str:
    if value and all(ch.isalnum() or ch in "/._:=@+-" for ch in value):
        return value
    return (
        '"'
        + value.replace("%", "%%").replace("\\", "\\\\").replace('"', '\\"')
        + '"'
    )


def run_import_hishtory(install_path: Path, args: argparse.Namespace) -> None:
    source_path = Path(args.hishtory_path).expanduser() if args.hishtory_path else Path.home() / ".hishtory" / ".hishtory.db"
    if not source_path.exists():
        if args.hishtory_path:
            raise SystemExit(f"hishtory import path not found: {source_path}")
        print(f"hishtory_import=skipped reason=missing_default_db path={source_path}")
        return

    cmd = [str(install_path), "import", "--shell", "hishtory", "--path", str(source_path)]
    if args.hishtory_limit:
        cmd += ["--limit", args.hishtory_limit]
    print(f"hishtory_import=running path={source_path}")
    try:
        subprocess.run(cmd, check=True)
    except subprocess.CalledProcessError as exc:
        raise SystemExit(f"hishtory_import=failed exit_code={exc.returncode}") from None


def remove_hishtory_hooks() -> None:
    changed: list[Path] = []
    for rc_file in user_startup_files():
        if cleanup_hishtory_file(rc_file):
            changed.append(rc_file)

    if changed:
        files = ",".join(str(path) for path in changed)
        print(f"hishtory_hooks=removed files={files}")
    else:
        print("hishtory_hooks=removed files=0")


def user_startup_files() -> list[Path]:
    home = Path.home()
    files = [home / name for name in USER_STARTUP_FILES]
    if platform.system().lower() == "darwin":
        files.append(home / ".bash_profile")
    return dedupe_paths(files)


def dedupe_paths(paths: list[Path]) -> list[Path]:
    seen: set[Path] = set()
    result: list[Path] = []
    for path in paths:
        normalized = path.expanduser()
        if normalized in seen:
            continue
        seen.add(normalized)
        result.append(normalized)
    return result


def cleanup_hishtory_file(path: Path) -> bool:
    if not path.exists() or not path.is_file():
        return False

    original = path.read_text()
    lines = original.splitlines()
    cleaned = remove_hishtory_lines(lines)
    if cleaned == lines:
        return False
    if path.is_symlink():
        raise SystemExit(
            f"refusing to rewrite symlinked startup file {path}; edit its target manually"
        )

    had_final_newline = original.endswith("\n")
    text = "\n".join(cleaned)
    if text and had_final_newline:
        text += "\n"
    atomic_write_text(path, text)
    return True


def remove_hishtory_lines(lines: list[str]) -> list[str]:
    cleaned: list[str] = []
    index = 0
    while index < len(lines):
        line = lines[index]
        if is_hishtory_config_header(line) or is_hishtory_line(line):
            index += 1
            continue
        cleaned.append(line)
        index += 1
    return cleaned


def is_hishtory_config_header(line: str) -> bool:
    return line.strip().casefold().rstrip(":") == "# hishtory config"


def is_hishtory_line(line: str) -> bool:
    stripped = line.strip()
    folded = stripped.casefold()
    if not folded or folded.startswith("#"):
        return False

    references_hishtory_path = any(
        marker in folded
        for marker in (".hishtory", "hishtory/config", "hishtory config")
    )
    is_source = folded.startswith("source ") or folded.startswith(". ")
    is_path_assignment = folded.startswith("export path=") or folded.startswith("path=")
    is_eval_hook = folded.startswith("eval ") and (
        "$(hishtory " in folded or "`hishtory " in folded
    )
    words = folded.split()
    is_direct_hook = (
        len(words) >= 2
        and words[0] == "hishtory"
        and words[1] in {"init", "enable", "shell", "daemon"}
    )
    return (
        references_hishtory_path and (is_source or is_path_assignment)
    ) or is_eval_hook or is_direct_hook


def install_shell_hook(install_path: Path, args: argparse.Namespace) -> None:
    shell = resolve_hook_shell(args.hook_shell)
    rc_file = Path(args.rc_file).expanduser() if args.rc_file else default_rc_file(shell)
    block = render_hook_block(shell, install_path.parent)
    record_managed_rc_file(rc_file)
    removed_blocks = update_managed_block(
        rc_file,
        block,
        legacy_marker_pairs=((LEGACY_HOOK_START, LEGACY_HOOK_END),),
    )
    print(f"hook=installed shell={shell} rc_file={rc_file} deduped_blocks={removed_blocks}")


def resolve_hook_shell(value: str) -> str:
    if value != "auto":
        return value

    shell_name = Path(os.environ.get("SHELL", "")).name
    if shell_name in SUPPORTED_HOOK_SHELLS:
        return shell_name

    home = Path.home()
    if (home / ".zshrc").exists():
        return "zsh"
    if (home / ".bashrc").exists():
        return "bash"
    if platform.system().lower() == "darwin":
        return "zsh"
    return "bash"


def default_rc_file(shell: str) -> Path:
    if shell == "zsh":
        return Path.home() / ".zshrc"
    if shell == "bash":
        return Path.home() / ".bashrc"
    raise SystemExit(f"unsupported hook shell: {shell}")


def render_hook_block(shell: str, bin_dir: Path) -> str:
    bin_expr = shell_path_expr(bin_dir)
    return "\n".join(
        [
            HOOK_START,
            "# Managed by rustory installer. Re-run with --install-hook to update.",
            f'export PATH="{bin_expr}:$PATH"',
            "if command -v rr >/dev/null 2>&1; then",
            f"  source <(rr hook --shell {shell})",
            "fi",
            HOOK_END,
            "",
        ]
    )


def shell_path_expr(path: Path) -> str:
    path = path.expanduser()
    home = Path.home()
    try:
        rel = path.relative_to(home)
    except ValueError:
        return str(path).replace("\\", "\\\\").replace('"', '\\"').replace("$", "\\$")
    if str(rel) == ".":
        return "$HOME"
    return "$HOME/" + str(rel).replace("\\", "\\\\").replace('"', '\\"')


def update_managed_block(
    rc_file: Path,
    block: str,
    start_marker: str = HOOK_START,
    end_marker: str = HOOK_END,
    legacy_marker_pairs: tuple[tuple[str, str], ...] = (),
) -> int:
    rc_file.parent.mkdir(parents=True, exist_ok=True)
    existing = rc_file.read_text() if rc_file.exists() else ""
    cleaned, removed_blocks = strip_managed_blocks(
        existing,
        ((start_marker, end_marker), *legacy_marker_pairs),
    )
    prefix = cleaned.rstrip() + "\n\n" if cleaned.strip() else ""
    updated = prefix + block
    atomic_write_text(rc_file, updated)
    return removed_blocks


def strip_managed_blocks(content: str, marker_pairs: tuple[tuple[str, str], ...]) -> tuple[str, int]:
    lines = content.splitlines(keepends=True)
    output: list[str] = []
    index = 0
    removed_blocks = 0
    marker_by_start = dict(marker_pairs)
    all_markers = {marker for pair in marker_pairs for marker in pair}

    while index < len(lines):
        line_value = lines[index].rstrip("\r\n")
        end_marker = marker_by_start.get(line_value)
        if end_marker is None:
            output.append(lines[index])
            index += 1
            continue

        end_index = index + 1
        while end_index < len(lines):
            candidate = lines[end_index].rstrip("\r\n")
            if candidate == end_marker:
                break
            if candidate in all_markers:
                raise SystemExit(
                    "managed_block=failed reason=malformed_nested_marker "
                    f"start={line_value} marker={candidate}"
                )
            end_index += 1
        if end_index >= len(lines):
            raise SystemExit(
                f"managed_block=failed reason=missing_end_marker start={line_value} expected={end_marker}"
            )
        index = end_index + 1
        removed_blocks += 1
    return trim_repeated_blank_lines_text("".join(output)), removed_blocks


def find_next_managed_block_start(
    content: str,
    marker_pairs: tuple[tuple[str, str], ...],
):
    found = None
    for start_marker, end_marker in marker_pairs:
        start = content.find(start_marker)
        if start == -1:
            continue
        if found is None or start < found[0]:
            found = (start, start_marker, end_marker)
    return found


def atomic_write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    write_path = path.resolve(strict=False) if path.is_symlink() else path
    write_path.parent.mkdir(parents=True, exist_ok=True)
    existing_mode: int | None = None
    try:
        existing_mode = stat.S_IMODE(write_path.stat().st_mode)
    except FileNotFoundError:
        pass

    fd, tmp_name = tempfile.mkstemp(prefix=f".{write_path.name}.", dir=str(write_path.parent))
    tmp_path = Path(tmp_name)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="") as file:
            file.write(content)
            file.flush()
            os.fsync(file.fileno())
        if existing_mode is not None:
            os.chmod(tmp_path, existing_mode)
        else:
            current_umask = os.umask(0)
            os.umask(current_umask)
            os.chmod(tmp_path, 0o666 & ~current_umask)
        os.replace(tmp_path, write_path)
        fsync_directory(write_path.parent)
    finally:
        if tmp_path.exists():
            tmp_path.unlink()


def trim_repeated_blank_lines_text(content: str) -> str:
    result: list[str] = []
    blank = False
    for line in content.splitlines():
        is_blank = not line.strip()
        if is_blank and blank:
            continue
        result.append(line)
        blank = is_blank
    while result and not result[0].strip():
        result.pop(0)
    while result and not result[-1].strip():
        result.pop()
    return "\n".join(result)


if __name__ == "__main__":
    raise SystemExit(main())
