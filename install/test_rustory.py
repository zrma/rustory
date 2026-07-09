#!/usr/bin/env python3

import importlib.util
import os
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


if __name__ == "__main__":
    unittest.main()
