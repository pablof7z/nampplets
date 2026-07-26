"use strict";

// The napplet console is the surface standing between a developer and a
// diagnosis, so what it drops is what they never get to see.
//
// WebKit's `Error.prototype.stack` is only the frame list -- unlike V8 it does
// not begin with "Name: message". The shell used to forward `stack || message`,
// which under WKWebView therefore dropped the reason entirely and filled the
// console with rows like "Unhandled rejection: @about:srcdoc:1547:11154": an
// anonymous frame position, no error name, no message, no rejection value. On
// V8 the same expression looks correct, because there the message is already
// inside the stack string. It worked everywhere except the one engine this
// code actually runs on. `forwardConsoleEntry` carried the identical
// expression, so `console.error(err)` lost its message the same way.
//
// Both now go through `describeThrowable`, which builds the headline from
// `name` and `message` and appends the stack only when the stack does not
// already start with it, so V8 output is unchanged and not double-prefixed.
// Non-Error reasons are JSON-described rather than stringified, since a
// rejected `{ code: "invoke-failed" }` previously rendered as "[object
// Object]".
//
// These tests run under node, which is V8, so a WebKit stack has to be
// constructed deliberately. Asserting against a normal Error would pass
// against the old code and prove nothing -- that engine difference is the
// whole defect.

const assert = require("node:assert/strict");
const test = require("node:test");
const vm = require("node:vm");

const shell = require("../trusted-shell.js");

function createConsoleHarness(domains) {
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
    JSON,
    parent,
    queueMicrotask,
    setTimeout,
    clearTimeout,
    addEventListener(type, listener) {
      listeners.set(type, listener);
    }
  };
  context.window = context;
  vm.runInNewContext(shell.compatibilityPreludeSource(domains), context);
  return { context, listeners, sent };
}

function consoleMessages(harness) {
  return harness.sent
    .filter((envelope) => envelope.type === "debug.console")
    .map((envelope) => envelope.message);
}

function webKitError(name, message, frames) {
  const error = new Error(message);
  error.name = name;
  Object.defineProperty(error, "stack", { value: frames, configurable: true });
  return error;
}

test("an unhandled rejection reports the reason, not just a WebKit frame", () => {
  const harness = createConsoleHarness(["identity"]);
  harness.listeners.get("unhandledrejection")({
    reason: webKitError("TypeError", "intent payload missing", "@about:srcdoc:1547:11154")
  });

  const [message] = consoleMessages(harness);
  assert.match(message, /intent payload missing/);
  assert.match(message, /TypeError/);
  assert.match(
    message,
    /@about:srcdoc:1547:11154/,
    "the frame is still useful once the reason is present"
  );
});

test("a V8-style stack is not given a duplicate headline", () => {
  const harness = createConsoleHarness(["identity"]);
  const error = new Error("already prefixed");
  error.name = "RangeError";
  Object.defineProperty(error, "stack", {
    value: "RangeError: already prefixed\n    at somewhere",
    configurable: true
  });
  harness.listeners.get("unhandledrejection")({ reason: error });

  const [message] = consoleMessages(harness);
  assert.equal(
    message,
    "Unhandled rejection: RangeError: already prefixed\n    at somewhere"
  );
});

test("a non-Error rejection reason is described rather than stringified to [object Object]", () => {
  const harness = createConsoleHarness(["identity"]);
  harness.listeners.get("unhandledrejection")({ reason: { code: "invoke-failed" } });

  const [message] = consoleMessages(harness);
  assert.match(message, /invoke-failed/);
  assert.doesNotMatch(message, /\[object Object\]/);
});

test("console.error(error) keeps the message under a WebKit stack", () => {
  const harness = createConsoleHarness(["identity"]);
  // `console` is a global inside the contextified sandbox rather than a
  // property the harness hands back, so the call has to be made in there.
  harness.context.probeError = webKitError(
    "Error",
    "relay handshake refused",
    "@about:srcdoc:12:34"
  );
  vm.runInContext("console.error(probeError)", harness.context);

  const message = consoleMessages(harness).at(-1);
  assert.match(message, /relay handshake refused/);
});
