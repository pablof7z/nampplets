"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const shell = require("../trusted-shell.js");
const policy = require("../trusted-shell-policy.js");

function createPreludeHarness(domains, initialConfigSchema) {
  const listeners = new Map();
  const sent = [];
  const objectURLs = {
    created: [],
    revoked: []
  };
  const URL = {
    createObjectURL(blob) {
      const value = `blob:trusted-shell-${objectURLs.created.length + 1}`;
      objectURLs.created.push({ value, blob });
      return value;
    },
    revokeObjectURL(value) {
      objectURLs.revoked.push(value);
    }
  };
  const parent = {
    postMessage(envelope, target) {
      sent.push({
        envelope: JSON.parse(JSON.stringify(envelope)),
        target
      });
    }
  };
  const context = {
    Map,
    Object,
    Promise,
    Set,
    Array,
    Number,
    TypeError,
    RangeError,
    Error,
    JSON,
    Blob,
    Uint8Array,
    atob,
    URL,
    parent,
    queueMicrotask,
    setTimeout,
    clearTimeout,
    addEventListener(type, listener) {
      listeners.set(type, listener);
    }
  };
  context.window = context;
  vm.runInNewContext(
    shell.compatibilityPreludeSource(domains, initialConfigSchema),
    context
  );
  return {
    context,
    listeners,
    parent,
    sent,
    objectURLs,
    receive(data, source = parent) {
      listeners.get("message")({ source, data });
    }
  };
}

test("only the exact mapped iframe window may forward an envelope", () => {
  const mappedWindow = {};
  const spoofingWindow = {};
  const frame = { contentWindow: mappedWindow };
  const privileged = { type: "shell.ping", requestId: "one" };

  assert.deepEqual(
    shell.mappedEnvelope({ source: mappedWindow, data: privileged }, frame),
    privileged
  );
  assert.equal(
    shell.mappedEnvelope({ source: spoofingWindow, data: privileged }, frame),
    null
  );
  assert.equal(
    shell.mappedEnvelope({ source: null, data: privileged }, frame),
    null
  );
});

test("the iframe sandbox and CSP deny ambient origin, network, and storage power", () => {
  const html = fs.readFileSync(
    path.join(__dirname, "..", "trusted-shell.html"),
    "utf8"
  );
  const js = fs.readFileSync(
    path.join(__dirname, "..", "trusted-shell.js"),
    "utf8"
  );

  assert.match(js, /setAttribute\("sandbox", "allow-scripts"\)/);
  assert.doesNotMatch(js, /allow-same-origin/);
  assert.match(html, /connect-src 'none'/);
  assert.match(shell.sandboxPolicyContent(), /connect-src 'none'/);
  assert.match(shell.sandboxPolicyContent(), /default-src 'none'/);
  assert.match(shell.sandboxPolicyContent(), /worker-src 'none'/);
  assert.match(
    shell.sandboxPolicyContent(),
    /script-src 'unsafe-inline' nmp-artifact:/
  );
  assert.match(
    shell.sandboxPolicyContent(),
    /style-src 'unsafe-inline' nmp-artifact:/
  );
  assert.match(shell.sandboxPolicyContent(), /connect-src 'none'/);
  assert.match(shell.sandboxPolicyContent(), /base-uri nmp-artifact:/);
});

test("outer and inner CSP are generated from the single reviewed allowlist", () => {
  const html = fs.readFileSync(
    path.join(__dirname, "..", "trusted-shell.html"),
    "utf8"
  );
  const escapedPolicy = policy.outerPolicyContent()
    .replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

  assert.match(html, new RegExp(`content="${escapedPolicy}"`));
  assert.equal(policy.ALLOWLIST.artifactScheme, "nmp-artifact:");
  assert.match(policy.innerPolicyContent(), /connect-src 'none'/);
  assert.doesNotMatch(policy.innerPolicyContent(), /https?:/);
  assert.doesNotMatch(policy.innerPolicyContent(), /wss?:/);
});

test("oversized and non-JSON messages never cross the bridge", () => {
  const mappedWindow = {};
  const frame = { contentWindow: mappedWindow };
  const oversized = {
    payload: "x".repeat(shell.MAX_ENVELOPE_BYTES + 1)
  };

  assert.equal(
    shell.mappedEnvelope({ source: mappedWindow, data: "shell.ping" }, frame),
    null
  );
  assert.equal(
    shell.mappedEnvelope({ source: mappedWindow, data: oversized }, frame),
    null
  );
  assert.equal(shell.isBoundedEnvelope(oversized), false);
  assert.equal(shell.isBoundedEnvelope({ type: "identity.changed" }), true);
});

