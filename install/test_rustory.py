#!/usr/bin/env python3

import importlib.util
import os
import sys
import tempfile
import unittest
from pathlib import Path


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

    @unittest.skipUnless(sys.platform == "linux", "Linux /proc validation only")
    def test_stale_pid_file_for_unrelated_process_is_removed_without_signal(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            pid_path = Path(temp_dir) / "daemon.pid"
            pid_path.write_text(f"{os.getpid()}\n")

            validated = rustory.validated_background_pid(pid_path, Path("/tmp/not-rr"))

            self.assertIsNone(validated)
            self.assertFalse(pid_path.exists())
            os.kill(os.getpid(), 0)


if __name__ == "__main__":
    unittest.main()
