#!/usr/bin/env python3
"""Offline verifier for the committed M0 compatibility baseline."""

from __future__ import annotations

import hashlib
import json
import re
import sys
import tomllib
from pathlib import Path
from typing import Any

import generate_digests


ROOT = Path(__file__).resolve().parents[2]
CONFORMANCE = ROOT / "conformance"

NAP_DOMAINS = {
    "relay",
    "identity",
    "storage",
    "inc",
    "theme",
    "keys",
    "media",
    "notify",
    "config",
    "resource",
    "cvm",
    "outbox",
    "upload",
    "intent",
    "ble",
    "webrtc",
    "link",
    "count",
    "lists",
    "serial",
    "common",
    "dm",
}


class BaselineError(ValueError):
    """Raised when committed compatibility evidence is inconsistent."""


def sha256_file(file: Path) -> str:
    return hashlib.sha256(file.read_bytes()).hexdigest()


def load_json(relative: str) -> Any:
    return json.loads((ROOT / relative).read_text(encoding="utf-8"))


def load_lock() -> dict[str, Any]:
    with (ROOT / "compatibility.lock").open("rb") as handle:
        return tomllib.load(handle)


def verify_lock(lock: dict[str, Any]) -> None:
    if lock["baseline"]["schema"] != 1:
        raise BaselineError("unsupported lock schema")
    if lock["baseline"]["status"] not in {"unratified", "ratified"}:
        raise BaselineError("baseline status must be unratified or ratified")

    required_commits = (
        lock["nip_5d"]["commit"],
        lock["nap_registry"]["commit"],
        lock["napplet_packages"]["commit"],
        lock["kehto"]["commit"],
        lock["nmp"]["commit"],
    )
    if any(not re.fullmatch(r"[0-9a-f]{40}", commit) for commit in required_commits):
        raise BaselineError("every upstream commit must be exact 40-hex")

    if lock["nip_5d"]["manifest_kinds"] != [5129, 15129, 35129]:
        raise BaselineError("manifest kind baseline drifted")
    if set(lock["artifacts"]["accepted_modes"]) != {"single-file", "external-assets"}:
        raise BaselineError("artifact modes do not match deliberate baseline")
    artifact_redirect_contract = {
        "redirect_policy": "manual-per-hop-revalidation",
        "maximum_redirect_hops": 5,
        "accepted_redirect_statuses": [301, 302, 303, 307, 308],
        "transport_auto_follow": False,
        "requires_https": True,
        "requires_credential_free_url": True,
        "requires_query_free_url": True,
        "requires_fragment_free_url": True,
        "requires_fresh_dns_each_hop": True,
        "requires_public_address_each_hop": True,
        "requires_address_pinned_connection": True,
        "requires_hostname_tls_sni": True,
        "allows_ambient_proxy": False,
        "requires_exact_effective_url_each_hop": True,
        "default_request_deadline_seconds": 15,
        "default_maximum_path_bytes": 8 * 1024 * 1024,
        "default_maximum_artifact_bytes": 32 * 1024 * 1024,
        "requires_typed_observable_refusal": True,
        "retention_execution_gate": "per-path-sha256-and-aggregate",
    }
    for field, expected in artifact_redirect_contract.items():
        if lock["artifacts"].get(field) != expected:
            raise BaselineError(f"artifact redirect contract drifted: {field}")
    if lock["web_projection"]["sandbox_tokens"] != ["allow-scripts"]:
        raise BaselineError("sandbox baseline must contain only allow-scripts")
    for required_true in (
        "forbid_allow_same_origin",
        "require_srcdoc",
        "require_source_window_binding",
        "forbid_window_nostr",
    ):
        if lock["web_projection"][required_true] is not True:
            raise BaselineError(f"web projection invariant disabled: {required_true}")
    if lock["web_projection"]["unknown_message_policy"] != "ignore":
        raise BaselineError("unknown message policy must be ignore")

    if set(lock["domain_versions"]) != NAP_DOMAINS | {"shell_handshake"}:
        raise BaselineError("domain version map is incomplete")
    for platform in ("macos", "ios", "android"):
        provider = lock["platform"][platform]
        supported = set(provider["supported_domains"])
        unsupported = set(provider["unsupported_domains"])
        if supported & unsupported:
            raise BaselineError(f"{platform}: supported/unsupported overlap")
        if supported | unsupported != NAP_DOMAINS:
            raise BaselineError(f"{platform}: provider matrix is incomplete")
        if supported:
            raise BaselineError(f"{platform}: M0 cannot advertise providers")

    profiles = lock.get("artifact_profiles", {})
    if set(profiles) != {"good_morning"}:
        raise BaselineError("exact-build artifact profile inventory drifted")
    good_morning = profiles["good_morning"]
    expected_identity = {
        "source": "published-immutable-artifacts",
        "author": (
            "266815e0c9210dfa324c6cba3573b14b"
            "ee49da4209a9456f9484e5106cd408a5"
        ),
        "d_tag": "good-morning",
        "aggregate_sha256": (
            "828a6df02afd56782ea20f805084acce6"
            "5c53f7c37554948c1e0a64aa5a2b0a8"
        ),
    }
    for field, expected in expected_identity.items():
        if good_morning.get(field) != expected:
            raise BaselineError(f"Good Morning artifact profile {field} drifted")
    required = good_morning.get("required_domains")
    optional = good_morning.get("optional_domains")
    if required != ["identity", "inc", "outbox"]:
        raise BaselineError("Good Morning required capability profile drifted")
    if optional != ["resource", "theme", "link"]:
        raise BaselineError("Good Morning optional capability profile drifted")
    if set(required) & set(optional):
        raise BaselineError("Good Morning capability classes overlap")
    if not set(required + optional) <= NAP_DOMAINS:
        raise BaselineError("Good Morning capability profile has an unknown domain")

    status = lock["baseline"]["status"]
    signoffs = lock["signoff"]
    if signoffs.get("product_owner") != "pablof7z":
        raise BaselineError("product-owner direction is not recorded")
    pending_signoffs = (
        signoffs.get("compatibility_reviewer"),
        signoffs.get("security_reviewer"),
        signoffs.get("nmp_boundary_reviewer"),
    )
    if status == "unratified" and any(pending_signoffs):
        raise BaselineError("unratified baseline must keep pending signoffs empty")
    if status == "ratified" and any(not value.strip() for value in signoffs.values()):
        raise BaselineError("ratified baseline requires every signoff")
    if any(value == "PENDING" for value in lock["corpus"].values()):
        raise BaselineError("corpus digests are not finalized")