test("materialization is parser-based rather than regex HTML rewriting", () => {
  const source = fs.readFileSync(
    path.join(__dirname, "..", "trusted-shell.js"),
    "utf8"
  );

  assert.match(source, /new global\.DOMParser\(\)/);
  assert.match(source, /parser\.parseFromString\(artifactHTML, "text\/html"\)/);
  assert.match(source, /head\.prepend\(policy\)/);
  assert.match(source, /head\.prepend\(base\)/);
  assert.doesNotMatch(source, /artifactHTML\.replace/);
  assert.match(
    shell.compatibilityPreludeSource(),
    /Object\.defineProperty\(window, "napplet"/
  );
  assert.equal(
    shell.compatibilityPreludeSource(["resource"]).includes("\u0000"),
    false,
    "the serialized prelude must not contain an HTML-replaced NUL"
  );
  assert.match(
    shell.compatibilityPreludeSource(["resource"]),
    /\\u0000-\\u001f\\u007f/,
    "control-character bounds must survive HTML parsing as regex escapes"
  );
  assert.equal(
    shell.isVerifiedArtifactBaseURL(
      "nmp-artifact://abcd1234-1234-4123-8123-abcdefabcdef/"
    ),
    true
  );
  assert.equal(
    shell.isVerifiedArtifactBaseURL("https://example.com/"),
    false
  );
});

test("the prelude performs the registry NAP-SHELL handshake exactly once", async () => {
  const listeners = new Map();
  const sent = [];
  const parent = {
    postMessage(envelope, target) {
      sent.push({ envelope: JSON.parse(JSON.stringify(envelope)), target });
    }
  };
  const context = {
    Map,
    Object,
    Promise,
    Set,
    Array,
    Number,
    TypeError,
    RangeError,
    Error,
    parent,
    queueMicrotask,
    setTimeout,
    clearTimeout,
    addEventListener(type, listener) {
      listeners.set(type, listener);
    }
  };
  context.window = context;
  vm.runInNewContext(shell.compatibilityPreludeSource(["storage"]), context);

  assert.deepEqual(sent, [
    { envelope: { type: "shell.ready" }, target: "*" }
  ]);
  assert.equal(context.napplet.shell.supports("storage"), false);
  assert.deepEqual(Array.from(context.napplet.shell.services), []);

  let callbackCount = 0;
  context.napplet.shell.onReady(() => {
    callbackCount += 1;
  });
  listeners.get("message")({
    source: parent,
    data: {
      type: "shell.init",
      capabilities: { domains: ["shell", "storage", "storage"] },
      services: ["settings"]
    }
  });
  const environment = await context.napplet.shell.ready();
  await new Promise((resolve) => queueMicrotask(resolve));

  assert.deepEqual(
    JSON.parse(JSON.stringify(environment)),
    {
      capabilities: { domains: ["shell", "storage"] },
      services: ["settings"]
    }
  );
  assert.equal(context.napplet.shell.supports("storage"), true);
  assert.equal(context.napplet.shell.supports("unknown"), false);
  assert.deepEqual(Array.from(context.napplet.shell.services), ["settings"]);
  assert.equal(callbackCount, 1);

  listeners.get("message")({
    source: parent,
    data: {
      type: "shell.init",
      capabilities: { domains: ["shell", "theme"] },
      services: ["mutated"]
    }
  });
  assert.equal(context.napplet.shell.supports("theme"), false);
  assert.deepEqual(Array.from(context.napplet.shell.services), ["settings"]);
  assert.equal(sent.length, 1, "shell.init never causes another shell.ready");
});

test("prelude request envelopes use pinned flat fields and id correlation", async () => {
  const listeners = new Map();
  const sent = [];
  const parent = {
    postMessage(envelope, target) {
      sent.push({ envelope: JSON.parse(JSON.stringify(envelope)), target });
    }
  };
  const context = {
    Map,
    Object,
    Promise,
    Set,
    Array,
    Number,
    TypeError,
    RangeError,
    Error,
    parent,
    queueMicrotask,
    setTimeout,
    clearTimeout,
    addEventListener(type, listener) {
      listeners.set(type, listener);
    }
  };
  context.window = context;
  vm.runInNewContext(shell.compatibilityPreludeSource(), context);

  const pending = context.napplet.shell.ping({
    source: "fixture",
    type: "forged.type",
    id: "forged-id"
  });
  assert.deepEqual(sent[1], {
    envelope: {
      type: "shell.ping",
      id: "request-1",
      source: "fixture"
    },
    target: "*"
  });
  listeners.get("message")({
    source: parent,
    data: {
      type: "shell.ping.result",
      id: "request-1",
      result: { ok: true }
    }
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(await pending)),
    { ok: true }
  );
});

test("storage projection matches the exact pinned async shim surface", async () => {
  const listeners = new Map();
  const sent = [];
  const parent = {
    postMessage(envelope) {
      sent.push(JSON.parse(JSON.stringify(envelope)));
    }
  };
  const context = {
    Map,
    Object,
    Promise,
    Set,
    Array,
    Number,
    TypeError,
    RangeError,
    Error,
    parent,
    queueMicrotask,
    setTimeout,
    clearTimeout,
    addEventListener(type, listener) {
      listeners.set(type, listener);
    }
  };
  context.window = context;
  vm.runInNewContext(
    shell.compatibilityPreludeSource(["storage", "shell"]),
    context
  );

  assert.deepEqual(Object.keys(context.napplet).sort(), ["shell", "storage"]);
  assert.deepEqual(
    Object.keys(context.napplet.storage).sort(),
    ["getItem", "instance", "keys", "removeItem", "setItem"]
  );

  const shared = context.napplet.storage.getItem("theme");
  assert.deepEqual(sent[1], {
    type: "storage.get",
    id: "request-1",
    key: "theme"
  });
  listeners.get("message")({
    source: parent,
    data: {
      type: "storage.get.result",
      id: "request-1",
      value: "dark"
    }
  });
  assert.equal(await shared, "dark");

  const instance = context.napplet.storage.instance.setItem("draft", "hello");
  assert.deepEqual(sent[2], {
    type: "storage.set",
    id: "request-2",
    key: "draft",
    value: "hello",
    scope: "instance"
  });
  listeners.get("message")({
    source: parent,
    data: {
      type: "storage.set.result",
      id: "request-2"
    }
  });
  assert.equal(await instance, undefined);
});

test("prelude refuses domains it cannot faithfully project", () => {
  assert.throws(
    () => shell.compatibilityPreludeSource(["shell", "surface"]),
    /cannot project every negotiated domain/
  );
});

test("identity projection matches the pinned callable surface and push lifecycle", async () => {
  const harness = createPreludeHarness(["identity"]);
  const identity = harness.context.napplet.identity;

  assert.deepEqual(
    Object.keys(harness.context.napplet).sort(),
    ["identity", "shell"]
  );
  assert.deepEqual(
    Object.keys(identity).sort(),
    [
      "getBadges",
      "getBlocked",
      "getFollows",
      "getList",
      "getMutes",
      "getProfile",
      "getPublicKey",
      "getRelays",
      "getZaps",
      "onChanged"
    ]
  );

  const publicKey = identity.getPublicKey();
  assert.deepEqual(harness.sent[1], {
    envelope: {
      type: "identity.getPublicKey",
      id: "request-1"
    },
    target: "*"
  });
  harness.receive({
    type: "identity.getPublicKey.result",
    id: "request-1",
    pubkey: "a".repeat(64)
  });
  assert.equal(await publicKey, "a".repeat(64));

  const follows = identity.getFollows();
  harness.receive({
    type: "identity.getFollows.result",
    id: "request-2",
    pubkeys: ["b".repeat(64)]
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(await follows)),
    ["b".repeat(64)]
  );

  const list = identity.getList("bookmarks");
  assert.deepEqual(harness.sent[3], {
    envelope: {
      type: "identity.getList",
      id: "request-3",
      listType: "bookmarks"
    },
    target: "*"
  });
  harness.receive({
    type: "identity.getList.result",
    id: "request-3",
    entries: ["note1example"]
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(await list)),
    ["note1example"]
  );

  let changed = [];
  const subscription = identity.onChanged((pubkey) => {
    changed.push(pubkey);
  });
  harness.receive(
    { type: "identity.changed", pubkey: "spoofed" },
    {}
  );
  harness.receive({ type: "identity.changed", pubkey: 42 });
  harness.receive({ type: "identity.changed", pubkey: "c".repeat(64) });
  assert.deepEqual(changed, ["c".repeat(64)]);
  subscription.close();
  subscription.close();
  harness.receive({ type: "identity.changed", pubkey: "" });
  assert.deepEqual(changed, ["c".repeat(64)]);

  const rejected = identity.getProfile();
  harness.receive({
    type: "identity.getProfile.result",
    id: "request-4",
    profile: null,
    error: "profile query refused"
  });
  await assert.rejects(rejected, /profile query refused/);
});

test("prelude refuses unbounded local event handlers before registration", () => {
  const harness = createPreludeHarness(["identity"]);
  const subscriptions = [];
  for (let index = 0; index < 128; index += 1) {
    subscriptions.push(
      harness.context.napplet.identity.onChanged(() => {})
    );
  }
  assert.throws(
    () => harness.context.napplet.identity.onChanged(() => {}),
    /event handler capacity is full/
  );
  subscriptions.forEach((subscription) => subscription.close());
});

test("INC topic projection preserves legacy emit shape and exact subscription teardown", async () => {
  const harness = createPreludeHarness(["inc"]);
  const inc = harness.context.napplet.inc;

  assert.deepEqual(Object.keys(inc).sort(), ["channel", "emit", "on"]);
  assert.deepEqual(
    Object.keys(inc.channel).sort(),
    ["broadcast", "list", "open"]
  );

  inc.emit("profile:open", [], JSON.stringify({ pubkey: "abc" }));
  inc.emit("state:update", { enabled: true });
  assert.deepEqual(harness.sent.slice(1, 3), [
    {
      envelope: {
        type: "inc.emit",
        topic: "profile:open",
        payload: { pubkey: "abc" }
      },
      target: "*"
    },
    {
      envelope: {
        type: "inc.emit",
        topic: "state:update",
        payload: { enabled: true }
      },
      target: "*"
    }
  ]);

  const calls = [];
  const first = inc.on("profile:open", (payload, event) => {
    calls.push({ payload, event });
  });
  const second = inc.on("profile:open", () => {
    calls.push({ second: true });
  });
  assert.deepEqual(harness.sent[3], {
    envelope: {
      type: "inc.subscribe",
      id: "request-1",
      topic: "profile:open"
    },
    target: "*"
  });
  assert.equal(
    harness.sent.filter((item) => item.envelope.type === "inc.subscribe").length,
    1,
    "one shell subscription backs local fan-out for the same topic"
  );
  harness.receive({ type: "inc.subscribe.result", id: "request-1" });
  harness.receive({
    type: "inc.event",
    topic: "profile:open",
    sender: "sender-d-tag",
    payload: { pubkey: "def" }
  });
  assert.equal(calls.length, 2);
  assert.deepEqual(
    JSON.parse(JSON.stringify(calls[0].payload)),
    { pubkey: "def" }
  );
  assert.equal(calls[0].event.pubkey, "sender-d-tag");
  assert.equal(calls[0].event.created_at, 0);
  assert.deepEqual(
    JSON.parse(JSON.stringify(calls[0].event.tags)),
    [["t", "profile:open"]]
  );

  first.close();
  assert.equal(
    harness.sent.filter((item) => item.envelope.type === "inc.unsubscribe").length,
    0
  );
  second.close();
  second.close();
  assert.deepEqual(harness.sent[4], {
    envelope: {
      type: "inc.unsubscribe",
      topic: "profile:open"
    },
    target: "*"
  });
});

test("INC channel projection correlates open/list and routes only owned channel pushes", async () => {
  const harness = createPreludeHarness(["inc"]);
  const inc = harness.context.napplet.inc;

  const opening = inc.channel.open("peer-d-tag");
  assert.deepEqual(harness.sent[1], {
    envelope: {
      type: "inc.channel.open",
      id: "request-1",
      target: "peer-d-tag"
    },
    target: "*"
  });
  harness.receive({
    type: "inc.channel.open.result",
    id: "request-1",
    channelId: "channel-1",
    peer: "peer-d-tag"
  });
  const channel = await opening;
  assert.deepEqual(
    Object.keys(channel).sort(),
    ["close", "emit", "id", "on", "peer"]
  );

  const events = [];
  const subscription = channel.on((event) => {
    events.push(event);
  });
  harness.receive({
    type: "inc.channel.event",
    channelId: "unowned",
    sender: "attacker",
    payload: "ignored"
  });
  harness.receive({
    type: "inc.channel.event",
    channelId: "channel-1",
    sender: "peer-d-tag",
    payload: { state: "ready" }
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(events)),
    [{
      channelId: "channel-1",
      sender: "peer-d-tag",
      payload: { state: "ready" }
    }]
  );

  channel.emit({ command: "play" });
  inc.channel.broadcast({ command: "stop" });
  assert.deepEqual(harness.sent.slice(2, 4), [
    {
      envelope: {
        type: "inc.channel.emit",
        channelId: "channel-1",
        payload: { command: "play" }
      },
      target: "*"
    },
    {
      envelope: {
        type: "inc.channel.broadcast",
        payload: { command: "stop" }
      },
      target: "*"
    }
  ]);

  const listing = inc.channel.list();
  harness.receive({
    type: "inc.channel.list.result",
    id: "request-2",
    channels: [{ id: "channel-1", peer: "peer-d-tag" }]
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(await listing)),
    [{ id: "channel-1", peer: "peer-d-tag" }]
  );

  subscription.close();
  channel.close();
  channel.close();
  assert.deepEqual(harness.sent[5], {
    envelope: {
      type: "inc.channel.close",
      channelId: "channel-1"
    },
    target: "*"
  });
  assert.throws(
    () => channel.emit("after-close"),
    /channel is closed/
  );
});

test("theme projection matches the pinned get and automatic-change surface", async () => {
  const harness = createPreludeHarness(["theme"]);
  const theme = harness.context.napplet.theme;
  const firstTheme = {
    colors: {
      background: "#1a1a2e",
      text: "#e0e0e0",
      primary: "#6c3ce0"
    },
    title: "Pinned"
  };

  assert.deepEqual(
    Object.keys(harness.context.napplet).sort(),
    ["shell", "theme"]
  );
  assert.deepEqual(Object.keys(theme).sort(), ["get", "onChanged"]);

  const current = theme.get();
  assert.deepEqual(harness.sent[1], {
    envelope: {
      type: "theme.get",
      id: "request-1"
    },
    target: "*"
  });
  harness.receive(
    {
      type: "theme.get.result",
      id: "request-1",
      theme: { colors: {} }
    },
    {}
  );
  harness.receive({
    type: "theme.get.result",
    id: "request-1",
    theme: firstTheme
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(await current)),
    firstTheme
  );

  const changes = [];
  const subscription = theme.onChanged((value) => changes.push(value));
  harness.receive(
    { type: "theme.changed", theme: firstTheme },
    {}
  );
  harness.receive({ type: "theme.changed", theme: "invalid" });
  harness.receive({ type: "theme.changed", theme: firstTheme });
  assert.deepEqual(
    JSON.parse(JSON.stringify(changes)),
    [firstTheme]
  );
  subscription.close();
  subscription.close();
  harness.receive({ type: "theme.changed", theme: firstTheme });
  assert.equal(changes.length, 1);

  const rejected = theme.get();
  harness.receive({
    type: "theme.get.result",
    id: "request-2",
    error: "no active theme"
  });
  await assert.rejects(rejected, /no active theme/);
});

test("outbox projection supports bounded query, publish, and subscription lifecycles", async () => {
  const absent = createPreludeHarness([]);
  assert.equal(absent.context.napplet.outbox, undefined);

  const harness = createPreludeHarness(["outbox"]);
  const outbox = harness.context.napplet.outbox;
  assert.deepEqual(
    Object.keys(outbox).sort(),
    ["getEvent", "publish", "query", "resolveRelays", "subscribe"]
  );

  const query = outbox.query(
    [{ kinds: [1], authors: ["a".repeat(64)], limit: 20 }],
    { authors: ["a".repeat(64)], timeoutMs: 1000 }
  );
  const queryEnvelope = harness.sent.at(-1).envelope;
  assert.equal(queryEnvelope.type, "outbox.query");
  harness.receive({
    type: "outbox.query.result",
    id: queryEnvelope.id,
    events: [],
    incomplete: true,
    error: "one relay is unavailable"
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(await query)),
    { events: [], incomplete: true, error: "one relay is unavailable" }
  );

  const bounded = outbox.query([{ kinds: [1] }], { timeoutMs: 1000 });
  const boundedEnvelope = harness.sent.at(-1).envelope;
  harness.receive({
    type: "outbox.query.result",
    id: boundedEnvelope.id,
    events: [],
    incomplete: true,
    reason: "query event bound reached (1024 events)"
  });
  assert.deepEqual(JSON.parse(JSON.stringify(await bounded)), {
    events: [],
    incomplete: true,
    reason: "query event bound reached (1024 events)"
  });

  const received = [];
  const closed = [];
  const subscription = outbox.subscribe({ kinds: [1] });
  const subscribeEnvelope = harness.sent.at(-1).envelope;
  subscription.on("event", (result) => received.push(result.event.id));
  subscription.on("closed", (reason) => closed.push(reason));
  harness.receive({
    type: "outbox.event",
    subId: subscribeEnvelope.subId,
    result: { event: { id: "event-1" } }
  });
  harness.receive({
    type: "outbox.closed",
    subId: subscribeEnvelope.subId,
    reason: "upstream closed"
  });
  assert.deepEqual(received, ["event-1"]);
  assert.deepEqual(closed, ["upstream closed"]);

  const publish = outbox.publish({
    kind: 1,
    content: "GM",
    tags: [],
    created_at: 1
  });
  const publishEnvelope = harness.sent.at(-1).envelope;
  harness.receive({
    type: "outbox.publish.result",
    id: publishEnvelope.id,
    ok: true,
    eventId: "signed-event",
    relays: { "wss://relay.example": true }
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(await publish)),
    {
      ok: true,
      eventId: "signed-event",
      relays: { "wss://relay.example": true }
    }
  );
});

test("relay projection preserves event, EOSE, query, and governed publish results", async () => {
  const harness = createPreludeHarness(["relay"]);
  const relay = harness.context.napplet.relay;
  const events = [];
  let eose = 0;
  const subscription = relay.subscribe(
    { kinds: [1] },
    (result) => events.push(result.event.id),
    () => { eose += 1; }
  );
  const subscribeEnvelope = harness.sent.at(-1).envelope;
  harness.receive({
    type: "relay.event",
    subId: subscribeEnvelope.subId,
    result: { event: { id: "relay-event" } }
  });
  harness.receive({ type: "relay.eose", subId: subscribeEnvelope.subId });
  harness.receive({ type: "relay.eose", subId: subscribeEnvelope.subId });
  assert.deepEqual(events, ["relay-event"]);
  assert.equal(eose, 1);
  subscription.close();
  assert.equal(harness.sent.at(-1).envelope.type, "relay.close");

  const query = relay.query({ kinds: [0] });
  const queryEnvelope = harness.sent.at(-1).envelope;
  harness.receive({
    type: "relay.query.result",
    id: queryEnvelope.id,
    events: [{ event: { id: "profile" } }],
    incomplete: true
  });
  const queryResult = await query;
  assert.equal(queryResult[0].event.id, "profile");
  assert.equal(queryResult.incomplete, true);
  assert.equal(queryResult.error, undefined);

  const bounded = relay.query({ kinds: [1] });
  const boundedEnvelope = harness.sent.at(-1).envelope;
  harness.receive({
    type: "relay.query.result",
    id: boundedEnvelope.id,
    events: [{ event: { id: "bounded" } }],
    incomplete: true,
    reason: "query event bound reached (1024 events)"
  });
  const boundedResult = await bounded;
  assert.equal(boundedResult.incomplete, true);
  assert.equal(boundedResult.reason, "query event bound reached (1024 events)");

  const publish = relay.publish({
    kind: 1,
    content: "hello",
    tags: [],
    created_at: 1
  });
  const publishEnvelope = harness.sent.at(-1).envelope;
  harness.receive({
    type: "relay.publish.result",
    id: publishEnvelope.id,
    ok: true,
    event: { id: "signed" }
  });
  assert.equal((await publish).id, "signed");

  const encrypted = relay.publishEncrypted(
    { kind: 4, content: "secret", tags: [], created_at: 1 },
    "b".repeat(64)
  );
  const encryptedEnvelope = harness.sent.at(-1).envelope;
  harness.receive({
    type: "relay.publishEncrypted.result",
    id: encryptedEnvelope.id,
    ok: false,
    error: "governed content encryption unavailable"
  });
  await assert.rejects(encrypted, /governed content encryption unavailable/);
});

test("resource projection stays unavailable until matching shell.init", async () => {
  const absent = createPreludeHarness();
  assert.equal(absent.context.napplet.resource, undefined);
  assert.equal(absent.context.napplet.shell.supports("resource"), false);

  const harness = createPreludeHarness(["resource"]);
  const resource = harness.context.napplet.resource;
  assert.deepEqual(
    Object.keys(resource).sort(),
    ["bytes", "bytesAsObjectURL", "bytesMany", "info"]
  );
  await assert.rejects(
    resource.info(),
    /unavailable before shell.init/
  );
  assert.equal(harness.sent.length, 1);

  harness.receive({
    type: "shell.init",
    capabilities: { domains: ["shell"] },
    services: []
  });
  assert.equal(harness.context.napplet.shell.supports("resource"), false);
  await assert.rejects(
    resource.bytes("data:text/plain,hello"),
    /unavailable before shell.init/
  );

  harness.receive({
    type: "shell.init",
    capabilities: { domains: ["resource", "shell"] },
    services: []
  });
  assert.equal(harness.context.napplet.shell.supports("resource"), true);

  const info = resource.info();
  assert.deepEqual(harness.sent[1], {
    envelope: {
      type: "resource.info",
      id: "request-1"
    },
    target: "*"
  });
  harness.receive({
    type: "resource.info.result",
    id: "request-1",
    info: {
      schemes: [
        { scheme: "data", enabled: true },
        { scheme: "https", enabled: true },
        { scheme: "blossom", enabled: true }
      ],
      maxBytes: 10 * 1024 * 1024,
      maxUrls: 100
    }
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(await info)),
    {
      schemes: [
        { scheme: "data", enabled: true },
        { scheme: "https", enabled: true },
        { scheme: "blossom", enabled: true }
      ],
      maxBytes: 10 * 1024 * 1024,
      maxUrls: 100
    }
  );
});

test("resource byte terminals become Blobs before sandbox delivery", async () => {
  const harness = createPreludeHarness(["resource"]);
  harness.receive({
    type: "shell.init",
    capabilities: { domains: ["shell", "resource"] },
    services: []
  });
  const bytes = harness.context.napplet.resource.bytes(
    "data:text/plain;base64,aGVsbG8="
  );
  assert.deepEqual(harness.sent[1], {
    envelope: {
      type: "resource.bytes",
      id: "request-1",
      url: "data:text/plain;base64,aGVsbG8="
    },
    target: "*"
  });

  const terminal = {
    type: "resource.bytes.result",
    id: "request-1",
    blob: "aGVsbG8=",
    mime: "text/plain"
  };
  const projected = shell.projectNativeEnvelope(terminal, true);
  assert.equal(projected.blob instanceof Blob, true);
  assert.equal(projected.blob.type, "text/plain");
  assert.equal(JSON.stringify(projected).includes("aGVsbG8="), false);
  harness.receive(projected);

  const blob = await bytes;
  assert.equal(blob instanceof Blob, true);
  assert.equal(blob.type, "text/plain");
  assert.equal(await blob.text(), "hello");
  assert.equal(
    shell.projectNativeEnvelope(terminal, false),
    null,
    "resource terminals are not delivered to an unadvertised sandbox"
  );
});

test("resource bulk projection preserves order and per-item failures", async () => {
  const harness = createPreludeHarness(["resource"]);
  harness.receive({
    type: "shell.init",
    capabilities: { domains: ["resource", "shell"] },
    services: []
  });
  const urls = [
    "data:text/plain;base64,b25l",
    "http://blocked.example/two",
    "data:text/plain;base64,dGhyZWU="
  ];
  const bulk = harness.context.napplet.resource.bytesMany(urls);
  assert.deepEqual(harness.sent[1], {
    envelope: {
      type: "resource.bytesMany",
      id: "request-1",
      urls
    },
    target: "*"
  });

  const projected = shell.projectNativeEnvelope({
    type: "resource.bytesMany.result",
    id: "request-1",
    items: [
      {
        url: urls[0],
        ok: true,
        blob: "b25l",
        mime: "text/plain"
      },
      {
        url: urls[1],
        ok: false,
        error: "unsupported-scheme",
        message: "only data, https, and blossom are supported"
      },
      {
        url: urls[2],
        ok: true,
        blob: "dGhyZWU=",
        mime: "text/plain"
      }
    ]
  }, true);
  assert.equal(projected.items[0].blob instanceof Blob, true);
  assert.equal(projected.items[2].blob instanceof Blob, true);
  assert.notEqual(projected.items[2].blob, "dGhyZWU=");
  harness.receive(projected);

  const items = await bulk;
  assert.equal(items.length, 3);
  assert.equal(await items[0].blob.text(), "one");
  assert.deepEqual(
    JSON.parse(JSON.stringify(items[1])),
    {
      url: urls[1],
      ok: false,
      error: "unsupported-scheme",
      message: "only data, https, and blossom are supported"
    }
  );
  assert.equal(await items[2].blob.text(), "three");
});

test("resource bulk input reads at most one over the Rust-owned URL limit", async () => {
  const harness = createPreludeHarness(["resource"]);
  harness.receive({
    type: "shell.init",
    capabilities: { domains: ["resource", "shell"] },
    services: []
  });
  let iteratorClosed = false;
  function *manyURLs() {
    try {
      for (let index = 0; index < 10_000; index += 1) {
        yield `https://images.example/${index}`;
      }
    } finally {
      iteratorClosed = true;
    }
  }

  const bulk = harness.context.napplet.resource.bytesMany(manyURLs());
  assert.equal(harness.sent[1].envelope.urls.length, 101);
  assert.equal(iteratorClosed, true);
  harness.receive({
    type: "resource.bytesMany.error",
    id: "request-1",
    error: "too-large",
    message: "bulk URL count exceeds its limit"
  });
  await assert.rejects(bulk, /too-large/);
});

test("resource errors and malformed or oversized terminals refuse explicitly", async () => {
  const harness = createPreludeHarness(["resource"]);
  harness.receive({
    type: "shell.init",
    capabilities: { domains: ["resource", "shell"] },
    services: []
  });

  const failed = harness.context.napplet.resource.bytes(
    "https://blocked.example/image"
  );
  harness.receive({
    type: "resource.bytes.error",
    id: "request-1",
    error: "blocked-by-policy",
    message: "resolved address is not public"
  });
  await assert.rejects(
    failed,
    /blocked-by-policy: resolved address is not public/
  );

  const malformed = harness.context.napplet.resource.bytes(
    "data:text/plain;base64,broken"
  );
  const malformedTerminal = shell.projectNativeEnvelope({
    type: "resource.bytes.result",
    id: "request-2",
    blob: "not-padded-base64",
    mime: "text/plain"
  }, true);
  assert.deepEqual(malformedTerminal, {
    type: "resource.bytes.error",
    id: "request-2",
    error: "decode-failed",
    message: "resource bytes were not standard padded base64"
  });
  harness.receive(malformedTerminal);
  await assert.rejects(malformed, /decode-failed/);

  const oversized = shell.projectResourceTerminal({
    type: "resource.bytes.result",
    id: "small-test-bound",
    blob: "YWJj",
    mime: "text/plain"
  }, 2);
  assert.deepEqual(oversized, {
    type: "resource.bytes.error",
    id: "small-test-bound",
    error: "too-large",
    message: "resource Blob exceeds the trusted shell byte limit"
  });

  const invalidBlob = harness.context.napplet.resource.bytes(
    "data:text/plain,invalid-parent-terminal"
  );
  harness.receive({
    type: "resource.bytes.result",
    id: "request-3",
    blob: "raw-base64-must-not-cross",
    mime: "text/plain"
  });
  await assert.rejects(invalidBlob, /invalid resource Blob/);
  assert.deepEqual(
    harness.sent.at(-1).envelope,
    { type: "resource.cancel", id: "request-3" }
  );
});

test("resource teardown cancels pending work and revokes bounded object URLs", async () => {
  const harness = createPreludeHarness(["resource"]);
  harness.receive({
    type: "shell.init",
    capabilities: { domains: ["resource", "shell"] },
    services: []
  });
  const resource = harness.context.napplet.resource;
  const handle = resource.bytesAsObjectURL(
    "data:text/plain;base64,b2JqZWN0"
  );
  const projected = shell.projectNativeEnvelope({
    type: "resource.bytes.result",
    id: "request-1",
    blob: "b2JqZWN0",
    mime: "text/plain"
  }, true);
  harness.receive(projected);
  assert.equal(await handle.ready, "blob:trusted-shell-1");
  assert.equal(handle.url, "blob:trusted-shell-1");

  const firstPending = resource.bytes("https://images.example/one");
  const secondPending = resource.bytesMany([
    "https://images.example/two",
    "https://images.example/three"
  ]);
  harness.listeners.get("pagehide")();
  const settled = await Promise.allSettled([firstPending, secondPending]);
  assert.equal(
    settled.every((result) =>
      result.status === "rejected" &&
      /session is closed/.test(result.reason.message)
    ),
    true
  );
  assert.deepEqual(
    harness.sent.slice(-2).map((item) => item.envelope),
    [
      { type: "resource.cancel", id: "request-2" },
      { type: "resource.cancel", id: "request-3" }
    ]
  );
  assert.deepEqual(
    harness.objectURLs.revoked,
    ["blob:trusted-shell-1"]
  );

  harness.receive({
    type: "resource.bytes.result",
    id: "request-2",
    blob: new Blob(["late"], { type: "text/plain" }),
    mime: "text/plain"
  });
  handle.revoke();
  assert.deepEqual(
    harness.objectURLs.revoked,
    ["blob:trusted-shell-1"],
    "late terminals and repeated revocation are inert"
  );
});

test("link projection matches the pinned open surface and shell.init gate", async () => {
  const absent = createPreludeHarness();
  assert.equal(absent.context.napplet.link, undefined);

  const harness = createPreludeHarness(["link"]);
  const link = harness.context.napplet.link;
  assert.deepEqual(Object.keys(link), ["open"]);
  await assert.rejects(
    link.open("https://example.com/post/1"),
    /unavailable before shell.init/
  );
  assert.equal(harness.sent.length, 1);

  harness.receive({
    type: "shell.init",
    capabilities: { domains: ["shell"] },
    services: []
  });
  await assert.rejects(
    link.open("https://example.com/post/1"),
    /unavailable before shell.init/
  );

  harness.receive({
    type: "shell.init",
    capabilities: { domains: ["link", "shell"] },
    services: []
  });
  assert.equal(harness.context.napplet.shell.supports("link"), true);

  const opened = link.open(
    "https://example.com/post/1",
    { label: "Read post" }
  );
  assert.deepEqual(harness.sent[1], {
    envelope: {
      type: "link.open",
      id: "request-1",
      url: "https://example.com/post/1",
      options: { label: "Read post" }
    },
    target: "*"
  });
  harness.receive({
    type: "link.open.result",
    id: "request-1",
    status: "opened"
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(await opened)),
    { status: "opened" }
  );

  const denied = link.open("https://example.com/post/2");
  assert.deepEqual(harness.sent[2], {
    envelope: {
      type: "link.open",
      id: "request-2",
      url: "https://example.com/post/2"
    },
    target: "*"
  });
  harness.receive({
    type: "link.open.result",
    id: "request-2",
    status: "denied",
    error: "user-denied"
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(await denied)),
    { status: "denied" },
    "the pinned public result contains only status"
  );
});

test("link projection bounds wire fields without taking URL policy from Rust", async () => {
  const harness = createPreludeHarness(["link"]);
  harness.receive({
    type: "shell.init",
    capabilities: { domains: ["link", "shell"] },
    services: []
  });
  const link = harness.context.napplet.link;
  const sentBeforeRefusals = harness.sent.length;

  await assert.rejects(
    link.open("🙂".repeat(2_049)),
    /bounded non-empty string URL/
  );
  await assert.rejects(
    link.open("https://example.com", {
      label: "🙂".repeat(1_025)
    }),
    /bounded string label/
  );
  await assert.rejects(
    link.open("https://example.com", { target: "_blank" }),
    /only a bounded string label/
  );
  await assert.rejects(
    link.open({ href: "https://example.com" }),
    /bounded non-empty string URL/
  );
  assert.equal(harness.sent.length, sentBeforeRefusals);

  const failed = link.open("javascript:must-be-decided-by-rust");
  assert.equal(
    harness.sent.at(-1).envelope.url,
    "javascript:must-be-decided-by-rust",
    "the shell validates transport bounds but does not own scheme policy"
  );
  harness.receive({
    type: "link.open.result",
    id: "request-1",
    status: "failed",
    error: "unsupported-scheme"
  });
  await assert.rejects(failed, /unsupported-scheme/);

  const malformed = link.open("https://example.com/malformed");
  harness.receive({
    type: "link.open.result",
    id: "request-2",
    status: "unknown"
  });
  await assert.rejects(malformed, /link open failed/);
});

test("link correlations are finite and teardown rejects late terminals", async () => {
  const harness = createPreludeHarness(["link"]);
  harness.receive({
    type: "shell.init",
    capabilities: { domains: ["link", "shell"] },
    services: []
  });
  const pending = [];
  for (let index = 0; index < 128; index += 1) {
    pending.push(
      harness.context.napplet.link.open(
        `https://example.com/pending/${index}`
      )
    );
  }
  await assert.rejects(
    harness.context.napplet.link.open(
      "https://example.com/over-capacity"
    ),
    /request capacity is full/
  );
  assert.equal(
    harness.sent.filter((item) =>
      item.envelope.type === "link.open"
    ).length,
    128
  );

  harness.listeners.get("pagehide")();
  const settled = await Promise.allSettled(pending);
  assert.equal(
    settled.every((result) =>
      result.status === "rejected" &&
      /session is closed/.test(result.reason.message)
    ),
    true
  );
  assert.equal(
    harness.sent.some((item) =>
      item.envelope.type === "link.cancel"
    ),
    false,
    "the pinned wire contract has no invented link cancellation action"
  );
  harness.receive({
    type: "link.open.result",
    id: "request-1",
    status: "opened"
  });
  await assert.rejects(
    harness.context.napplet.link.open("https://example.com/after-close"),
    /session is closed/
  );

  const source = fs.readFileSync(
    path.join(__dirname, "..", "trusted-shell.js"),
    "utf8"
  );
  assert.doesNotMatch(source, /window\.open\s*\(/);
  assert.doesNotMatch(source, /location(?:\.href)?\s*=/);
});

test("config projection matches the pinned schema, snapshot, subscription, and settings surface", async () => {
  const manifestSchema = {
    type: "object",
    properties: {
      enabled: { type: "boolean", default: true }
    },
    additionalProperties: false
  };
  const harness = createPreludeHarness(["config"], manifestSchema);
  const config = harness.context.napplet.config;

  assert.deepEqual(
    Object.keys(harness.context.napplet).sort(),
    ["config", "shell"]
  );
  assert.deepEqual(
    Object.keys(config).sort(),
    [
      "get",
      "onSchemaError",
      "openSettings",
      "registerSchema",
      "schema",
      "subscribe"
    ]
  );
  assert.deepEqual(
    JSON.parse(JSON.stringify(config.schema)),
    manifestSchema
  );

  const nextSchema = {
    type: "object",
    properties: {
      mode: { type: "string", enum: ["quiet", "loud"], default: "quiet" }
    },
    additionalProperties: false
  };
  const registration = config.registerSchema(nextSchema, 2);
  assert.deepEqual(harness.sent[1], {
    envelope: {
      type: "config.registerSchema",
      id: "request-1",
      schema: nextSchema,
      version: 2
    },
    target: "*"
  });
  harness.receive({
    type: "config.registerSchema.result",
    id: "request-1",
    ok: true
  });
  assert.equal(await registration, undefined);
  assert.deepEqual(
    JSON.parse(JSON.stringify(config.schema)),
    nextSchema
  );

  const rejected = config.registerSchema({ type: "array" });
  harness.receive({
    type: "config.registerSchema.result",
    id: "request-2",
    ok: false,
    code: "invalid-schema",
    error: "root must be an object"
  });
  await assert.rejects(
    rejected,
    /invalid-schema: root must be an object/
  );

  const snapshot = config.get();
  assert.deepEqual(harness.sent[3], {
    envelope: {
      type: "config.get",
      id: "request-3"
    },
    target: "*"
  });
  harness.receive({
    type: "config.values",
    id: "request-3",
    values: { mode: "quiet" }
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(await snapshot)),
    { mode: "quiet" }
  );

  const firstValues = [];
  const secondValues = [];
  const first = config.subscribe((values) => firstValues.push(values));
  const second = config.subscribe((values) => secondValues.push(values));
  assert.equal(
    harness.sent.filter((item) =>
      item.envelope.type === "config.subscribe"
    ).length,
    1
  );
  await Promise.resolve();
  assert.deepEqual(
    JSON.parse(JSON.stringify(secondValues)),
    [{ mode: "quiet" }],
    "late local subscribers receive the cached snapshot"
  );
  harness.receive({
    type: "config.values",
    values: { mode: "loud" }
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(firstValues)),
    [{ mode: "loud" }]
  );
  assert.deepEqual(
    JSON.parse(JSON.stringify(secondValues)),
    [{ mode: "quiet" }, { mode: "loud" }]
  );

  first.close();
  assert.equal(
    harness.sent.filter((item) =>
      item.envelope.type === "config.unsubscribe"
    ).length,
    0
  );
  second.close();
  second.close();
  assert.equal(
    harness.sent.filter((item) =>
      item.envelope.type === "config.unsubscribe"
    ).length,
    1
  );

  config.openSettings({ section: "appearance" });
  config.openSettings();
  assert.deepEqual(
    harness.sent.slice(-2).map((item) => item.envelope),
    [
      { type: "config.openSettings", section: "appearance" },
      { type: "config.openSettings" }
    ]
  );
});

test("config pushes are parent-bound and teardown returns the wire subscription", async () => {
  const harness = createPreludeHarness(["config"]);
  const config = harness.context.napplet.config;
  const values = [];
  const errors = [];
  config.subscribe((value) => values.push(value));
  const offError = config.onSchemaError((error) => errors.push(error));

  harness.receive(
    { type: "config.values", values: { forged: true } },
    {}
  );
  harness.receive(
    {
      type: "config.schemaError",
      code: "no-schema",
      error: "forged"
    },
    {}
  );
  harness.receive({ type: "config.values", values: { enabled: true } });
  harness.receive({
    type: "config.schemaError",
    code: "no-schema",
    error: "no schema is registered"
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(values)),
    [{ enabled: true }]
  );
  assert.deepEqual(
    JSON.parse(JSON.stringify(errors)),
    [{ code: "no-schema", error: "no schema is registered" }]
  );

  const pending = config.get();
  harness.listeners.get("pagehide")();
  await assert.rejects(pending, /session is closed/);
  assert.deepEqual(
    harness.sent.slice(-1).map((item) => item.envelope),
    [{ type: "config.unsubscribe" }]
  );
  offError();
});

test("embedded manifest config schemas are bounded and script-safe", () => {
  const schema = {
    type: "object",
    description: "</script><script>globalThis.compromised=true</script>"
  };
  const source = shell.compatibilityPreludeSource(["config"], schema);
  assert.equal(source.includes("</script>"), false);
  assert.match(source, /\\u003c\/script>/);

  const harness = createPreludeHarness(["config"], schema);
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.context.napplet.config.schema)),
    schema
  );
});

