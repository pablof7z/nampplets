#!/usr/bin/env python3
"""Static contract tests for the required CI workflow topology.

Two independent contracts are enforced here. The first is the event topology.
The second is the job topology that keeps generated-artifact drift and a broken
source tree independently visible: the UniFFI bindings check must be its own
job, and the Swift build and test steps must run whatever that check does.
"""

from __future__ import annotations

from pathlib import Path
import re
import unittest


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = REPOSITORY_ROOT / ".github" / "workflows" / "ci.yml"
EVENT_KEY = re.compile(r"^  ([A-Za-z_][A-Za-z0-9_-]*):(?:\s*(.*))?$")
JOB_KEY = re.compile(r"^  ([A-Za-z_][A-Za-z0-9_-]*):$")
STEP_NAME = re.compile(r"^      - name: (.+)$")
BINDINGS_JOB = "bindings"
APPLE_JOB = "apple"
SWIFT_STEPS = {
    "Build the iOS reference shell for a generic simulator",
    "Test the generated Swift binding package",
    "Test the Apple host package",
    "Test the Workbench feature package",
    "Build and test the shared RuntimeWorkbench scheme",
}
UNCONDITIONAL_STEP = "if: ${{ !cancelled() }}"


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


def parse_jobs(source: str) -> dict[str, str]:
    """Return each job identifier mapped to its raw body text."""
    lines = source.splitlines()
    job_indexes = [index for index, line in enumerate(lines) if line.rstrip() == "jobs:"]
    if len(job_indexes) != 1:
        raise ValueError("workflow must contain exactly one top-level jobs block")

    jobs: dict[str, list[str]] = {}
    current_job: str | None = None
    for line in lines[job_indexes[0] + 1 :]:
        if line.strip() and not line.startswith(" "):
            break
        if line.lstrip().startswith("#"):
            continue

        match = JOB_KEY.fullmatch(line.rstrip())
        if match:
            current_job = match.group(1)
            if current_job in jobs:
                raise ValueError(f"duplicate workflow job: {current_job}")
            jobs[current_job] = []
            continue

        if current_job is not None:
            jobs[current_job].append(line)

    return {job: "\n".join(body) for job, body in jobs.items()}


def parse_steps(job_body: str) -> dict[str, str]:
    """Return each step name in a job mapped to its raw body text."""
    steps: dict[str, list[str]] = {}
    current_step: str | None = None
    for line in job_body.splitlines():
        if line.lstrip().startswith("#"):
            continue

        match = STEP_NAME.fullmatch(line.rstrip())
        if match:
            current_step = match.group(1)
            if current_step in steps:
                raise ValueError(f"duplicate step name: {current_step}")
            steps[current_step] = []
            continue

        if line.strip() and not line.startswith("      "):
            current_step = None
            continue
        if current_step is not None:
            steps[current_step].append(line)

    return {step: "\n".join(body) for step, body in steps.items()}


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


class CiWorkflowJobTopologyTests(unittest.TestCase):
    """A stale generated file must never hide a broken source tree."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.source = WORKFLOW_PATH.read_text()
        cls.jobs = parse_jobs(cls.source)

    def test_the_bindings_check_lives_in_its_own_job(self) -> None:
        checking_jobs = [
            job for job, body in self.jobs.items() if "--check-bindings" in body
        ]
        self.assertEqual(checking_jobs, [BINDINGS_JOB])
        self.assertIn("name: UniFFI Swift bindings", self.jobs[BINDINGS_JOB])
        self.assertIn(
            "name: Apple package and shared scheme",
            self.jobs[APPLE_JOB],
        )

    def test_swift_steps_never_wait_on_the_bindings_check(self) -> None:
        for job in (BINDINGS_JOB, APPLE_JOB):
            declared = [
                line
                for line in self.jobs[job].splitlines()
                if line.startswith("    needs:")
            ]
            self.assertEqual(declared, [], job)

    def test_swift_steps_compile_against_freshly_generated_bindings(self) -> None:
        apple = self.jobs[APPLE_JOB]
        self.assertIn(
            "run: scripts/build-runtime-swift-xcframework.sh --universal\n",
            apple,
        )
        steps = parse_steps(apple)
        for step in SWIFT_STEPS:
            self.assertIn(step, steps)

    def test_every_swift_step_reports_on_the_same_run(self) -> None:
        steps = parse_steps(self.jobs[APPLE_JOB])
        for name in SWIFT_STEPS:
            self.assertIn(UNCONDITIONAL_STEP, steps[name], name)



if __name__ == "__main__":
    unittest.main()