def verify_digest_manifest() -> int:
    manifest = CONFORMANCE / "digests.sha256"
    expected_paths = {
        relative.as_posix() for relative in generate_digests.tracked_inputs()
    }
    observed_paths: set[str] = set()
    count = 0
    for line_number, line in enumerate(manifest.read_text(encoding="utf-8").splitlines(), 1):
        match = re.fullmatch(r"([0-9a-f]{64})  (.+)", line)
        if not match:
            raise BaselineError(f"invalid digest line {line_number}")
        expected, relative = match.groups()
        if relative in observed_paths:
            raise BaselineError(f"duplicate digest target: {relative}")
        observed_paths.add(relative)
        file = ROOT / relative
        if not file.is_file():
            raise BaselineError(f"digest target missing: {relative}")
        actual = sha256_file(file)
        if actual != expected:
            raise BaselineError(
                f"digest mismatch for {relative}: expected {expected}, found {actual}"
            )
        count += 1
    missing = expected_paths - observed_paths
    unexpected = observed_paths - expected_paths
    if missing or unexpected:
        raise BaselineError(
            "digest target set drifted: "
            f"missing={sorted(missing)}, unexpected={sorted(unexpected)}"
        )
    if manifest.read_bytes() != generate_digests.manifest_bytes():
        raise BaselineError("digest manifest is not in canonical order or encoding")
    if count < 20:
        raise BaselineError("digest manifest is unexpectedly small")
    return count


def verify_envelopes(lock: dict[str, Any]) -> int:
    inventory = load_json("conformance/envelopes/inventory.json")
    if inventory["source"]["commit"] != lock["napplet_packages"]["commit"]:
        raise BaselineError("envelope inventory source commit mismatch")
    if inventory["unknown_message_policy"] != "ignore":
        raise BaselineError("envelope unknown-message policy mismatch")
    entries = inventory["entries"]
    types = [entry["type"] for entry in entries]
    if len(types) != len(set(types)):
        raise BaselineError("duplicate envelope inventory type")
    domains = {entry["domain"] for entry in entries}
    if not NAP_DOMAINS <= domains:
        raise BaselineError(f"envelope inventory misses domains: {NAP_DOMAINS - domains}")
    handshake = {
        entry["type"]: entry["validator"]
        for entry in entries
        if entry["domain"] == "shell"
    }
    if handshake != {
        "shell.init": "registry-only-handshake",
        "shell.ready": "registry-only-handshake",
    }:
        raise BaselineError("NAP-SHELL drift records changed")
    allowed = {"pinned-conformance", "registry-only-handshake", "explicit-unsupported"}
    if any(entry["validator"] not in allowed for entry in entries):
        raise BaselineError("envelope entry has no validator or unsupported record")
    return len(entries)


