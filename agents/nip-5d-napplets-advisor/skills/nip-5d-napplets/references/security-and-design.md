# Security and design review

## Trust model

The useful default is:

| Actor | Trust |
| --- | --- |
| Napplet artifact and authored code | Untrusted |
| Trusted shell document / adapter | Trusted boundary |
| Runtime core and policy | Trusted |
| Capability provider | Trusted but fallible and bounded |
| Browser/WebView engine | Platform trust base |
| Relay, Blossom server, gateway, cache, URL target | Untrusted input/transport |
| Signer | Separate high-value authority |

The protocol does not protect against a malicious shell, compromised browser,
side channels, or social engineering. Say which threat is in scope.

## Review the complete causal path

```text
source event
-> verified manifest
-> verified blobs and aggregate
-> trusted bootstrap and CSP injection
-> sandboxed session
-> exact principal and exposed domains
-> request validation and grant
-> bounded provider
-> result/refusal
-> cancellation, teardown, and retained facts
```

A strong review attacks every arrow, not only iframe flags.

## Artifact and identity checks

- Verify the manifest signature before trusting tags.
- Validate kind, author, `d`, tag cardinality, paths, and size limits.
- Reject path traversal, duplicates, ambiguous indexes, and unsupported shapes.
- Fetch with bounded byte and time budgets.
- Hash every blob and recompute the aggregate.
- Compute identity from verified bytes, not gateway JSON or a napplet payload.
- Keep trusted injections outside the artifact hash and constrain them to their
  documented namespace.
- Register the source window before accepting messages.
- Do not let caches bypass verification.

Redirects are not a provenance failure when the final bytes match signed hashes.
They remain a transport-security concern: validate scheme, credentials, query
and fragment policy, hop count, DNS/IP class, TLS name, effective URL, size, and
deadline according to product policy.

## Sandbox and CSP

`sandbox="allow-scripts"` without `allow-same-origin` removes a real origin and
many ambient browser surfaces. It does not itself block every network request.

For `srcdoc`, a shell can inject a CSP meta element before authored resources or
scripts are parsed. The current proposal recommends a conservative baseline
with default deny, inline artifact script/style as necessary, data/blob images
and fonts as chosen, and no direct connect, worker, child, frame, media, object,
manifest, base, or form targets.

Review:

- injection order before authored code;
- whether the CSP actually covers every active resource class;
- absence of `allow-same-origin`;
- no navigation route that escapes the intended document;
- no raw native message handler visible to the untrusted frame;
- popup/form/download tokens as explicit host grants, not ambient defaults.

Dedicated per-napplet origins improve normal web ergonomics but restore storage,
service workers, cookies, stable origin identifiers, notifications, WebAuthn,
and more network/fingerprinting surface. Treat that as a different security
posture, not an equivalent spelling of opaque origin.

## Message boundary

- Require a plain structured-cloneable object and string `type`.
- Bound envelope depth, strings, arrays, tag counts, filters, and binary values.
- Route only recognized domains and operations.
- Check `MessageEvent.source` against the registered session every time.
- Ignore unknown types only at the compatibility boundary.
- Correlate request/results with collision-resistant session-scoped IDs.
- Bound outstanding requests and reject duplicates.
- Support cancellation and deterministic close.
- Prevent a late result from reviving a closed session.
- Emit typed refusals for known invalid or denied operations.

Do not treat `event.origin`, a payload token, dTag, or author assertion as
sender proof.

## Principals, grants, and updates

Protocol identity and product authorization need not be identical.

A strong runtime grant key is:

```text
(manifest author, dTag, aggregateHash)
```

This prevents grants from crossing publishers or changed builds. Decide and
document:

- whether updates inherit nothing, safe read grants, or an explicit reviewed
  subset;
- whether grants are one-shot, session, project, or durable;
- which operations require per-action approval even after domain grant;
- how revocation affects in-flight requests and retained storage;
- how users inspect the exact publisher, build hash, capability, and effect.

Never let a broad domain grant imply every operation, destination, byte count,
recipient, or durable effect.

## Signing and encryption

- Napplet code never receives keys, raw signers, extension handles, or arbitrary
  sign methods.
- Prefer intent-oriented operations where the runtime builds/validates the final
  event, selects relays, obtains consent, signs, and records receipts.
- The current proposal requires cleartext napplet output and forbids the shell
  from signing or broadcasting ciphertext supplied by the napplet.
- Encryption/decryption happens in trusted code under a defined domain contract.
- Bind consent to the actual event/effect shown to the user.
- Reject signer/account mismatch and stale approval.

Cleartext visibility improves runtime inspection; it is also sensitive data
inside the trusted shell. Define retention, redaction, diagnostics access, and
crash-report policy.

## Network and resources

Direct network inside an untrusted napplet undermines runtime mediation. Use a
resource/provider domain that owns:

- allowed schemes and destination policy;
- per-hop DNS/IP checks and redirect caps;
- byte, time, concurrency, and rate limits;
- MIME sniffing rather than trusting upstream headers;
- safe handling or rasterization of SVG and other active formats;
- cache partitioning by exact principal;
- cancellation and object-URL lifetime;
- privacy policy for prefetch and sidecars;
- explicit upload destination, durability, mirroring, and signed authorization.

Browser code often cannot safely implement DNS-pinned SSRF policy itself. A
native or controlled server provider may be required.

## Storage

Napplet storage is runtime-owned, scoped, quota-bound KV. Define:

- principal or instance scope;
- key/value and total byte limits;
- update and uninstall behavior;
- wipe/export/sync semantics;
- encryption at rest;
- concurrent write behavior;
- corruption recovery;
- observability without leaking values.

Do not use runtime KV as a second canonical event store, pending-write system,
replacement table, or relay cache.

## Provider design

Each provider should declare:

```text
domain and protocol revision
operations
input limits
grant requirements
concurrency class
timeouts and cancellation
error/refusal model
retained state
teardown behavior
diagnostics
platform availability
```

Unsupported domains are absent. A stub returning success is worse than an
honest preflight refusal.

## Lifecycle and pressure

For each resource class, name owner, limit, refusal, and release event:

- sessions and WebViews;
- outstanding requests;
- relay subscriptions;
- cached events or state frames;
- provider tasks and callbacks;
- media/device handles;
- uploaded/downloaded buffers;
- object URLs and temporary files;
- notifications and intent handlers;
- activity/diagnostic facts.

Test normal close, cancellation, provider failure, WebView crash, app
backgrounding, update, revoke, uninstall, and process restart. Resource counts
should return to an expected baseline.

## Common category errors

- Calling a package helper a protocol guarantee.
- Calling Kehto's ACL vocabulary universal.
- Adding `allow-same-origin` to solve build tooling without accepting the new
  authority surface.
- Using a gateway's aggregate claim without hashing the executed bytes.
- Treating a hash-matched redirect as a provenance attack.
- Advertising a domain because one happy-path handler exists.
- Letting napplet UI own durable publication after the runtime accepted it.
- Returning an empty result as if it proved global completeness.
- Ignoring known invalid requests because unknown future types are ignored.
- Adding retries, polling, or sleep loops instead of explicit lifecycle signals.
