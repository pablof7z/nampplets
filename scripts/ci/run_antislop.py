#!/usr/bin/env python3
"""Run AntiSlop against tracked, first-party source files."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import stat
import subprocess
import sys
import tempfile


ANTISLOP_VERSION = "0.3.0"
MAX_SOURCE_BYTES = 1024 * 1024
SOURCE_EXTENSIONS = frozenset(
    {
        ".bash",
        ".c",
        ".cc",
        ".cjs",
        ".cpp",
        ".cs",
        ".cxx",
        ".dart",
        ".fish",
        ".go",
        ".h",
        ".hs",
        ".hpp",
        ".java",
        ".js",
        ".jsx",
        ".kt",
        ".kts",
        ".lua",
        ".mjs",
        ".pl",
        ".pm",
        ".php",
        ".py",
        ".r",
        ".R",
        ".rb",
        ".rs",
        ".scala",
        ".sh",
        ".swift",
        ".ts",
        ".tsx",
        ".zsh",
    }
)

# AntiSlop 0.3.0 parses exclude globs but does not apply them in its walker.
# Select tracked source explicitly so generated and third-party compatibility
# inputs cannot create findings that this repository does not own.
EXCLUDED_PREFIXES = (
    "conformance/napplet-corpus/",
    "conformance/vendor/",
    "platforms/apple/Sources/NMPNativeRuntimeApple/Resources/TrustedShell/fixtures/",
    "web/trusted-shell/fixtures/",
)
EXCLUDED_FILES = frozenset(
    {
        "Packages/NMPNativeRuntime/Sources/NMPNativeRuntime/NMPNativeRuntime.swift",
    }
)

# The 0.3.0 JavaScript AST heuristic treats every `return null` as a stub.
# These byte-identical trusted-shell sources use null as a bounded protocol
# result and return a generated compatibility prelude. Stub suppression is
# permitted only for the exact reviewed bytes; all non-stub categories remain
# active, and any shell change must deliberately update this fingerprint.
TRUSTED_SHELL_FILES = frozenset(
    {
        "platforms/apple/Sources/NMPNativeRuntimeApple/Resources/TrustedShell/trusted-shell.js",
        "web/trusted-shell/trusted-shell.js",
    }
)
TRUSTED_SHELL_SHA256 = (
    "94fd38f2d4b66d11a16482b62a0da55671381c0497779302416fc2ce922c42b7"
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--binary",
        default="antislop",
        help="AntiSlop executable (default: antislop from PATH)",
    )
    parser.add_argument(
        "--repository",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="repository root",
    )
    return parser.parse_args()


def tracked_sources(repository: Path) -> list[str]:
    result = subprocess.run(
        ["git", "-C", str(repository), "ls-files", "-z"],
        check=True,
        capture_output=True,
    )
    paths = result.stdout.decode("utf-8").split("\0")
    selected = []
    for path in paths:
        if (
            not path
            or path in EXCLUDED_FILES
            or path.startswith(EXCLUDED_PREFIXES)
        ):
            continue
        suffix = Path(path).suffix
        if suffix not in SOURCE_EXTENSIONS:
            if suffix.lower() in SOURCE_EXTENSIONS:
                raise RuntimeError(
                    f"tracked source extension must be lowercase: {path}"
                )
            continue
        source = repository / path
        metadata = source.lstat()
        if not stat.S_ISREG(metadata.st_mode):
            raise RuntimeError(f"tracked source is not a regular file: {path}")
        if metadata.st_size > MAX_SOURCE_BYTES:
            raise RuntimeError(
                f"tracked source exceeds {MAX_SOURCE_BYTES}-byte limit: {path}"
            )
        selected.append(path)
    return selected


def trusted_shell_sources(repository: Path, sources: list[str]) -> list[str]:
    selected = sorted(path for path in sources if path in TRUSTED_SHELL_FILES)
    expected = sorted(TRUSTED_SHELL_FILES)
    if selected != expected:
        raise RuntimeError(
            "tracked trusted-shell source set changed: "
            f"expected {expected!r}, got {selected!r}"
        )

    for path in selected:
        actual = hashlib.sha256((repository / path).read_bytes()).hexdigest()
        if actual != TRUSTED_SHELL_SHA256:
            raise RuntimeError(
                f"trusted-shell source fingerprint changed: {path}: "
                f"expected {TRUSTED_SHELL_SHA256}, got {actual}"
            )
    return selected


def verify_version(binary: str, repository: Path) -> None:
    result = subprocess.run(
        [binary, "--version"],
        cwd=repository,
        check=True,
        capture_output=True,
        text=True,
    )
    actual = result.stdout.strip()
    expected = f"antislop {ANTISLOP_VERSION}"
    if actual != expected:
        raise RuntimeError(f"expected {expected!r}, got {actual!r}")


def materialize_builtin_config(
    binary: str, repository: Path, directory: Path
) -> Path:
    result = subprocess.run(
        [binary, "--print-config"],
        cwd=repository,
        check=True,
        capture_output=True,
        text=True,
    )
    if not result.stdout.strip():
        raise RuntimeError("AntiSlop built-in configuration is empty")
    config = directory / f"antislop-{ANTISLOP_VERSION}.toml"
    config.write_text(result.stdout, encoding="utf-8")
    return config


def scan(
    binary: str,
    repository: Path,
    config: Path,
    label: str,
    paths: list[str],
    *options: str,
) -> int:
    if not paths:
        raise RuntimeError(f"no source files selected for {label}")

    print(f"AntiSlop: scanning {len(paths)} {label} files", flush=True)
    result = subprocess.run(
        [
            binary,
            "--config",
            str(config),
            "--extensions",
            ",".join(sorted(SOURCE_EXTENSIONS)),
            *options,
            "--",
            *paths,
        ],
        cwd=repository,
        check=False,
    )
    return result.returncode


def main() -> int:
    args = parse_args()
    repository = args.repository.resolve()
    verify_version(args.binary, repository)

    sources = tracked_sources(repository)
    trusted_shell = trusted_shell_sources(repository, sources)
    regular = sorted(path for path in sources if path not in TRUSTED_SHELL_FILES)

    with tempfile.TemporaryDirectory(prefix="nampplets-antislop-") as directory:
        config = materialize_builtin_config(
            args.binary, repository, Path(directory)
        )
        regular_status = scan(
            args.binary, repository, config, "first-party", regular
        )
        shell_status = scan(
            args.binary,
            repository,
            config,
            "trusted-shell",
            trusted_shell,
            "--disable",
            "stub",
        )
    return 1 if regular_status or shell_status else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"AntiSlop runner failed: {error}", file=sys.stderr)
        sys.exit(2)