def verify_corpus(lock: dict[str, Any]) -> tuple[int, int, int]:
    reference = load_json("conformance/napplet-corpus/reference/index.json")
    kehto = load_json("conformance/napplet-corpus/kehto/index.json")
    published = load_json("conformance/napplet-corpus/published/index.json")

    if reference["digest"] != lock["corpus"]["reference_fixture_digest"]:
        raise BaselineError("reference corpus digest mismatch")
    if kehto["digest"] != lock["corpus"]["kehto_fixture_digest"]:
        raise BaselineError("Kehto corpus digest mismatch")
    if published["digest"] != lock["corpus"]["published_fixture_digest"]:
        raise BaselineError("published corpus digest mismatch")
    if kehto["source"]["commit"] != lock["kehto"]["commit"]:
        raise BaselineError("Kehto corpus commit mismatch")
    if kehto["source"]["git_tree"] != lock["kehto"]["corpus_tree"]:
        raise BaselineError("Kehto corpus tree mismatch")

    for collection, base in (
        (reference["fixtures"], CONFORMANCE / "napplet-corpus" / "reference"),
        (published["fixtures"], CONFORMANCE / "napplet-corpus" / "published"),
    ):
        for fixture in collection:
            fixture_root = base / fixture["name"]
            for file_record in fixture["files"]:
                file = fixture_root / file_record["path"]
                if sha256_file(file) != file_record["sha256"]:
                    raise BaselineError(f"corpus file mismatch: {file}")
                if file.stat().st_size != file_record["bytes"]:
                    raise BaselineError(f"corpus byte count mismatch: {file}")

    if len(published["fixtures"]) < 1:
        raise BaselineError("published corpus must contain a real immutable artifact")
    return (
        len(reference["fixtures"]),
        len(kehto["applications"]),
        len(published["fixtures"]),
    )


def verify_falsifiers() -> int:
    feature = (CONFORMANCE / "bdd" / "core.feature").read_text(encoding="utf-8")
    matrix = load_json("conformance/bdd/falsifiers.json")
    entries = matrix["entries"]
    invariants = {entry["invariant"] for entry in entries}
    expected = {f"I-{index:02d}" for index in range(1, 11)}
    if invariants != expected:
        raise BaselineError(f"falsifier coverage mismatch: {invariants ^ expected}")
    for entry in entries:
        if f"Scenario: {entry['scenario']}" not in feature:
            raise BaselineError(f"falsifier scenario missing: {entry['scenario']}")
        if entry["current_expected"] != "red":
            raise BaselineError("M0 falsifiers must honestly remain red")
        if not entry["falsifier"].strip():
            raise BaselineError("empty falsifier")
    return len(entries)


