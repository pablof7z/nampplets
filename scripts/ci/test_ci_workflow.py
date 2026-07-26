#!/usr/bin/env python3
"""Static contract tests for the required CI workflow event topology."""

from __future__ import annotations

from pathlib import Path
import re
import unittest


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = REPOSITORY_ROOT / ".github" / "workflows" / "ci.yml"
EVENT_KEY = re.compile(r"^  ([A-Za-z_][A-Za-z0-9_-]*):(?:\s*(.*))?$")


def parse_top_level_events(source: str) -> dict[str, list[str]]:
    """Return the immediate event mappings and their normalized child lines."""
    lines = source.splitlines()
    trigger_indexes = [
        index for index, line in enumerate(lines) if line.rstrip() == "on:"
    ]
    if len(trigger_indexes) != 1:
        raise ValueError("workflow must contain exactly one top-level on block")

    events: dict[str, list[str]] = {}
    current_event: str | None = None
    for line in lines[trigger_indexes[0] + 1 :]:
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if not line.startswith(" "):
            break
        if "\t" in line:
            raise ValueError("trigger block must use spaces for indentation")

        match = EVENT_KEY.fullmatch(line.rstrip())
        if match:
            current_event = match.group(1)
            if current_event in events:
                raise ValueError(f"duplicate workflow event: {current_event}")
            events[current_event] = []
            inline_value = match.group(2)
            if inline_value:
                events[current_event].append(inline_value)
            continue

        if current_event is None or not line.startswith("    "):
            raise ValueError(f"malformed trigger line: {line!r}")
        events[current_event].append(line[4:].rstrip())

    return events


class CiWorkflowEventTopologyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.events = parse_top_level_events(WORKFLOW_PATH.read_text())

    def test_pull_request_event_is_present(self) -> None:
        self.assertIn("pull_request", self.events)
        self.assertEqual(self.events["pull_request"], [])

    def test_feature_branch_push_is_absent_and_main_push_remains(self) -> None:
        self.assertEqual(self.events["push"], ["branches:", "  - main"])

    def test_manual_dispatch_is_present_and_events_are_exact(self) -> None:
        self.assertEqual(self.events["workflow_dispatch"], [])
        self.assertEqual(
            set(self.events),
            {"pull_request", "push", "workflow_dispatch"},
        )


class CiWorkflowConcurrencyTests(unittest.TestCase):
    """A merge to main must never cancel an earlier merge's verification.

    Keying the concurrency group by `github.ref` put every push to main in one
    group, so each merge cancelled the one before it. A burst of merges then
    left main with no completed run at all -- the branch looked verified
    because each PR had been green *before* merging, while the only run that
    sees those branches combined never finished. That is the failure this
    guards: a gate that reports success while measuring nothing.
    """

    @classmethod
    def setUpClass(cls) -> None:
        cls.concurrency = parse_concurrency_block(WORKFLOW_PATH.read_text())

    def test_group_distinguishes_individual_pushes(self) -> None:
        group = self.concurrency["group"]
        self.assertIn("github.sha", group)
        self.assertNotIn(
            "github.ref",
            group,
            "keying by ref collapses every push to main into one group",
        )

    def test_pushes_do_not_cancel_each_other(self) -> None:
        self.assertEqual(
            self.concurrency["cancel-in-progress"],
            "${{ github.event_name == 'pull_request' }}",
        )


def parse_concurrency_block(source: str) -> dict[str, str]:
    """Return the top-level concurrency mapping's immediate key/value pairs."""
    lines = source.splitlines()
    starts = [index for index, line in enumerate(lines) if line.rstrip() == "concurrency:"]
    if len(starts) != 1:
        raise ValueError("workflow must contain exactly one top-level concurrency block")

    block: dict[str, str] = {}
    for line in lines[starts[0] + 1 :]:
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if not line.startswith("  "):
            break
        match = EVENT_KEY.fullmatch(line.rstrip())
        if not match or match.group(2) is None:
            raise ValueError(f"malformed concurrency line: {line!r}")
        block[match.group(1)] = match.group(2)
    return block


if __name__ == "__main__":
    unittest.main()
