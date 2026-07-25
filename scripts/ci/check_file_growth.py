#!/usr/bin/env python3
"""Enforce the AGENTS.md 600-line file-size ceiling as a one-way ratchet.

A file already at or above the ceiling must not grow; a file below the
ceiling must not cross it. Shrinking is always allowed, even while a file
remains over the ceiling (a 2000-line file that drops to 1500 lines passes).
"""

from __future__ import annotations

import argparse
import importlib.util
from pathlib import Path
import subprocess
import sys


def _load_run_antislop():
    module_path = Path(__file__).with_name("run_antislop.py")
    spec = importlib.util.spec_from_file_location("run_antislop", module_path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


run_antislop = _load_run_antislop()


HARD_CEILING = 600


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--repository",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="repository root",
    )
    parser.add_argument(
        "--base",
        default="HEAD",
        help="git ref the change is measured against (default: HEAD)",
    )
    parser.add_argument(
        "--head",
        default="INDEX",
        help="git ref being checked, or INDEX for the staged tree (default: INDEX)",
    )
    return parser.parse_args()


def changed_paths(
    repository: Path, base: str, head: str
) -> list[tuple[str, str, str]]:
    """Return (status, old_path, new_path) triples for base..head changes.

    Deletions are omitted. Renames and copies carry their pre-image path in
    `old_path` so growth is measured against the file's own prior content,
    not treated as a brand-new file appearing at line zero.
    """
    command = ["git", "-C", str(repository), "diff", "--name-status"]
    if head == "INDEX":
        command += ["--cached", base]
    else:
        command += [base, head]
    result = subprocess.run(command, check=True, capture_output=True, text=True)

    entries = []
    for line in result.stdout.splitlines():
        if not line:
            continue
        fields = line.split("\t")
        status = fields[0]
        if status.startswith("D"):
            continue
        if status.startswith(("R", "C")):
            old_path, new_path = fields[1], fields[2]
        else:
            old_path = new_path = fields[1]
        entries.append((status, old_path, new_path))
    return entries


def line_count_at(repository: Path, ref: str, path: str) -> int | None:
    spec = f":{path}" if ref == "INDEX" else f"{ref}:{path}"
    result = subprocess.run(
        ["git", "-C", str(repository), "show", spec],
        capture_output=True,
    )
    if result.returncode != 0:
        return None
    if not result.stdout:
        return 0
    return result.stdout.count(b"\n") + (0 if result.stdout.endswith(b"\n") else 1)


def is_checked_source(path: str) -> bool:
    if path in run_antislop.EXCLUDED_FILES:
        return False
    if path.startswith(run_antislop.EXCLUDED_PREFIXES):
        return False
    return Path(path).suffix in run_antislop.SOURCE_EXTENSIONS


def check(repository: Path, base: str, head: str) -> list[str]:
    violations = []
    for _status, old_path, new_path in changed_paths(repository, base, head):
        if not is_checked_source(new_path):
            continue
        old_loc = line_count_at(repository, base, old_path)
        if old_loc is None:
            old_loc = 0
        new_loc = line_count_at(repository, head, new_path)
        if new_loc is None:
            continue

        if old_loc >= HARD_CEILING:
            if new_loc > old_loc:
                violations.append(
                    f"{new_path}: {old_loc} -> {new_loc} lines; already at or "
                    f"over the {HARD_CEILING}-line ceiling and must not grow "
                    "(split or shrink it instead)"
                )
        elif new_loc >= HARD_CEILING:
            violations.append(
                f"{new_path}: {old_loc} -> {new_loc} lines; crosses the "
                f"{HARD_CEILING}-line hard ceiling"
            )
    return violations


def main() -> int:
    args = parse_args()
    repository = args.repository.resolve()
    violations = check(repository, args.base, args.head)
    if violations:
        print(
            "File-growth ratchet violation (AGENTS.md 600-line ceiling):",
            file=sys.stderr,
        )
        for violation in violations:
            print(f"  - {violation}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, subprocess.SubprocessError) as error:
        print(f"File-growth ratchet check failed: {error}", file=sys.stderr)
        sys.exit(2)
