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
import html
import hashlib
import ipaddress
import os
import platform
import stat
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path


DEFAULT_REPO = "zrma/rustory"
MAX_ASSET_BYTES = 128 * 1024 * 1024
MAX_CHECKSUM_BYTES = 64 * 1024
HOOK_START = "# >>> rustory hook >>>"
HOOK_END = "# <<< rustory hook <<<"
DAEMON_AUTOSTART_START = "# >>> rustory daemon autostart >>>"
DAEMON_AUTOSTART_END = "# <<< rustory daemon autostart <<<"
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

    if args.swarm_key_source or args.swarm_key_b64:
        install_swarm_key(install_path, args)

    if args.token or args.trackers or args.relay:
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
    try:
        output = subprocess.run(
            [str(install_path), "version"],
            check=True,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
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
    print(f"binary_check={first_line}")


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
    daemon_args = [
        str(install_path),
        "daemon",
        "--interval-sec",
        str(args.daemon_interval_sec),
        "--start-jitter-sec",
        str(args.daemon_start_jitter_sec),
    ]
    if system == "darwin":
        install_launchd_daemon(daemon_args, not args.no_start_daemon)
        return
    if system == "linux":
        install_systemd_user_daemon(daemon_args, not args.no_start_daemon, args)
        return
    raise SystemExit(f"daemon=failed reason=unsupported_platform platform={platform.system()}")


def install_launchd_daemon(daemon_args: list[str], start: bool) -> None:
    label = "com.rustory.daemon"
    plist_path = Path.home() / "Library" / "LaunchAgents" / f"{label}.plist"
    log_dir = Path.home() / "Library" / "Logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    plist_path.parent.mkdir(parents=True, exist_ok=True)
    plist_path.write_text(render_launchd_plist(label, daemon_args, log_dir), encoding="utf-8")
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


def render_launchd_plist(label: str, daemon_args: list[str], log_dir: Path) -> str:
    arg_lines = "\n".join(f"    <string>{html.escape(arg)}</string>" for arg in daemon_args)
    stdout_path = html.escape(str(log_dir / "rustory-daemon.out.log"))
    stderr_path = html.escape(str(log_dir / "rustory-daemon.err.log"))
    path_value = html.escape(os.environ.get("PATH", ""))
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
  </dict>
</dict>
</plist>
"""


def install_systemd_user_daemon(daemon_args: list[str], start: bool, args: argparse.Namespace) -> None:
    unit_path = Path.home() / ".config" / "systemd" / "user" / "rustory.service"
    unit_path.parent.mkdir(parents=True, exist_ok=True)
    unit_path.write_text(render_systemd_user_unit(daemon_args), encoding="utf-8")
    os.chmod(unit_path, 0o644)
    print(f"daemon=installed manager=systemd-user unit={unit_path}")

    if start:
        for step in (["daemon-reload"], ["enable", "rustory.service"], ["restart", "rustory.service"]):
            try:
                run_systemd_user(step)
            except subprocess.CalledProcessError as exc:
                if systemd_user_bus_unavailable(exc):
                    print_systemd_user_start_deferred(step[0], exc)
                    start_background_daemon(daemon_args)
                    install_background_daemon_autostart(daemon_args, args)
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


def start_background_daemon(daemon_args: list[str]) -> None:
    state_dir = rustory_state_dir()
    state_dir.mkdir(parents=True, exist_ok=True)
    pid_path = state_dir / "daemon.pid"
    log_path = state_dir / "daemon.log"

    existing_pid = read_pid_file(pid_path)
    if existing_pid and pid_is_running(existing_pid):
        print(f"daemon=started manager=background status=already_running pid={existing_pid} log={log_path}")
        return

    with log_path.open("ab") as log_file:
        proc = subprocess.Popen(
            daemon_args,
            cwd=str(Path.home()),
            stdin=subprocess.DEVNULL,
            stdout=log_file,
            stderr=subprocess.STDOUT,
            start_new_session=True,
            close_fds=True,
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

    print(f"daemon=started manager=background pid={proc.pid} log={log_path}")
    print("daemon=start_note manager=background persistence=until_process_exit_or_reboot")


def install_background_daemon_autostart(daemon_args: list[str], args: argparse.Namespace) -> None:
    if args.no_daemon_shell_autostart:
        print("daemon=autostart_skipped manager=background reason=--no-daemon-shell-autostart")
        return

    shell = resolve_hook_shell(args.hook_shell)
    rc_file = Path(args.rc_file).expanduser() if args.rc_file else default_rc_file(shell)
    block = render_daemon_autostart_block(daemon_args)
    update_managed_block(rc_file, block, DAEMON_AUTOSTART_START, DAEMON_AUTOSTART_END)
    print(f"daemon=autostart_installed manager=background shell={shell} rc_file={rc_file}")


def render_daemon_autostart_block(daemon_args: list[str]) -> str:
    daemon_command = " ".join(shell_quote_arg(arg) for arg in daemon_args)
    return "\n".join(
        [
            DAEMON_AUTOSTART_START,
            "# Managed by rustory installer. Re-run with --install-daemon to update.",
            "case $- in",
            "  *i*)",
            '    __rustory_daemon_state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/rustory"',
            '    __rustory_daemon_pid_file="$__rustory_daemon_state_dir/daemon.pid"',
            '    __rustory_daemon_log_file="$__rustory_daemon_state_dir/daemon.log"',
            "    __rustory_daemon_running=0",
            '    if [ -r "$__rustory_daemon_pid_file" ]; then',
            '      __rustory_daemon_pid="$(cat "$__rustory_daemon_pid_file" 2>/dev/null)"',
            '      case "$__rustory_daemon_pid" in',
            "        ''|*[!0-9]*) __rustory_daemon_running=0 ;;",
            '        *) kill -0 "$__rustory_daemon_pid" >/dev/null 2>&1 && __rustory_daemon_running=1 ;;',
            "      esac",
            "    fi",
            '    if [ "$__rustory_daemon_running" != "1" ]; then',
            '      mkdir -p "$__rustory_daemon_state_dir"',
            f'      nohup {daemon_command} >> "$__rustory_daemon_log_file" 2>&1 </dev/null &',
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


def rustory_state_dir() -> Path:
    base = os.environ.get("XDG_STATE_HOME")
    if base:
        return Path(base).expanduser() / "rustory"
    return Path.home() / ".local" / "state" / "rustory"


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


def render_systemd_user_unit(daemon_args: list[str]) -> str:
    exec_start = " ".join(systemd_quote_arg(arg) for arg in daemon_args)
    return f"""[Unit]
Description=Rustory daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={exec_start}
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
"""


def systemd_quote_arg(value: str) -> str:
    if value and all(ch.isalnum() or ch in "/._:=@+-" for ch in value):
        return value
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


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

    lines = path.read_text().splitlines()
    cleaned = remove_hishtory_lines(lines)
    if cleaned == lines:
        return False

    had_final_newline = path.read_text().endswith("\n")
    text = "\n".join(cleaned)
    if text and had_final_newline:
        text += "\n"
    path.write_text(text)
    return True


def remove_hishtory_lines(lines: list[str]) -> list[str]:
    cleaned: list[str] = []
    index = 0
    while index < len(lines):
        line = lines[index]
        if is_hishtory_line(line):
            index += 1
            continue
        if is_hishtory_config_header(line):
            index = skip_following_hishtory_block(lines, index + 1)
            continue
        cleaned.append(line)
        index += 1
    return trim_repeated_blank_lines(cleaned)


def is_hishtory_config_header(line: str) -> bool:
    return "hishtory config" in line.casefold()


def skip_following_hishtory_block(lines: list[str], index: int) -> int:
    while index < len(lines):
        line = lines[index]
        stripped = line.strip()
        if not stripped:
            index += 1
            break
        if is_hishtory_line(line) or "hishtory" in line.casefold():
            index += 1
            continue
        break
    return index


def is_hishtory_line(line: str) -> bool:
    folded = line.casefold()
    stripped = line.strip()
    if "hishtory" not in folded:
        return False
    markers = (
        ".hishtory",
        "hishtory/config",
        "hishtory config",
        "hishtory init",
        "hishtory enable",
        "hishtory shell",
        "hishtory daemon",
        "source ",
        "export path",
        "eval ",
    )
    return stripped.startswith("#") or any(marker in folded for marker in markers)


def trim_repeated_blank_lines(lines: list[str]) -> list[str]:
    result: list[str] = []
    blank = False
    for line in lines:
        is_blank = not line.strip()
        if is_blank and blank:
            continue
        result.append(line)
        blank = is_blank
    while result and not result[0].strip():
        result.pop(0)
    while result and not result[-1].strip():
        result.pop()
    return result


def install_shell_hook(install_path: Path, args: argparse.Namespace) -> None:
    shell = resolve_hook_shell(args.hook_shell)
    rc_file = Path(args.rc_file).expanduser() if args.rc_file else default_rc_file(shell)
    block = render_hook_block(shell, install_path.parent)
    update_managed_block(rc_file, block)
    print(f"hook=installed shell={shell} rc_file={rc_file}")


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
) -> None:
    rc_file.parent.mkdir(parents=True, exist_ok=True)
    existing = rc_file.read_text() if rc_file.exists() else ""
    start = existing.find(start_marker)
    end = existing.find(end_marker)
    if start != -1 and end != -1 and end > start:
        end += len(end_marker)
        updated = existing[:start].rstrip() + "\n\n" + block + existing[end:].lstrip("\n")
    else:
        prefix = existing.rstrip() + "\n\n" if existing.strip() else ""
        updated = prefix + block
    rc_file.write_text(updated)


if __name__ == "__main__":
    raise SystemExit(main())