def verify_service_scenarios() -> tuple[int, int, int]:
    scenarios = load_json("conformance/test-services/scenarios.json")
    events = load_json("conformance/test-services/events.json")
    if scenarios["clock"]["kind"] != "manual":
        raise BaselineError("test service clock must be manual")
    if scenarios["secrets"]["fixture_policy"] != "no-secret-keys":
        raise BaselineError("test service fixtures may not contain secret keys")
    serialized_events = json.dumps(events, sort_keys=True, separators=(",", ":")).lower()
    if "private_key" in serialized_events or "secret_key" in serialized_events:
        raise BaselineError("test service event fixtures may not contain secret keys")
    for alias, event in events.items():
        canonical = [
            0,
            event["pubkey"],
            event["created_at"],
            event["kind"],
            event["tags"],
            event["content"],
        ]
        encoded = json.dumps(
            canonical,
            ensure_ascii=False,
            separators=(",", ":"),
        ).encode("utf-8")
        expected_id = hashlib.sha256(encoded).hexdigest()
        if event["id"] != expected_id:
            raise BaselineError(f"event fixture has invalid id: {alias}")
        if not re.fullmatch(r"[0-9a-f]{128}", event["sig"]):
            raise BaselineError(f"event fixture has invalid signature encoding: {alias}")
    relay_ids = {scenario["id"] for scenario in scenarios["relay"]}
    required_relays = {
        "eose-empty",
        "event-then-eose",
        "auth-required",
        "auth-denied",
        "relay-error",
        "disconnect-reconnect",
        "replacement",
        "deletion",
        "expiry",
    }
    if relay_ids != required_relays:
        raise BaselineError(f"relay scenario mismatch: {relay_ids ^ required_relays}")
    for scenario in scenarios["relay"]:
        for step in scenario["script"]:
            if step.startswith("event:") and step.removeprefix("event:") not in events:
                raise BaselineError(f"relay scenario references unknown event fixture: {step}")
    blob_by_id = {scenario["id"]: scenario for scenario in scenarios["blob"]}
    blob_ids = set(blob_by_id)
    required_blobs = {
        "verified-index",
        "one-byte-corrupt",
        "missing",
        "redirect-public-followed",
        "redirect-unsafe-target",
        "redirect-hop-limit",
        "redirect-effective-url-mismatch",
        "mime-mismatch",
        "oversized",
        "slow-stream",
    }
    if blob_ids != required_blobs:
        raise BaselineError(f"blob scenario mismatch: {blob_ids ^ required_blobs}")
    redirect_expectations = {
        "redirect-public-followed": {
            "status": 302,
            "request_url": "https://artifacts.example/index.html",
            "effective_url": "https://artifacts.example/index.html",
            "redirect_hop": 1,
            "mutation": "location:https://cdn.example/index.html",
            "expected_policy": "follow-after-manual-revalidation",
        },
        "redirect-unsafe-target": {
            "status": 302,
            "request_url": "https://artifacts.example/index.html",
            "effective_url": "https://artifacts.example/index.html",
            "redirect_hop": 1,
            "mutation": "location:https://127.0.0.1/index.html",
            "expected_policy": "typed-refusal-non-public-address",
        },
        "redirect-hop-limit": {
            "status": 308,
            "request_url": "https://hop-5.example/index.html",
            "effective_url": "https://hop-5.example/index.html",
            "redirect_hop": 6,
            "mutation": "location:https://hop-6.example/index.html",
            "expected_policy": "typed-refusal-hop-limit",
        },
        "redirect-effective-url-mismatch": {
            "status": 200,
            "request_url": "https://artifacts.example/index.html",
            "effective_url": "https://confused.example/index.html",
            "redirect_hop": 0,
            "mutation": "none",
            "expected_policy": "typed-refusal-effective-url",
        },
    }
    for scenario_id, expected in redirect_expectations.items():
        scenario = blob_by_id[scenario_id]
        if scenario.get("response_role") != "raw-hop-response":
            raise BaselineError(
                f"blob scenario {scenario_id} must remain a raw hop response"
            )
        for field, value in expected.items():
            if scenario.get(field) != value:
                raise BaselineError(
                    f"blob scenario {scenario_id} contract drifted: {field}"
                )
    signer_results = {scenario["result"] for scenario in scenarios["signer"]}
    if signer_results != {"approved", "rejected", "invalid", "unavailable"}:
        raise BaselineError("signer result coverage is incomplete")
    for scenario in scenarios["signer"]:
        fixture = scenario["response_fixture"]
        if fixture is not None and fixture not in events:
            raise BaselineError(f"signer scenario references unknown event fixture: {fixture}")
    return len(relay_ids), len(blob_ids), len(scenarios["signer"])


def verify() -> dict[str, Any]:
    lock = load_lock()
    verify_lock(lock)
    files = verify_digest_manifest()
    envelopes = verify_envelopes(lock)
    reference, kehto, published = verify_corpus(lock)
    falsifiers = verify_falsifiers()
    relay, blob, signer = verify_service_scenarios()
    return {
        "baseline": lock["baseline"]["name"],
        "status": lock["baseline"]["status"],
        "verified_files": files,
        "envelopes": envelopes,
        "corpus": {
            "reference": reference,
            "kehto": kehto,
            "published": published,
        },
        "falsifiers": falsifiers,
        "service_scenarios": {"relay": relay, "blob": blob, "signer": signer},
    }


def main() -> int:
    try:
        result = verify()
    except (BaselineError, KeyError, OSError, json.JSONDecodeError, tomllib.TOMLDecodeError) as error:
        print(f"compatibility baseline FAILED: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
