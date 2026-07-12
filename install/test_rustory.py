#!/usr/bin/env python3

import importlib.util
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


SCRIPT = Path(__file__).with_name("rustory.py")
SPEC = importlib.util.spec_from_file_location("rustory_installer", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
rustory = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(rustory)


class HishtoryCleanupTests(unittest.TestCase):
    def test_remove_hishtory_lines_preserves_unrelated_mentions(self) -> None:
        lines = [
            "# hishtory was retired",
            "export MIGRATION_NOTE='hishtory was retired'",
            "echo hishtory",
            'eval "echo hishtory init was old"',
            "",
            "",
            "# Hishtory Config",
            "source $HOME/.hishtory/config.sh",
            'export PATH="$PATH:$HOME/.hishtory"',
            'eval "$(hishtory init bash)"',
            "hishtory daemon start",
        ]

        self.assertEqual(
            rustory.remove_hishtory_lines(lines),
            lines[:6],
        )

    @unittest.skipUnless(hasattr(os, "symlink"), "symlinks are unavailable")
    def test_cleanup_hishtory_file_refuses_symlink_rewrite(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            target = root / "shared-zshrc"
            link = root / ".zshrc"
            target.write_text("source $HOME/.hishtory/config.zsh\n")
            link.symlink_to(target)

            with self.assertRaisesRegex(SystemExit, "symlinked startup file"):
                rustory.cleanup_hishtory_file(link)

            self.assertEqual(
                target.read_text(),
                "source $HOME/.hishtory/config.zsh\n",
            )


class BinaryInstallTests(unittest.TestCase):
    def test_failed_binary_check_preserves_existing_binary(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            install_path = Path(temp_dir) / "rr"
            original = b"#!/bin/sh\necho 'version: 1.2.2'\n"
            incompatible = b"#!/bin/sh\necho incompatible >&2\nexit 1\n"
            install_path.write_bytes(original)
            install_path.chmod(0o755)

            with self.assertRaises(SystemExit):
                rustory.install_binary(incompatible, install_path, requested_version="v1.2.3")

            self.assertEqual(install_path.read_bytes(), original)

    def test_pinned_version_mismatch_preserves_existing_binary(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            install_path = Path(temp_dir) / "rr"
            original = b"#!/bin/sh\necho 'version: 1.2.2'\n"
            wrong = b"#!/bin/sh\necho 'version: 9.9.9'\n"
            install_path.write_bytes(original)
            install_path.chmod(0o755)

            with self.assertRaisesRegex(SystemExit, "version_mismatch"):
                rustory.install_binary(wrong, install_path, requested_version="v1.2.3")

            self.assertEqual(install_path.read_bytes(), original)

    def test_pinned_version_match_installs_and_custom_tag_remains_compatible(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            install_path = Path(temp_dir) / "rr"
            matching = b"#!/bin/sh\necho 'version: 1.2.3'\n"
            custom = b"#!/bin/sh\necho 'version: 7.8.9'\n"

            rustory.install_binary(matching, install_path, requested_version="v1.2.3")
            self.assertEqual(install_path.read_bytes(), matching)
            rustory.install_binary(custom, install_path, requested_version="nightly-main")
            self.assertEqual(install_path.read_bytes(), custom)


class RetirementInstallerTests(unittest.TestCase):
    def test_managed_state_home_is_persisted_privately(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            state_home = root / "custom state"
            previous_state_home = root / "previous state"
            with mock.patch.object(Path, "home", return_value=root):
                rustory.record_managed_state_home(previous_state_home)
                rustory.record_managed_state_home(state_home)

            state_path = root / ".config" / "rustory" / "managed-state-home"
            self.assertEqual(state_path.read_text(encoding="utf-8"), f"{state_home}\n")
            self.assertEqual(state_path.stat().st_mode & 0o777, 0o600)
            history_path = root / ".config" / "rustory" / "managed-state-homes.json"
            self.assertEqual(
                rustory.json.loads(history_path.read_text(encoding="utf-8")),
                {
                    "version": 1,
                    "paths": [str(previous_state_home), str(state_home)],
                },
            )
            self.assertEqual(history_path.stat().st_mode & 0o777, 0o600)

    def test_background_child_receives_resolved_state_home_and_manager_marker(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            state_home = root / "resolved state"
            process = mock.Mock(pid=1234)
            process.poll.return_value = None
            with mock.patch.object(Path, "home", return_value=root):
                with mock.patch.object(rustory.subprocess, "Popen", return_value=process) as popen:
                    rustory.start_background_daemon(
                        [str(root / "bin" / "rr"), "daemon"],
                        state_home,
                    )

            child_env = popen.call_args.kwargs["env"]
            self.assertEqual(child_env["XDG_STATE_HOME"], str(state_home))
            self.assertEqual(child_env["RUSTORY_DAEMON_MANAGER"], "background")

    def test_background_pid_write_failure_terminates_spawned_child(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            state_home = root / "state"
            pid_path = state_home / "rustory" / "daemon.pid"
            pid_path.mkdir(parents=True)
            process = mock.Mock(pid=1234)
            process.poll.return_value = None
            process.wait.return_value = 0
            with (
                mock.patch.object(Path, "home", return_value=root),
                mock.patch.object(rustory.subprocess, "Popen", return_value=process),
                mock.patch.object(rustory, "signal_background_process") as signal_process,
                mock.patch.object(
                    rustory,
                    "remove_background_pid_file",
                    side_effect=PermissionError("injected cleanup failure"),
                ),
            ):
                with self.assertRaises(OSError):
                    rustory.start_background_daemon(
                        [str(root / "bin" / "rr"), "daemon"],
                        state_home,
                    )

            signal_process.assert_called_once_with(1234, rustory.signal.SIGTERM)
            process.wait.assert_called_once_with(timeout=2)

    def test_background_pid_writer_closes_raw_fd_on_setup_failure(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            captured_fds: list[int] = []
            original_mkstemp = rustory.tempfile.mkstemp

            def capture_mkstemp(*args, **kwargs):
                fd, path = original_mkstemp(*args, **kwargs)
                captured_fds.append(fd)
                return fd, path

            with (
                mock.patch.object(
                    rustory.tempfile,
                    "mkstemp",
                    side_effect=capture_mkstemp,
                ),
                mock.patch.object(
                    rustory.os,
                    "fchmod",
                    side_effect=OSError("injected chmod failure"),
                ),
            ):
                with self.assertRaises(OSError):
                    rustory.write_background_pid_file(root / "daemon.pid", 1234)

            self.assertEqual(len(captured_fds), 1)
            with self.assertRaises(OSError):
                os.fstat(captured_fds[0])

    def test_immediately_exited_background_child_removes_pid_file(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            state_home = root / "state"
            process = mock.Mock(pid=1234)
            process.poll.return_value = 7
            with (
                mock.patch.object(Path, "home", return_value=root),
                mock.patch.object(rustory.subprocess, "Popen", return_value=process),
                mock.patch.object(rustory.time, "sleep"),
            ):
                with self.assertRaises(SystemExit) as raised:
                    rustory.start_background_daemon(
                        [str(root / "bin" / "rr"), "daemon"],
                        state_home,
                    )

            self.assertEqual(raised.exception.code, 7)
            self.assertFalse((state_home / "rustory" / "daemon.pid").exists())

    def test_custom_rc_file_is_recorded_for_automated_uninstall(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            rc_file = root / "shell" / "custom.rc"
            state_home = root / "state"
            with mock.patch.dict(
                os.environ,
                {"HOME": str(root), "XDG_STATE_HOME": str(state_home)},
            ):
                with mock.patch.object(Path, "home", return_value=root):
                    rustory.record_managed_rc_file(rc_file)
                    rustory.record_managed_rc_file(rc_file)

            state_path = root / ".config" / "rustory" / "managed-rc-files.json"
            payload = rustory.json.loads(state_path.read_text(encoding="utf-8"))
            self.assertEqual(payload, {"version": 1, "paths": [str(rc_file.resolve())]})
            self.assertEqual(state_path.stat().st_mode & 0o777, 0o600)

    def test_custom_rc_is_recorded_before_hook_rewrite(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            rc_file = root / "custom.rc"
            state_home = root / "state"
            args = rustory.argparse.Namespace(hook_shell="zsh", rc_file=str(rc_file))
            with mock.patch.dict(
                os.environ,
                {"HOME": str(root), "XDG_STATE_HOME": str(state_home)},
            ):
                with mock.patch.object(Path, "home", return_value=root):
                    with mock.patch.object(
                        rustory,
                        "update_managed_block",
                        side_effect=OSError("injected write failure"),
                    ):
                        with self.assertRaisesRegex(OSError, "injected write failure"):
                            rustory.install_shell_hook(root / "bin" / "rr", args)

            state_path = root / ".config" / "rustory" / "managed-rc-files.json"
            payload = rustory.json.loads(state_path.read_text(encoding="utf-8"))
            self.assertEqual(payload["paths"], [str(rc_file.resolve())])

    def test_relative_xdg_state_home_is_rejected(self) -> None:
        with mock.patch.dict(os.environ, {"XDG_STATE_HOME": "relative/state"}):
            with self.assertRaisesRegex(SystemExit, "must be absolute"):
                rustory.rustory_state_dir()

    def test_remote_retirement_requires_strict_membership_and_one_tracker(self) -> None:
        with mock.patch.object(
            sys,
            "argv",
            ["rustory.py", "--allow-remote-retirement", "--tracker", "https://tracker.example"],
        ):
            with self.assertRaises(SystemExit):
                rustory.parse_args()

        with mock.patch.object(
            sys,
            "argv",
            [
                "rustory.py",
                "--allow-remote-retirement",
                "--require-device-membership",
                "--tracker",
                "https://tracker-a.example",
                "--tracker",
                "https://tracker-b.example",
            ],
        ):
            with self.assertRaises(SystemExit):
                rustory.parse_args()

    def test_remote_retirement_flags_are_forwarded_to_rr_init(self) -> None:
        with mock.patch.object(
            sys,
            "argv",
            [
                "rustory.py",
                "--allow-remote-retirement",
                "--require-device-membership",
                "--tracker",
                "https://tracker.example",
            ],
        ):
            args = rustory.parse_args()

        with mock.patch.object(rustory.subprocess, "run") as run:
            rustory.run_init(Path("/tmp/rr"), args)

        command = run.call_args.args[0]
        self.assertIn("--require-device-membership", command)
        self.assertIn("--allow-remote-retirement", command)
        self.assertIn("--update-existing-security", command)
        self.assertTrue(rustory.init_requested(args))

    def test_force_security_init_does_not_request_conflicting_merge(self) -> None:
        with mock.patch.object(
            sys,
            "argv",
            [
                "rustory.py",
                "--force",
                "--require-device-membership",
                "--tracker",
                "https://tracker.example",
            ],
        ):
            args = rustory.parse_args()

        with mock.patch.object(rustory.subprocess, "run") as run:
            rustory.run_init(Path("/tmp/rr"), args)

        command = run.call_args.args[0]
        self.assertIn("--force", command)
        self.assertNotIn("--update-existing-security", command)

    def test_user_or_device_only_install_still_requests_rr_init(self) -> None:
        with mock.patch.object(sys, "argv", ["rustory.py", "--user-id", "u1"]):
            args = rustory.parse_args()
        self.assertTrue(rustory.init_requested(args))

    def test_managed_service_templates_identify_the_recovery_manager(self) -> None:
        launchd = rustory.render_launchd_plist(
            "com.rustory.daemon",
            ["/tmp/rr", "daemon"],
            Path("/tmp/logs"),
            Path("/tmp/custom state"),
        )
        systemd = rustory.render_systemd_user_unit(
            ["/tmp/rr", "daemon"], Path("/tmp/custom state")
        )
        background = rustory.render_daemon_autostart_block(
            ["/tmp/rr", "daemon"], Path("/tmp/custom state")
        )

        self.assertIn("RUSTORY_DAEMON_MANAGER", launchd)
        self.assertIn("<string>launchd</string>", launchd)
        self.assertIn("<key>XDG_STATE_HOME</key>", launchd)
        self.assertIn("<string>/tmp/custom state</string>", launchd)
        self.assertIn("Environment=RUSTORY_DAEMON_MANAGER=systemd-user", systemd)
        self.assertIn('Environment="XDG_STATE_HOME=/tmp/custom state"', systemd)
        self.assertIn("RUSTORY_DAEMON_MANAGER=background setsid", background)
        self.assertIn("RUSTORY_DAEMON_MANAGER=background nohup", background)
        self.assertIn("systemctl --user is-active --quiet rustory.service", background)
        self.assertIn("XDG_STATE_HOME='/tmp/custom state'", background)
        self.assertIn('__rustory_daemon_expected_exe=/tmp/rr', background)
        self.assertIn('/proc/$__rustory_daemon_pid/exe', background)
        self.assertIn('/proc/$__rustory_daemon_pid/cmdline', background)
        self.assertIn('__rustory_daemon_subcommand" = "daemon"', background)


class ManagedBlockSafetyTests(unittest.TestCase):
    def test_quoted_marker_text_is_not_treated_as_managed_block(self) -> None:
        content = (
            "echo '# >>> rustory hook >>>'\n"
            "export KEEP=1\n"
            "echo '# <<< rustory hook <<<'\n"
        )

        cleaned, removed = rustory.strip_managed_blocks(
            content,
            ((rustory.HOOK_START, rustory.HOOK_END),),
        )

        self.assertEqual(removed, 0)
        self.assertEqual(cleaned, content.rstrip("\n"))

    def test_unmatched_marker_fails_without_rewriting_rc_file(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            rc_file = Path(temp_dir) / ".zshrc"
            original = f"{rustory.HOOK_START}\nexport KEEP=1\n"
            rc_file.write_text(original)

            with self.assertRaisesRegex(SystemExit, "missing_end_marker"):
                rustory.update_managed_block(
                    rc_file,
                    f"{rustory.HOOK_START}\nmanaged\n{rustory.HOOK_END}\n",
                )

            self.assertEqual(rc_file.read_text(), original)

    @unittest.skipUnless(hasattr(os, "symlink"), "symlinks are unavailable")
    def test_atomic_rc_rewrite_preserves_symlink_and_target_mode(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            target = root / "shared-zshrc"
            link = root / ".zshrc"
            target.write_text("export KEEP=1\n")
            target.chmod(0o640)
            link.symlink_to(target)
            block = f"{rustory.HOOK_START}\nmanaged\n{rustory.HOOK_END}\n"

            rustory.update_managed_block(link, block)

            self.assertTrue(link.is_symlink())
            self.assertIn("export KEEP=1", target.read_text())
            self.assertIn(rustory.HOOK_START, target.read_text())
            self.assertEqual(target.stat().st_mode & 0o777, 0o640)


class BackgroundDaemonSafetyTests(unittest.TestCase):
    def test_managed_cmdline_excludes_interactive_p2p_commands(self) -> None:
        self.assertFalse(rustory.is_managed_background_cmdline(["rr", "p2p-serve"]))
        self.assertFalse(rustory.is_managed_background_cmdline(["rr", "p2p-sync"]))
        self.assertTrue(
            rustory.is_managed_background_cmdline(
                [
                    "rr",
                    "--db-path",
                    "/home/user/.rustory/history.db",
                    "p2p-sync",
                    "--watch",
                ],
                has_managed_daemon_ancestor=True,
            )
        )
        self.assertFalse(
            rustory.is_managed_background_cmdline(
                [
                    "rr",
                    "--db-path",
                    "/home/user/.rustory/history.db",
                    "p2p-serve",
                ]
            )
        )
        self.assertTrue(
            rustory.is_managed_background_cmdline(
                ["rr", "daemon", "--interval-sec", "60", "--start-jitter-sec", "10"]
            )
        )

    def test_orphan_managed_child_remains_owned_by_background_marker(self) -> None:
        cmdline = [
            "rr",
            "--db-path",
            "/home/user/.rustory/history.db",
            "p2p-serve",
        ]
        with (
            mock.patch.object(rustory.sys, "platform", "linux"),
            mock.patch.object(rustory, "proc_is_current_user", return_value=True),
            mock.patch.object(rustory, "read_proc_cmdline", return_value=cmdline),
            mock.patch.object(rustory, "proc_exe_matches", return_value=True),
            mock.patch.object(rustory, "proc_has_background_manager", return_value=True),
        ):
            self.assertTrue(
                rustory.managed_process_matches_install(1234, Path("/home/user/.local/bin/rr"))
            )

    def test_pid_file_daemon_requires_background_marker(self) -> None:
        cmdline = ["rr", "daemon", "--interval-sec", "60", "--start-jitter-sec", "10"]
        with (
            mock.patch.object(rustory.sys, "platform", "linux"),
            mock.patch.object(rustory, "proc_is_current_user", return_value=True),
            mock.patch.object(rustory, "read_proc_cmdline", return_value=cmdline),
            mock.patch.object(rustory, "proc_exe_matches", return_value=True),
            mock.patch.object(rustory, "proc_has_background_manager", return_value=False),
        ):
            self.assertFalse(
                rustory.background_pid_matches_install(1234, Path("/home/user/.local/bin/rr"))
            )

    @unittest.skipUnless(sys.platform == "linux", "Linux /proc validation only")
    def test_stale_pid_file_for_unrelated_process_is_removed_without_signal(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            pid_path = Path(temp_dir) / "daemon.pid"
            pid_path.write_text(f"{os.getpid()}\n")

            validated = rustory.validated_background_pid(pid_path, Path("/tmp/not-rr"))

            self.assertIsNone(validated)
            self.assertFalse(pid_path.exists())
            os.kill(os.getpid(), 0)

    @unittest.skipUnless(
        sys.platform == "linux" and Path("/bin/bash").is_file(),
        "Linux shell autostart validation only",
    )
    def test_shell_autostart_does_not_trust_reused_unrelated_pid(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            state_home = root / "state"
            state_dir = state_home / "rustory"
            state_dir.mkdir(parents=True)
            (state_dir / "daemon.pid").write_text(f"{os.getpid()}\n")

            fake_bin = root / "bin"
            fake_bin.mkdir()
            marker = root / "daemon-started"
            fake_rr = fake_bin / "rr"
            fake_rr.write_text('#!/bin/sh\n: > "$RUSTORY_TEST_MARKER"\n')
            fake_rr.chmod(0o755)
            fake_systemctl = fake_bin / "systemctl"
            fake_systemctl.write_text("#!/bin/sh\nexit 1\n")
            fake_systemctl.chmod(0o755)
            fake_setsid = fake_bin / "setsid"
            fake_setsid.write_text('#!/bin/sh\nexec "$@"\n')
            fake_setsid.chmod(0o755)

            block_path = root / "autostart.sh"
            block_path.write_text(
                rustory.render_daemon_autostart_block(
                    [str(fake_rr), "daemon"], state_home
                )
            )
            command = (
                f". {rustory.shell_quote_arg(str(block_path))}; "
                "i=0; while [ ! -e \"$RUSTORY_TEST_MARKER\" ] && [ $i -lt 50 ]; do "
                "i=$((i + 1)); sleep 0.02; done"
            )
            env = os.environ.copy()
            env["PATH"] = f"{fake_bin}:{env.get('PATH', '')}"
            env["RUSTORY_TEST_MARKER"] = str(marker)

            completed = rustory.subprocess.run(
                ["/bin/bash", "--noprofile", "--norc", "-ic", command],
                env=env,
                capture_output=True,
                text=True,
                timeout=5,
                check=False,
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            self.assertTrue(marker.exists(), completed.stderr)

    @unittest.skipUnless(
        sys.platform == "linux" and Path("/bin/bash").is_file(),
        "Linux shell autostart validation only",
    )
    def test_shell_autostart_trusts_matching_daemon_process(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            state_home = root / "state"
            state_dir = state_home / "rustory"
            state_dir.mkdir(parents=True)
            daemon_script = root / "daemon"
            daemon_script.write_text("sleep 30\n")
            real_bash = Path("/bin/bash").resolve()
            daemon = rustory.subprocess.Popen(
                [str(real_bash), daemon_script.name],
                cwd=root,
                stdout=rustory.subprocess.DEVNULL,
                stderr=rustory.subprocess.DEVNULL,
            )

            def cleanup_daemon() -> None:
                if daemon.poll() is None:
                    daemon.kill()
                daemon.wait(timeout=5)

            self.addCleanup(cleanup_daemon)
            (state_dir / "daemon.pid").write_text(f"{daemon.pid}\n")

            fake_bin = root / "bin"
            fake_bin.mkdir()
            marker = root / "unexpected-daemon-start"
            fake_systemctl = fake_bin / "systemctl"
            fake_systemctl.write_text("#!/bin/sh\nexit 1\n")
            fake_systemctl.chmod(0o755)
            fake_setsid = fake_bin / "setsid"
            fake_setsid.write_text('#!/bin/sh\n: > "$RUSTORY_TEST_MARKER"\n')
            fake_setsid.chmod(0o755)

            block_path = root / "autostart.sh"
            block_path.write_text(
                rustory.render_daemon_autostart_block(
                    [str(real_bash), daemon_script.name], state_home
                )
            )
            env = os.environ.copy()
            env["PATH"] = f"{fake_bin}:{env.get('PATH', '')}"
            env["RUSTORY_TEST_MARKER"] = str(marker)
            completed = rustory.subprocess.run(
                [
                    str(real_bash),
                    "--noprofile",
                    "--norc",
                    "-ic",
                    f". {rustory.shell_quote_arg(str(block_path))}",
                ],
                cwd=root,
                env=env,
                capture_output=True,
                text=True,
                timeout=5,
                check=False,
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            self.assertFalse(marker.exists(), completed.stderr)
            self.assertIsNone(daemon.poll())


if __name__ == "__main__":
    unittest.main()
