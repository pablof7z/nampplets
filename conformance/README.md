# Conformance contract

This directory freezes M0 compatibility evidence. It does not claim that the
runtime implementation already passes the scenarios.

- `vendor/` contains byte-for-byte allowlisted source snapshots from the
  commits in `../compatibility.lock`. It is evidence, not runtime source.
- `envelopes/inventory.json` inventories every pinned conformance envelope and
  the registry-only NAP-SHELL handshake drift.
- `napplet-corpus/` indexes reference, Kehto, and published artifact bytes.
- `bdd/` contains the product feature and an explicit red/green falsifier map.
- `test-services/` defines deterministic relay/blob/signer/artifact scenarios
  consumed by the Rust test harness.
- `reports/` contains generated baseline evidence.

Blob redirect fixtures are deliberately flat raw-hop responses. Their
`request_url`, `effective_url`, `redirect_hop`, and `expected_policy` metadata
let policy tests compose a chain without pretending the deterministic
transport followed it. Full-chain follow/refusal behavior is exercised by the
catalog-resolver tests.

Regenerate from exact clean upstream checkouts:

```sh
python3 conformance/scripts/generate_baseline.py \
  --nip5d /path/to/dskvr-nips \
  --naps /path/to/napplet-naps \
  --napplet-web /path/to/napplet-web \
  --kehto /path/to/kehto-web
python3 conformance/scripts/generate_digests.py
python3 conformance/scripts/generate_digests.py --check
python3 conformance/scripts/verify_baseline.py
```

Digest generation reads only Git-tracked files in the allowlisted conformance
roots, sorts repository-relative POSIX paths, hashes file bytes only, and emits
fixed UTF-8/LF output. The `--check` form is the clean-checkout/CI assertion: it
does not write and fails when regenerating would change `digests.sha256`.

Importing a published artifact requires an exact signed event already committed
under `napplet-corpus/published/<name>/event.json`; the importer verifies the
path hash and intentionally refuses redirects:

```sh
python3 conformance/scripts/import_published_fixture.py \
  conformance/napplet-corpus/published/good-morning/event.json \
  conformance/napplet-corpus/published/good-morning/index.html
```

That refusal is a reproducible evidence-import rule, not runtime policy. The
runtime follows a bounded set of redirect statuses in Rust, manually
revalidating every hop while raw transport auto-follow remains disabled.

The normal offline gate is:

```sh
python3 -m unittest discover -s conformance/tests -p 'test_*.py'
```

Network access is never required to verify committed baseline bytes.

## M2 legacy execution evidence

The M0 baseline inventory is not a legacy-runtime pass. Executable M2 evidence
lives under `legacy-host/` and deliberately separates:

- package-active NAP domains from the registry-only NAP-SHELL handshake;
- trusted-host contract probes from the exact pinned conformance verdict;
- correct pre-execution `requires` refusal from a successful napplet boot;
- built artifacts from artifacts actually executed through the native host.

Run the committed reference and published fixtures with exact verified npm
archives:

```sh
python3 conformance/legacy-host/run.py \
  --allow-package-download \
  --package-cache /tmp/nampplets-npm-cache
```

Run the pinned Kehto source corpus from a reference checkout without modifying
it:

```sh
python3 conformance/legacy-host/run_kehto.py \
  --source /path/to/kehto-web \
  --dependency-store /path/to/pnpm-store
```

Both commands use finite process, output, fixture, and artifact limits.
The explicit store is populated from the pinned frozen lockfile in a separate
step; the evidence run remains offline. Unavailable offline dependencies are
emitted as machine-readable `not-run` reasons. See `legacy-host/README.md` for
the full contract and preparation command.
