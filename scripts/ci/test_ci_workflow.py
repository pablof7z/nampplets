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


if __name__ == "__main__":
    unittest.main()
