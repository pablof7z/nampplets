#!/usr/bin/env python3
"""Focused tests for the file-growth ratchet check."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest


MODULE_PATH = Path(__file__).with_name("check_file_growth.py")
SPEC = importlib.util.spec_from_file_location("check_file_growth", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
RATCHET = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RATCHET)


def lines(count: int) -> str:
    return "\n".join(f"line {index}" for index in range(count)) + "\n"


class FileGrowthRatchetTests(unittest.TestCase):
    def make_repository(self, root: Path) -> None:
        subprocess.run(["git", "init", "-q"], cwd=root, check=True)
        subprocess.run(
            ["git", "config", "user.email", "test@example.com"], cwd=root, check=True
        )
        subprocess.run(["git", "config", "user.name", "Test"], cwd=root, check=True)

    def commit(self, root: Path, message: str) -> None:
        subprocess.run(["git", "add", "-A"], cwd=root, check=True)
        subprocess.run(
            ["git", "commit", "-q", "-m", message], cwd=root, check=True
        )

    def stage(self, root: Path) -> None:
        subprocess.run(["git", "add", "-A"], cwd=root, check=True)

    def test_new_file_under_ceiling_is_allowed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository)
            (repository / "empty.py").write_text("x = 1\n", encoding="utf-8")
            self.commit(repository, "base")

            (repository / "new.py").write_text(lines(599), encoding="utf-8")
            self.stage(repository)

            self.assertEqual(RATCHET.check(repository, "HEAD", "INDEX"), [])

    def test_new_file_crossing_ceiling_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository)
            (repository / "empty.py").write_text("x = 1\n", encoding="utf-8")
            self.commit(repository, "base")

            (repository / "new.py").write_text(lines(600), encoding="utf-8")
            self.stage(repository)

            violations = RATCHET.check(repository, "HEAD", "INDEX")
            self.assertEqual(len(violations), 1)
            self.assertIn("crosses the 600-line hard ceiling", violations[0])

    def test_growth_under_ceiling_is_allowed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository)
            (repository / "grow.py").write_text(lines(100), encoding="utf-8")
            self.commit(repository, "base")

            (repository / "grow.py").write_text(lines(400), encoding="utf-8")
            self.stage(repository)

            self.assertEqual(RATCHET.check(repository, "HEAD", "INDEX"), [])

    def test_file_already_over_ceiling_cannot_grow(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository)
            (repository / "big.py").write_text(lines(2000), encoding="utf-8")
            self.commit(repository, "base")

            (repository / "big.py").write_text(lines(2001), encoding="utf-8")
            self.stage(repository)

            violations = RATCHET.check(repository, "HEAD", "INDEX")
            self.assertEqual(len(violations), 1)
            self.assertIn("must not grow", violations[0])

    def test_file_already_over_ceiling_may_shrink_and_stay_over(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository)
            (repository / "big.py").write_text(lines(2000), encoding="utf-8")
            self.commit(repository, "base")

            (repository / "big.py").write_text(lines(1500), encoding="utf-8")
            self.stage(repository)

            self.assertEqual(RATCHET.check(repository, "HEAD", "INDEX"), [])

    def test_file_already_over_ceiling_unchanged_size_is_allowed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository)
            (repository / "big.py").write_text(lines(2000), encoding="utf-8")
            self.commit(repository, "base")

            (repository / "big.py").write_text(lines(2000)[:-1] + " \n", encoding="utf-8")
            self.stage(repository)

            self.assertEqual(RATCHET.check(repository, "HEAD", "INDEX"), [])

    def test_deleted_file_is_ignored(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository)
            (repository / "big.py").write_text(lines(2000), encoding="utf-8")
            self.commit(repository, "base")

            (repository / "big.py").unlink()
            self.stage(repository)

            self.assertEqual(RATCHET.check(repository, "HEAD", "INDEX"), [])

    def test_non_source_extension_is_not_checked(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository)
            (repository / "notes.md").write_text("short\n", encoding="utf-8")
            self.commit(repository, "base")

            (repository / "notes.md").write_text(lines(900), encoding="utf-8")
            self.stage(repository)

            self.assertEqual(RATCHET.check(repository, "HEAD", "INDEX"), [])

    def test_excluded_generated_file_is_not_checked(self) -> None:
        excluded_path = next(iter(RATCHET.run_antislop.EXCLUDED_FILES))
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository)
            target = repository / excluded_path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(lines(10), encoding="utf-8")
            self.commit(repository, "base")

            target.write_text(lines(2000), encoding="utf-8")
            self.stage(repository)

            self.assertEqual(RATCHET.check(repository, "HEAD", "INDEX"), [])

    def test_pure_rename_measures_growth_against_the_old_path(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository)
            (repository / "old.py").write_text(lines(700), encoding="utf-8")
            self.commit(repository, "base")

            subprocess.run(
                ["git", "mv", "old.py", "new.py"], cwd=repository, check=True
            )
            (repository / "new.py").write_text(lines(700), encoding="utf-8")
            self.stage(repository)

            self.assertEqual(RATCHET.check(repository, "HEAD", "INDEX"), [])

    def test_ci_mode_compares_two_commits_without_a_staged_index(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository)
            (repository / "grow.py").write_text(lines(100), encoding="utf-8")
            self.commit(repository, "base")
            base_sha = subprocess.run(
                ["git", "rev-parse", "HEAD"],
                cwd=repository,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()

            (repository / "grow.py").write_text(lines(650), encoding="utf-8")
            self.commit(repository, "head")

            violations = RATCHET.check(repository, base_sha, "HEAD")
            self.assertEqual(len(violations), 1)
            self.assertIn("crosses the 600-line hard ceiling", violations[0])


if __name__ == "__main__":
    unittest.main()
