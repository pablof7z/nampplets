#!/usr/bin/env python3
"""Focused tests for the tracked-source AntiSlop runner."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock


MODULE_PATH = Path(__file__).with_name("run_antislop.py")
SPEC = importlib.util.spec_from_file_location("run_antislop", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


class AntiSlopRunnerTests(unittest.TestCase):
    def make_repository(self, root: Path, files: list[str]) -> None:
        subprocess.run(["git", "init", "-q"], cwd=root, check=True)
        for relative in files:
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("source\n", encoding="utf-8")
        subprocess.run(["git", "add", "--", "."], cwd=root, check=True)

    def test_tracked_sources_cover_supported_extensions_and_exclusions(self) -> None:
        included = [
            f"source_{index}{extension}"
            for index, extension in enumerate(sorted(RUNNER.SOURCE_EXTENSIONS))
        ]
        excluded = [
            "README.md",
            "conformance/vendor/source.rs",
            "conformance/napplet-corpus/source.js",
            "web/trusted-shell/fixtures/source.js",
            "platforms/apple/Sources/NMPNativeRuntimeApple/Resources/"
            "TrustedShell/fixtures/source.js",
            "Packages/NMPNativeRuntime/Sources/NMPNativeRuntime/"
            "NMPNativeRuntime.swift",
        ]
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository, included + excluded)

            self.assertEqual(
                RUNNER.tracked_sources(repository),
                sorted(included),
            )

    def test_noncanonical_supported_extension_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository, ["source.PY"])

            with self.assertRaisesRegex(
                RuntimeError,
                "tracked source extension must be lowercase: source.PY",
            ):
                RUNNER.tracked_sources(repository)

    def test_official_uppercase_r_extension_is_selected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository, ["analysis.R"])

            self.assertEqual(
                RUNNER.tracked_sources(repository),
                ["analysis.R"],
            )

    def test_tracked_source_symlink_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository, ["target.txt"])
            (repository / "linked.py").symlink_to("target.txt")
            subprocess.run(
                ["git", "add", "--", "linked.py"],
                cwd=repository,
                check=True,
            )

            with self.assertRaisesRegex(
                RuntimeError,
                "tracked source is not a regular file: linked.py",
            ):
                RUNNER.tracked_sources(repository)

    def test_tracked_source_size_limit_accepts_boundary_and_refuses_excess(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository, ["bounded.py"])
            source = repository / "bounded.py"
            source.write_bytes(b"x" * RUNNER.MAX_SOURCE_BYTES)

            self.assertEqual(
                RUNNER.tracked_sources(repository),
                ["bounded.py"],
            )

            source.write_bytes(b"x" * (RUNNER.MAX_SOURCE_BYTES + 1))
            with self.assertRaisesRegex(
                RuntimeError,
                f"tracked source exceeds {RUNNER.MAX_SOURCE_BYTES}-byte limit",
            ):
                RUNNER.tracked_sources(repository)

    def test_trusted_shell_stub_exemption_requires_exact_reviewed_bytes(
        self,
    ) -> None:
        shell_paths = sorted(RUNNER.TRUSTED_SHELL_FILES)
        reviewed = b"function bounded() { return null; }\n"
        fingerprint = hashlib.sha256(reviewed).hexdigest()
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self.make_repository(repository, shell_paths)
            for relative in shell_paths:
                (repository / relative).write_bytes(reviewed)
            sources = RUNNER.tracked_sources(repository)

            with mock.patch.object(
                RUNNER, "TRUSTED_SHELL_SHA256", fingerprint
            ):
                self.assertEqual(
                    RUNNER.trusted_shell_sources(repository, sources),
                    shell_paths,
                )

                (repository / shell_paths[0]).write_bytes(reviewed + b" ")
                with self.assertRaisesRegex(
                    RuntimeError,
                    "trusted-shell source fingerprint changed",
                ):
                    RUNNER.trusted_shell_sources(repository, sources)

                with self.assertRaisesRegex(
                    RuntimeError,
                    "tracked trusted-shell source set changed",
                ):
                    RUNNER.trusted_shell_sources(
                        repository, [shell_paths[0]]
                    )

    def test_verify_version_accepts_only_the_pinned_release(self) -> None:
        repository = Path("/repository")
        success = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=f"antislop {RUNNER.ANTISLOP_VERSION}\n",
        )
        with mock.patch.object(RUNNER.subprocess, "run", return_value=success):
            RUNNER.verify_version("/tools/antislop", repository)

        mismatch = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="antislop 999.0.0\n",
        )
        with (
            mock.patch.object(RUNNER.subprocess, "run", return_value=mismatch),
            self.assertRaisesRegex(RuntimeError, "expected 'antislop 0.3.0'"),
        ):
            RUNNER.verify_version("/tools/antislop", repository)

    def test_materialized_config_comes_from_the_pinned_binary(self) -> None:
        result = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="patterns = []\n",
        )
        with (
            tempfile.TemporaryDirectory() as directory,
            mock.patch.object(
                RUNNER.subprocess, "run", return_value=result
            ) as run,
        ):
            config = RUNNER.materialize_builtin_config(
                "/tools/antislop",
                Path("/repository"),
                Path(directory),
            )
            self.assertEqual(
                config.read_text(encoding="utf-8"),
                "patterns = []\n",
            )

        self.assertEqual(
            run.call_args.args[0],
            ["/tools/antislop", "--print-config"],
        )
        self.assertEqual(run.call_args.kwargs["cwd"], Path("/repository"))
        self.assertTrue(run.call_args.kwargs["check"])

    def test_scan_places_paths_after_option_boundary_and_propagates_status(self) -> None:
        result = subprocess.CompletedProcess(args=[], returncode=7)
        with mock.patch.object(RUNNER.subprocess, "run", return_value=result) as run:
            actual = RUNNER.scan(
                "/tools/antislop",
                Path("/repository"),
                Path("/isolated/antislop-0.3.0.toml"),
                "fixture",
                ["ordinary.py", "--config=untrusted.py"],
                "--disable",
                "stub",
            )

        self.assertEqual(actual, 7)
        command = run.call_args.args[0]
        boundary = command.index("--")
        self.assertEqual(
            command[boundary + 1 :],
            ["ordinary.py", "--config=untrusted.py"],
        )
        self.assertEqual(
            command[1:3],
            ["--config", "/isolated/antislop-0.3.0.toml"],
        )
        self.assertEqual(command[3], "--extensions")
        configured_extensions = set(command[4].split(","))
        self.assertEqual(configured_extensions, RUNNER.SOURCE_EXTENSIONS)
        self.assertEqual(run.call_args.kwargs["cwd"], Path("/repository"))
        self.assertFalse(run.call_args.kwargs["check"])

    def test_scan_refuses_an_empty_selection(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "no source files selected"):
            RUNNER.scan(
                "/tools/antislop",
                Path("/repository"),
                Path("/isolated/antislop-0.3.0.toml"),
                "empty",
                [],
            )

    def test_repository_config_cannot_suppress_findings(self) -> None:
        binary = os.environ.get("ANTISLOP_BINARY") or shutil.which("antislop")
        if binary is None:
            self.skipTest("pinned AntiSlop binary is unavailable")

        with (
            tempfile.TemporaryDirectory() as directory,
            tempfile.TemporaryDirectory() as config_directory,
        ):
            repository = Path(directory)
            (repository / "antislop.toml").write_text(
                "patterns = []\n", encoding="utf-8"
            )
            (repository / "source.py").write_text(
                "def unfinished():\n"
                "    # TODO: implement\n"
                "    pass\n",
                encoding="utf-8",
            )
            RUNNER.verify_version(binary, repository)
            config = RUNNER.materialize_builtin_config(
                binary, repository, Path(config_directory)
            )

            self.assertNotEqual(
                RUNNER.scan(
                    binary,
                    repository,
                    config,
                    "malicious-config",
                    ["source.py"],
                ),
                0,
            )

    def test_main_fails_if_either_scan_fails(self) -> None:
        arguments = argparse.Namespace(
            binary="/tools/antislop",
            repository=Path("/repository"),
        )
        sources = [
            "ordinary.py",
            *sorted(RUNNER.TRUSTED_SHELL_FILES),
        ]
        for statuses in ([3, 0], [0, 3]):
            with (
                self.subTest(statuses=statuses),
                mock.patch.object(
                    RUNNER, "parse_args", return_value=arguments
                ),
                mock.patch.object(RUNNER, "verify_version"),
                mock.patch.object(
                    RUNNER, "tracked_sources", return_value=sources
                ),
                mock.patch.object(
                    RUNNER,
                    "trusted_shell_sources",
                    return_value=sorted(RUNNER.TRUSTED_SHELL_FILES),
                ),
                mock.patch.object(
                    RUNNER,
                    "materialize_builtin_config",
                    return_value=Path("/isolated/antislop-0.3.0.toml"),
                ),
                mock.patch.object(
                    RUNNER, "scan", side_effect=statuses
                ) as scan,
            ):
                self.assertEqual(RUNNER.main(), 1)
            self.assertEqual(scan.call_count, 2)


if __name__ == "__main__":
    unittest.main()