test("prelude bounds correlations and returns subscriptions on teardown", async () => {
  const harness = createPreludeHarness(["identity", "inc"]);
  const subscription = harness.context.napplet.inc.on("state", () => {});
  harness.receive({ type: "inc.subscribe.result", id: "request-1" });

  const opening = harness.context.napplet.inc.channel.open("peer");
  harness.receive({
    type: "inc.channel.open.result",
    id: "request-2",
    channelId: "channel-1",
    peer: "peer"
  });
  await opening;

  const pending = [];
  for (let index = 0; index < 128; index += 1) {
    pending.push(harness.context.napplet.identity.getPublicKey());
  }
  await assert.rejects(
    harness.context.napplet.identity.getPublicKey(),
    /request capacity is full/
  );

  harness.listeners.get("pagehide")();
  const settled = await Promise.allSettled(pending);
  assert.equal(
    settled.every((result) =>
      result.status === "rejected" &&
      /session is closed/.test(result.reason.message)
    ),
    true
  );
  assert.deepEqual(
    harness.sent.slice(-2).map((item) => item.envelope),
    [
      { type: "inc.unsubscribe", topic: "state" },
      { type: "inc.channel.close", channelId: "channel-1" }
    ]
  );
  subscription.close();
  await assert.rejects(
    harness.context.napplet.identity.getPublicKey(),
    /session is closed/
  );
});

test("the Apple package snapshot exactly matches canonical trusted-shell bytes", () => {
  const canonicalRoot = path.join(__dirname, "..");
  const packagedRoot = path.join(
    __dirname,
    "..",
    "..",
    "..",
    "platforms",
    "apple",
    "Sources",
    "NMPNativeRuntimeApple",
    "Resources",
    "TrustedShell"
  );
  const relativeFiles = [
    "trusted-shell.html",
    "trusted-shell.css",
    "trusted-shell-policy.js",
    "trusted-shell.js",
    path.join("fixtures", "minimal-conformant-napplet.html"),
    path.join("fixtures", "external-assets", "index.html"),
    path.join("fixtures", "external-assets", "styles", "site.css"),
    path.join("fixtures", "external-assets", "scripts", "boot.js"),
    path.join("fixtures", "external-assets", "images", "verified.svg")
  ];

  for (const relativeFile of relativeFiles) {
    assert.equal(
      fs.readFileSync(path.join(packagedRoot, relativeFile), "utf8"),
      fs.readFileSync(path.join(canonicalRoot, relativeFile), "utf8"),
      `${relativeFile} must be refreshed with platforms/apple/scripts/sync-trusted-shell`
    );
  }
});
