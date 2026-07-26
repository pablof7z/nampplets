"use strict";

const assert = require("node:assert/strict");
const test = require("node:test");
const vm = require("node:vm");

const shell = require("../trusted-shell.js");

function preludeContext(domains) {
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
    parent: { postMessage() {} },
    queueMicrotask,
    setTimeout,
    clearTimeout,
    addEventListener() {}
  };
  context.window = context;
  vm.runInNewContext(shell.compatibilityPreludeSource(domains), context);
  return context;
}

test("a napplet bundling the published SDK can install its own window.napplet", () => {
  // The published SDK assigns its own postMessage client over the injected
  // facade at module load. ES modules are strict, so a non-writable property
  // throws there and aborts the napplet before it renders anything.
  const context = preludeContext(["relay"]);

  assert.equal(typeof context.napplet.shell.supports, "function");
  vm.runInNewContext(
    '"use strict"; window.napplet = { relay: { subscribe: function () {} } };',
    context
  );
  assert.equal(typeof context.napplet.relay.subscribe, "function");
});

test("the injected facade itself cannot be edited in place", () => {
  const context = preludeContext(["relay"]);

  assert.equal(Object.isFrozen(context.napplet), true);
  assert.equal(Object.isFrozen(context.napplet.shell), true);
});
