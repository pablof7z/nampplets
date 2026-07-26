(function trustedShell(global) {
  "use strict";

  const MAX_ENVELOPE_BYTES = 64 * 1024;
  const MAX_RESOURCE_TRANSPORT_BYTES = (100 * 1024 * 1024) + (64 * 1024);
  const MAX_RESOURCE_BLOB_BYTES = 50 * 1024 * 1024;
  const MAX_RESOURCE_ITEMS = 100;
  const MAX_RESOURCE_URL_BYTES = 16 * 1024;
  const MAX_RESOURCE_MIME_BYTES = 256;
  const MAX_CONFIG_SCHEMA_BYTES = 192 * 1024;
  const bridgeEventName = "nmp-native-envelope";
  const policySource = global.NMPTrustedShellPolicy ||
    (typeof require === "function" ? require("./trusted-shell-policy.js") : null);
  const preludeDomainsSource = global.NMPTrustedShellPreludeDomains ||
    (typeof require === "function"
      ? require("./trusted-shell-prelude-domains.js")
      : null);
  let activeFrame = null;
  let activeDomains = Object.freeze(["shell"]);
  let nativeSessionToken = null;

  function isPlainObject(value) {
    return value !== null && typeof value === "object" && !Array.isArray(value);
  }

  function isBoundedEnvelope(value) {
    if (!isPlainObject(value)) {
      return false;
    }
    try {
      return JSON.stringify(value).length <= MAX_ENVELOPE_BYTES;
    } catch (_) {
      return false;
    }
  }

  function boundedJSON(value, maximumBytes) {
    if (!isPlainObject(value)) {
      return false;
    }
    try {
      return JSON.stringify(value).length <= maximumBytes;
    } catch (_) {
      return false;
    }
  }

  function exactFields(value, fields) {
    if (!isPlainObject(value)) {
      return false;
    }
    const expected = fields.slice().sort();
    const actual = Object.keys(value).sort();
    return actual.length === expected.length &&
      actual.every((field, index) => field === expected[index]);
  }

  function resourceProjectionError(code, message) {
    const error = new Error(message);
    error.resourceCode = code;
    return error;
  }

  function decodedBase64Length(value) {
    if (typeof value !== "string" ||
        value.length % 4 !== 0 ||
        !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value)) {
      throw resourceProjectionError(
        "decode-failed",
        "resource bytes were not standard padded base64"
      );
    }
    const padding = value.endsWith("==") ? 2 : value.endsWith("=") ? 1 : 0;
    return (value.length / 4) * 3 - padding;
  }

  function decodeResourceBlob(encoded, mime, maximumBlobBytes) {
    if (typeof mime !== "string" ||
        mime.length === 0 ||
        mime.length > MAX_RESOURCE_MIME_BYTES ||
        /[\u0000-\u001f\u007f]/.test(mime)) {
      throw resourceProjectionError(
        "decode-failed",
        "resource MIME projection is invalid"
      );
    }
    const decodedLength = decodedBase64Length(encoded);
    if (decodedLength > maximumBlobBytes) {
      throw resourceProjectionError(
        "too-large",
        "resource Blob exceeds the trusted shell byte limit"
      );
    }
    if (typeof global.atob !== "function" ||
        typeof global.Blob !== "function" ||
        typeof global.Uint8Array !== "function") {
      throw resourceProjectionError(
        "decode-failed",
        "trusted resource byte conversion is unavailable"
      );
    }
    let binary;
    try {
      binary = global.atob(encoded);
    } catch (_) {
      throw resourceProjectionError(
        "decode-failed",
        "resource bytes could not be decoded"
      );
    }
    if (binary.length !== decodedLength) {
      throw resourceProjectionError(
        "decode-failed",
        "resource byte length did not match its base64 transport"
      );
    }
    const bytes = new global.Uint8Array(decodedLength);
    for (let index = 0; index < decodedLength; index += 1) {
      bytes[index] = binary.charCodeAt(index);
    }
    return new global.Blob([bytes], { type: mime });
  }

  function resourceErrorType(resultType) {
    switch (resultType) {
      case "resource.info.result":
        return "resource.info.error";
      case "resource.bytes.result":
        return "resource.bytes.error";
      case "resource.bytesMany.result":
        return "resource.bytesMany.error";
      default:
        return null;
    }
  }

  function projectedResourceError(resultType, id, error) {
    const type = resourceErrorType(resultType);
    if (type === null ||
        typeof id !== "string" ||
        id.length === 0 ||
        id.length > 1024) {
      return null;
    }
    return {
      type,
      id,
      error: typeof error.resourceCode === "string"
        ? error.resourceCode
        : "decode-failed",
      message: typeof error.message === "string"
        ? error.message
        : "resource projection failed"
    };
  }

  function projectResourceTerminal(
    envelope,
    maximumBlobBytes = MAX_RESOURCE_BLOB_BYTES
  ) {
    if (!isPlainObject(envelope) ||
        typeof envelope.type !== "string" ||
        typeof envelope.id !== "string" ||
        envelope.id.length === 0 ||
        envelope.id.length > 1024) {
      return null;
    }
    if (!boundedJSON(envelope, MAX_RESOURCE_TRANSPORT_BYTES)) {
      return projectedResourceError(
        envelope.type,
        envelope.id,
        resourceProjectionError(
          "too-large",
          "resource terminal exceeds the trusted shell transport limit"
        )
      );
    }
    if (envelope.type === "resource.info.result") {
      return boundedJSON(envelope, MAX_ENVELOPE_BYTES) &&
        exactFields(envelope, ["type", "id", "info"])
        ? envelope
        : projectedResourceError(
          envelope.type,
          envelope.id,
          resourceProjectionError(
            "decode-failed",
            "resource info terminal is malformed"
          )
        );
    }
    if (envelope.type === "resource.bytes.result") {
      try {
        if (!exactFields(envelope, ["type", "id", "blob", "mime"])) {
          throw resourceProjectionError(
            "decode-failed",
            "resource bytes terminal is malformed"
          );
        }
        return {
          type: envelope.type,
          id: envelope.id,
          blob: decodeResourceBlob(
            envelope.blob,
            envelope.mime,
            maximumBlobBytes
          ),
          mime: envelope.mime
        };
      } catch (error) {
        return projectedResourceError(envelope.type, envelope.id, error);
      }
    }
    if (envelope.type === "resource.bytesMany.result") {
      try {
        if (!exactFields(envelope, ["type", "id", "items"]) ||
            !Array.isArray(envelope.items) ||
            envelope.items.length > MAX_RESOURCE_ITEMS) {
          throw resourceProjectionError(
            "too-large",
            "resource bulk terminal is malformed or exceeds its item limit"
          );
        }
        let deliveredBytes = 0;
        const items = envelope.items.map((item) => {
          if (!isPlainObject(item) ||
              typeof item.url !== "string" ||
              item.url.length === 0 ||
              item.url.length > MAX_RESOURCE_URL_BYTES) {
            throw resourceProjectionError(
              "decode-failed",
              "resource bulk item is malformed"
            );
          }
          if (item.ok === true) {
            if (!exactFields(item, ["url", "ok", "blob", "mime"])) {
              throw resourceProjectionError(
                "decode-failed",
                "successful resource bulk item is malformed"
              );
            }
            const blob = decodeResourceBlob(
              item.blob,
              item.mime,
              maximumBlobBytes - deliveredBytes
            );
            deliveredBytes += blob.size;
            return {
              url: item.url,
              ok: true,
              blob,
              mime: item.mime
            };
          }
          if (item.ok === false &&
              exactFields(item, ["url", "ok", "error", "message"]) &&
              typeof item.error === "string" &&
              item.error.length > 0 &&
              item.error.length <= 128 &&
              typeof item.message === "string" &&
              item.message.length > 0 &&
              item.message.length <= 16 * 1024) {
            return {
              url: item.url,
              ok: false,
              error: item.error,
              message: item.message
            };
          }
          throw resourceProjectionError(
            "decode-failed",
            "failed resource bulk item is malformed"
          );
        });
        return {
          type: envelope.type,
          id: envelope.id,
          items
        };
      } catch (error) {
        return projectedResourceError(envelope.type, envelope.id, error);
      }
    }
    if (envelope.type === "resource.info.error" ||
        envelope.type === "resource.bytes.error" ||
        envelope.type === "resource.bytesMany.error") {
      return exactFields(envelope, ["type", "id", "error", "message"]) &&
        typeof envelope.error === "string" &&
        typeof envelope.message === "string" &&
        boundedJSON(envelope, MAX_ENVELOPE_BYTES)
        ? envelope
        : null;
    }
    return null;
  }

  function projectNativeEnvelope(
    envelope,
    resourceEnabled,
    maximumBlobBytes = MAX_RESOURCE_BLOB_BYTES
  ) {
    if (!isPlainObject(envelope) || typeof envelope.type !== "string") {
      return null;
    }
    if (envelope.type.indexOf("resource.") === 0) {
      return resourceEnabled
        ? projectResourceTerminal(envelope, maximumBlobBytes)
        : null;
    }
    return isBoundedEnvelope(envelope) ? envelope : null;
  }

  function mappedEnvelope(event, frame) {
    if (!frame || event.source !== frame.contentWindow) {
      return null;
    }
    if (!isBoundedEnvelope(event.data)) {
      return null;
    }
    return event.data;
  }

  function forwardToNative(envelope) {
    const payload = JSON.stringify({
      session: nativeSessionToken,
      envelope: envelope
    });
    const root = document.documentElement;
    root.setAttribute("data-nmp-native-envelope", payload);
    document.dispatchEvent(new Event(bridgeEventName));
    root.removeAttribute("data-nmp-native-envelope");
  }

  function scriptJSON(value) {
    return JSON.stringify(value)
      .replace(/</g, "\\u003c")
      .replace(/\u2028/g, "\\u2028")
      .replace(/\u2029/g, "\\u2029");
  }

  function manifestConfigSchema(parsed) {
    const element = parsed.querySelector('meta[name="napplet-config-schema"]');
    if (!element) {
      return null;
    }
    const raw = element.getAttribute("content");
    if (!raw || raw.length > MAX_CONFIG_SCHEMA_BYTES) {
      return null;
    }
    try {
      const schema = JSON.parse(raw);
      return isPlainObject(schema) ? schema : null;
    } catch (_) {
      return null;
    }
  }

  function compatibilityPreludeSource(domains, initialConfigSchema) {
    const requested = domains === undefined ? ["shell"] : domains;
    if (!Array.isArray(requested) ||
        requested.some((domain) =>
          domain !== "shell" &&
          domain !== "storage" &&
          domain !== "identity" &&
          domain !== "inc" &&
          domain !== "theme" &&
          domain !== "config" &&
          domain !== "resource" &&
          domain !== "link" &&
          domain !== "outbox" &&
          domain !== "relay" &&
          domain !== "intent"
        )) {
      throw new Error("The trusted shell cannot project every negotiated domain");
    }
    const projectedDomains = Array.from(new Set(["shell"].concat(requested))).sort();
    const configSchema = isPlainObject(initialConfigSchema)
      ? initialConfigSchema
      : null;
    return `(function () {
  "use strict";
  var MAX_PENDING_REQUESTS = 128;
  var MAX_EVENT_HANDLERS = 128;
  var MAX_CHANNELS = 32;
  var MAX_NOSTR_SUBSCRIPTIONS = 32;
  var MAX_OUTBOX_REQUEST_TIMEOUT_MILLIS = 11000;
  var MAX_RESOURCE_OBJECT_URLS = 128;
  var MAX_RESOURCE_INFO_SCHEMES = 16;
  var MAX_RESOURCE_INFO_LIMIT = 50 * 1024 * 1024;
  var MAX_RESOURCE_ITEMS = 100;
  var MAX_RESOURCE_URL_BYTES = 16 * 1024;
  var MAX_RESOURCE_MIME_BYTES = 256;
  var MAX_LINK_URL_BYTES = 8 * 1024;
  var MAX_LINK_LABEL_BYTES = 4 * 1024;
  var MAX_CONSOLE_MESSAGE_CHARS = 4000;
  var MAX_CONSOLE_ENTRIES = 500;
  var consoleEntriesForwarded = 0;
  // WebKit's \`Error.prototype.stack\` is only the frame list: unlike V8 it does
  // not begin with "Name: message". Why that matters: tests/trusted-shell-console.test.js.
  function describeThrowable(value) {
    if (typeof value === "string") return value;
    if (!(value instanceof Error)) {
      try { return JSON.stringify(value); } catch (_) { return String(value); }
    }
    var headline = (value.name || "Error") + (value.message ? ": " + value.message : "");
    var stack = value.stack;
    if (!stack) return headline;
    return stack.indexOf(headline) === 0 ? stack : headline + "\\n" + stack;
  }
  function forwardConsoleEntry(level, parts) {
    if (consoleEntriesForwarded >= MAX_CONSOLE_ENTRIES) {
      return;
    }
    consoleEntriesForwarded += 1;
    var text;
    try {
      text = Array.prototype.map.call(parts, function (value) { return describeThrowable(value); }).join(" ");
    } catch (_) {
      text = "<unprintable console arguments>";
    }
    if (text.length > MAX_CONSOLE_MESSAGE_CHARS) {
      text = text.slice(0, MAX_CONSOLE_MESSAGE_CHARS) + "…";
    }
    try {
      parent.postMessage({ type: "debug.console", level: level, message: text }, "*");
    } catch (_) {
      // Best effort only -- a console mirror must never itself throw.
    }
  }
  ["log", "info", "warn", "error", "debug"].forEach(function (level) {
    var original = console[level];
    console[level] = function () {
      forwardConsoleEntry(level, arguments);
      if (typeof original === "function") {
        original.apply(console, arguments);
      }
    };
  });
  addEventListener("error", function (event) {
    var detail = "Uncaught " + (event && event.message ? event.message : "error");
    if (event && event.filename) {
      detail += " (" + event.filename + ":" + event.lineno + ":" +
        (event.colno === undefined ? "?" : event.colno) + ")";
    }
    if (event && event.error && event.error.stack) {
      detail += "\\n" + event.error.stack;
    }
    forwardConsoleEntry("error", [detail]);
  });
  addEventListener("unhandledrejection", function (event) {
    forwardConsoleEntry("error", ["Unhandled rejection: " + describeThrowable(event && event.reason)]);
  });
  var projectedDomains = Object.freeze(${JSON.stringify(projectedDomains)});
  var nextRequest = 1;
  var pending = new Map();
  var environment = null;
  var disposed = false;
  var readyHandlers = new Set();
  var identityChangedHandlers = new Set();
  var themeChangedHandlers = new Set();
  var intentChangedHandlers = new Set();
  var configSubscribers = new Set();
  var configSchemaErrorHandlers = new Set();
  var configLastValues = null;
  var configCurrentSchema = ${scriptJSON(configSchema)};
  var topicStates = new Map();
  var channelStates = new Map();
  var outboxSubscriptions = new Map();
  var relaySubscriptions = new Map();
  var openingChannels = 0;
  var resourceObjectUrls = new Set();
  var resolveReady;
  var readyPromise = new Promise(function (resolve) {
    resolveReady = resolve;
  });
  function isObject(value) {
    return value !== null && typeof value === "object" && !Array.isArray(value);
  }
  function utf8ByteLengthAtMost(value, maximum) {
    var bytes = 0;
    for (var index = 0; index < value.length; index += 1) {
      var code = value.charCodeAt(index);
      if (code < 0x80) {
        bytes += 1;
      } else if (code < 0x800) {
        bytes += 2;
      } else if (code >= 0xd800 &&
                 code <= 0xdbff &&
                 index + 1 < value.length &&
                 value.charCodeAt(index + 1) >= 0xdc00 &&
                 value.charCodeAt(index + 1) <= 0xdfff) {
        bytes += 4;
        index += 1;
      } else {
        bytes += 3;
      }
      if (bytes > maximum) return false;
    }
    return true;
  }
  function handlerCount() {
    var count = readyHandlers.size +
      identityChangedHandlers.size +
      themeChangedHandlers.size +
      intentChangedHandlers.size +
      configSubscribers.size +
      configSchemaErrorHandlers.size;
    topicStates.forEach(function (state) {
      count += state.handlers.size;
    });
    channelStates.forEach(function (state) {
      count += state.handlers.size;
    });
    outboxSubscriptions.forEach(function (state) {
      count += state.event.size + state.closed.size;
    });
    relaySubscriptions.forEach(function (state) {
      count += (typeof state.onEvent === "function" ? 1 : 0) +
        (typeof state.onEose === "function" ? 1 : 0);
    });
    return count;
  }
  function requireHandlerCapacity() {
    if (handlerCount() >= MAX_EVENT_HANDLERS) {
      throw new RangeError("Napplet event handler capacity is full");
    }
  }
  function nextCorrelationId() {
    for (var attempt = 0; attempt < MAX_PENDING_REQUESTS; attempt += 1) {
      var id = "request-" + nextRequest;
      nextRequest = nextRequest >= Number.MAX_SAFE_INTEGER ? 1 : nextRequest + 1;
      if (!pending.has(id)) return id;
    }
    throw new Error("Napplet request correlation space is exhausted");
  }
  function request(
    type,
    fields,
    project,
    expectedType,
    acceptErrorEnvelope,
    expectedErrorType,
    cancelType,
    timeoutMillis
  ) {
    if (disposed) {
      return Promise.reject(new Error("Napplet session is closed"));
    }
    if (pending.size >= MAX_PENDING_REQUESTS) {
      return Promise.reject(new Error("Napplet request capacity is full"));
    }
    var id;
    try {
      id = nextCorrelationId();
    } catch (error) {
      return Promise.reject(error);
    }
    var envelope = { type: type, id: id };
    if (isObject(fields)) {
      Object.keys(fields).forEach(function (key) {
        if (key !== "type" && key !== "id") envelope[key] = fields[key];
      });
    }
    return new Promise(function (resolve, reject) {
      var operation = {
        resolve: resolve,
        reject: reject,
        resultType: expectedType || type + ".result",
        errorType: expectedErrorType || null,
        cancelType: cancelType || null,
        acceptErrorEnvelope: acceptErrorEnvelope === true,
        project: typeof project === "function" ? project : function (message) {
          return Object.prototype.hasOwnProperty.call(message, "result")
            ? message.result
            : message;
        },
        timer: null
      };
      pending.set(id, operation);
      if (typeof timeoutMillis === "number" &&
          Number.isFinite(timeoutMillis) &&
          timeoutMillis > 0) {
        operation.timer = setTimeout(function () {
          if (!pending.delete(id)) return;
          cancelPendingOperation(id, operation);
          reject(new Error(type + " timed out"));
        }, timeoutMillis);
      }
      try {
        parent.postMessage(envelope, "*");
      } catch (error) {
        pending.delete(id);
        if (operation.timer !== null) clearTimeout(operation.timer);
        reject(error);
      }
    });
  }
  function fireAndForget(type, fields) {
    if (disposed) return;
    var envelope = { type: type };
    if (isObject(fields)) {
      Object.keys(fields).forEach(function (key) {
        if (key !== "type" && key !== "id") envelope[key] = fields[key];
      });
    }
    parent.postMessage(envelope, "*");
  }
  function sendCorrelated(type, fields) {
    if (disposed) throw new Error("Napplet session is closed");
    var id = nextCorrelationId();
    var envelope = { type: type, id: id };
    if (isObject(fields)) {
      Object.keys(fields).forEach(function (key) {
        if (key !== "type" && key !== "id") envelope[key] = fields[key];
      });
    }
    parent.postMessage(envelope, "*");
    return id;
  }
  function cancelPendingOperation(id, operation) {
    if (!operation || typeof operation.cancelType !== "string") return;
    try {
      parent.postMessage({ type: operation.cancelType, id: id }, "*");
    } catch (_) {}
  }
  function closeHandle(close) {
    return Object.freeze({ close: close });
  }
  function normalizeEnvironment(message) {
    if (!isObject(message.capabilities) || !Array.isArray(message.services)) return null;
    var domains = Array.isArray(message.capabilities.domains)
      ? message.capabilities.domains
      : [];
    if (!domains.every(function (domain) { return typeof domain === "string"; }) ||
        !message.services.every(function (service) { return typeof service === "string"; })) {
      return null;
    }
    return Object.freeze({
      capabilities: Object.freeze({
        domains: Object.freeze(Array.from(new Set(domains)).sort())
      }),
      services: Object.freeze(message.services.slice())
    });
  }
  function acceptEnvironment(message) {
    if (environment !== null) return;
    var accepted = normalizeEnvironment(message);
    if (accepted === null) return;
    if (accepted.capabilities.domains.length !== projectedDomains.length ||
        accepted.capabilities.domains.some(function (domain, index) {
          return domain !== projectedDomains[index];
        })) {
      return;
    }
    environment = accepted;
    resolveReady(environment);
    readyHandlers.forEach(function (handler) {
      queueMicrotask(function () { handler(environment); });
    });
    readyHandlers.clear();
  }
  function errorMessage(error) {
    return typeof error === "string"
      ? error
      : isObject(error) && typeof error.message === "string"
        ? error.message
        : "Runtime request failed";
  }
  function terminalErrorMessage(message) {
    var code = typeof message.error === "string"
      ? message.error
      : "request-failed";
    return typeof message.message === "string"
      ? code + ": " + message.message
      : errorMessage(message.error);
  }
  function dispatchIdentityChanged(message) {
    if (typeof message.pubkey !== "string") return;
    Array.from(identityChangedHandlers).forEach(function (handler) {
      handler(message.pubkey);
    });
  }
  function dispatchThemeChanged(message) {
    if (!isObject(message.theme)) return;
    Array.from(themeChangedHandlers).forEach(function (handler) {
      handler(message.theme);
    });
  }
  function dispatchIntentChanged(message) {
    if (!isObject(message.availability)) return;
    Array.from(intentChangedHandlers).forEach(function (handler) {
      try {
        handler(message.availability);
      } catch (_) {}
    });
  }
  function dispatchConfigValues(message) {
    if (!isObject(message.values)) return;
    configLastValues = message.values;
    if (typeof message.id === "string") {
      settlePending(message);
      return;
    }
    Array.from(configSubscribers).forEach(function (handler) {
      try {
        handler(message.values);
      } catch (_) {}
    });
  }
  function dispatchConfigSchemaError(message) {
    if (typeof message.code !== "string" || typeof message.error !== "string") return;
    var error = Object.freeze({ code: message.code, error: message.error });
    Array.from(configSchemaErrorHandlers).forEach(function (handler) {
      try {
        handler(error);
      } catch (_) {}
    });
  }
  function settlePending(message) {
    var operation = pending.get(message.id);
    if (!operation ||
        (message.type !== operation.resultType &&
         message.type !== operation.errorType)) return false;
    pending.delete(message.id);
    if (operation.timer !== null) clearTimeout(operation.timer);
    if (message.type === operation.errorType ||
        (message.error && !operation.acceptErrorEnvelope)) {
      operation.reject(new Error(terminalErrorMessage(message)));
    } else {
      try {
        operation.resolve(operation.project(message));
      } catch (error) {
        cancelPendingOperation(message.id, operation);
        operation.reject(error);
      }
    }
    return true;
  }
  function payloadContent(payload) {
    if (typeof payload === "string") return payload;
    try {
      return JSON.stringify(payload === undefined ? {} : payload);
    } catch (_) {
      return "{}";
    }
  }
  function dispatchTopicEvent(message) {
    if (typeof message.topic !== "string" || typeof message.sender !== "string") return;
    var state = topicStates.get(message.topic);
    if (!state) return;
    var payload = Object.prototype.hasOwnProperty.call(message, "payload")
      ? message.payload
      : {};
    var event = Object.freeze({
      id: "",
      pubkey: message.sender,
      created_at: 0,
      kind: 0,
      tags: Object.freeze([Object.freeze(["t", message.topic])]),
      content: payloadContent(payload),
      sig: ""
    });
    Array.from(state.handlers).forEach(function (handler) {
      handler(payload, event);
    });
  }
  function dispatchChannelEvent(message) {
    if (typeof message.channelId !== "string" ||
        typeof message.sender !== "string") return;
    var state = channelStates.get(message.channelId);
    if (!state || state.closed) return;
    var event = Object.freeze({
      channelId: message.channelId,
      sender: message.sender,
      payload: Object.prototype.hasOwnProperty.call(message, "payload")
        ? message.payload
        : undefined
    });
    Array.from(state.handlers).forEach(function (handler) {
      handler(event);
    });
  }
  function dispatchChannelClosed(message) {
    if (typeof message.channelId !== "string") return;
    var state = channelStates.get(message.channelId);
    if (!state) return;
    state.closed = true;
    state.handlers.clear();
    channelStates.delete(message.channelId);
  }
  function dispatchOutboxEvent(message) {
    if (typeof message.subId !== "string" || !isObject(message.result)) return;
    var state = outboxSubscriptions.get(message.subId);
    if (!state) return;
    Array.from(state.event).forEach(function (handler) {
      try {
        handler(message.result);
      } catch (_) {}
    });
  }
  function dispatchOutboxClosed(message) {
    if (typeof message.subId !== "string") return;
    var state = outboxSubscriptions.get(message.subId);
    if (!state) return;
    outboxSubscriptions.delete(message.subId);
    state.active = false;
    Array.from(state.closed).forEach(function (handler) {
      try {
        handler(typeof message.reason === "string" ? message.reason : undefined);
      } catch (_) {}
    });
    state.event.clear();
    state.closed.clear();
  }
  function dispatchRelayEvent(message) {
    if (typeof message.subId !== "string" || !isObject(message.result)) return;
    var state = relaySubscriptions.get(message.subId);
    if (!state || typeof state.onEvent !== "function") return;
    try {
      state.onEvent(message.result);
    } catch (_) {}
  }
  function dispatchRelayEose(message) {
    if (typeof message.subId !== "string") return;
    var state = relaySubscriptions.get(message.subId);
    if (!state || state.eose || typeof state.onEose !== "function") return;
    state.eose = true;
    try {
      state.onEose();
    } catch (_) {}
  }
  function dispatchRelayClosed(message) {
    if (typeof message.subId !== "string") return;
    var state = relaySubscriptions.get(message.subId);
    if (!state) return;
    relaySubscriptions.delete(message.subId);
    state.active = false;
  }
  function dispose() {
    if (disposed) return;
    topicStates.forEach(function (_, topic) {
      try {
        fireAndForget("inc.unsubscribe", { topic: topic });
      } catch (_) {}
    });
    channelStates.forEach(function (_, channelId) {
      try {
        fireAndForget("inc.channel.close", { channelId: channelId });
      } catch (_) {}
    });
    outboxSubscriptions.forEach(function (_, subId) {
      try {
        sendCorrelated("outbox.close", { subId: subId });
      } catch (_) {}
    });
    relaySubscriptions.forEach(function (_, subId) {
      try {
        sendCorrelated("relay.close", { subId: subId });
      } catch (_) {}
    });
    if (configSubscribers.size > 0) {
      try {
        fireAndForget("config.unsubscribe");
      } catch (_) {}
    }
    disposed = true;
    topicStates.clear();
    channelStates.clear();
    outboxSubscriptions.clear();
    relaySubscriptions.clear();
    identityChangedHandlers.clear();
    themeChangedHandlers.clear();
    intentChangedHandlers.clear();
    configSubscribers.clear();
    configSchemaErrorHandlers.clear();
    configLastValues = null;
    configCurrentSchema = null;
    readyHandlers.clear();
    resourceObjectUrls.forEach(function (objectUrl) {
      try {
        URL.revokeObjectURL(objectUrl);
      } catch (_) {}
    });
    resourceObjectUrls.clear();
    pending.forEach(function (operation, id) {
      if (operation.timer !== null) clearTimeout(operation.timer);
      cancelPendingOperation(id, operation);
      operation.reject(new Error("Napplet session is closed"));
    });
    pending.clear();
  }
  addEventListener("message", function (event) {
    if (event.source !== parent || !event.data || typeof event.data !== "object") return;
    if (event.data.type === "shell.dispose") {
      dispose();
      return;
    }
    if (event.data.type === "shell.init") {
      acceptEnvironment(event.data);
      return;
    }
    if (event.data.type === "identity.changed") {
      dispatchIdentityChanged(event.data);
      return;
    }
    if (event.data.type === "theme.changed") {
      dispatchThemeChanged(event.data);
      return;
    }
    if (event.data.type === "config.values") {
      dispatchConfigValues(event.data);
      return;
    }
    if (event.data.type === "config.schemaError") {
      dispatchConfigSchemaError(event.data);
      return;
    }
    if (event.data.type === "inc.event") {
      dispatchTopicEvent(event.data);
      return;
    }
    if (event.data.type === "inc.channel.event") {
      dispatchChannelEvent(event.data);
      return;
    }
    if (event.data.type === "inc.channel.closed") {
      dispatchChannelClosed(event.data);
      return;
    }
    if (event.data.type === "outbox.event") {
      dispatchOutboxEvent(event.data);
      return;
    }
    if (event.data.type === "outbox.closed") {
      dispatchOutboxClosed(event.data);
      return;
    }
    if (event.data.type === "relay.event") {
      dispatchRelayEvent(event.data);
      return;
    }
    if (event.data.type === "relay.eose") {
      dispatchRelayEose(event.data);
      return;
    }
    if (event.data.type === "relay.closed") {
      dispatchRelayClosed(event.data);
      return;
    }
    if (event.data.type === "intent.changed") {
      dispatchIntentChanged(event.data);
      return;
    }
    settlePending(event.data);
  });
  addEventListener("pagehide", dispose);
  var shell = {};
  Object.defineProperties(shell, {
    supports: {
      enumerable: true,
      value: function (domain) {
        return typeof domain === "string" &&
          environment !== null &&
          environment.capabilities.domains.indexOf(domain) !== -1;
      }
    },
    services: {
      enumerable: true,
      get: function () {
        return environment === null ? Object.freeze([]) : environment.services;
      }
    },
    ready: {
      enumerable: true,
      value: function () { return readyPromise; }
    },
    onReady: {
      enumerable: true,
      value: function (handler) {
        if (typeof handler !== "function") throw new TypeError("onReady requires a function");
        requireHandlerCapacity();
        var active = true;
        if (environment === null) {
          readyHandlers.add(handler);
        } else {
          queueMicrotask(function () { if (active) handler(environment); });
        }
        return Object.freeze({
          unsubscribe: function () {
            if (!active) return;
            active = false;
            readyHandlers.delete(handler);
          }
        });
      }
    },
    ping: {
      enumerable: true,
      value: function (fields) { return request("shell.ping", fields); }
    }
  });
  var napplet = { shell: Object.freeze(shell) };
  if (projectedDomains.indexOf("storage") !== -1) {
    function storageGet(key, scope) {
      var fields = { key: key };
      if (scope === "instance") fields.scope = scope;
      return request("storage.get", fields).then(function (message) {
        return message.value;
      });
    }
    function storageSet(key, value, scope) {
      var fields = { key: key, value: value };
      if (scope === "instance") fields.scope = scope;
      return request("storage.set", fields).then(function () {});
    }
    function storageRemove(key, scope) {
      var fields = { key: key };
      if (scope === "instance") fields.scope = scope;
      return request("storage.remove", fields).then(function () {});
    }
    function storageKeys(scope) {
      var fields = {};
      if (scope === "instance") fields.scope = scope;
      return request("storage.keys", fields).then(function (message) {
        return message.keys;
      });
    }
    var instanceStorage = Object.freeze({
      getItem: function (key) { return storageGet(key, "instance"); },
      setItem: function (key, value) { return storageSet(key, value, "instance"); },
      removeItem: function (key) { return storageRemove(key, "instance"); },
      keys: function () { return storageKeys("instance"); }
    });
    napplet.storage = Object.freeze({
      getItem: function (key) { return storageGet(key); },
      setItem: function (key, value) { return storageSet(key, value); },
      removeItem: function (key) { return storageRemove(key); },
      keys: function () { return storageKeys(); },
      instance: instanceStorage
    });
  }
  if (projectedDomains.indexOf("identity") !== -1) {
    function identityRequest(action, fields, resultField) {
      return request("identity." + action, fields, function (message) {
        return message[resultField];
      });
    }
    napplet.identity = Object.freeze({
      getPublicKey: function () {
        return identityRequest("getPublicKey", null, "pubkey");
      },
      onChanged: function (handler) {
        if (typeof handler !== "function") {
          throw new TypeError("identity.onChanged requires a function");
        }
        requireHandlerCapacity();
        identityChangedHandlers.add(handler);
        var active = true;
        return closeHandle(function () {
          if (!active) return;
          active = false;
          identityChangedHandlers.delete(handler);
        });
      },
      getRelays: function () {
        return identityRequest("getRelays", null, "relays");
      },
      getProfile: function () {
        return identityRequest("getProfile", null, "profile");
      },
      getFollows: function () {
        return identityRequest("getFollows", null, "pubkeys");
      },
      getList: function (listType) {
        return identityRequest("getList", { listType: listType }, "entries");
      },
      getZaps: function () {
        return identityRequest("getZaps", null, "zaps");
      },
      getMutes: function () {
        return identityRequest("getMutes", null, "pubkeys");
      },
      getBlocked: function () {
        return identityRequest("getBlocked", null, "pubkeys");
      },
      getBadges: function () {
        return identityRequest("getBadges", null, "badges");
      }
    });
  }
  if (projectedDomains.indexOf("outbox") !== -1) {
    function outboxTimeout(options) {
      var requested = isObject(options) && typeof options.timeoutMs === "number"
        ? options.timeoutMs + 1000
        : MAX_OUTBOX_REQUEST_TIMEOUT_MILLIS;
      return Number.isFinite(requested) && requested > 0
        ? Math.min(requested, MAX_OUTBOX_REQUEST_TIMEOUT_MILLIS)
        : MAX_OUTBOX_REQUEST_TIMEOUT_MILLIS;
    }
    function outboxGetEvent(eventId, options) {
      var fields = { eventId: eventId };
      if (options !== undefined) fields.options = options;
      return request(
        "outbox.getEvent",
        fields,
        function (message) {
          var result = {};
          if (Object.prototype.hasOwnProperty.call(message, "result")) {
            result.result = message.result;
          }
          if (message.incomplete === true) result.incomplete = true;
          if (typeof message.reason === "string") result.reason = message.reason;
          if (typeof message.error === "string") result.error = message.error;
          return Object.freeze(result);
        },
        "outbox.getEvent.result",
        true,
        null,
        null,
        outboxTimeout(options)
      );
    }
    function outboxQuery(filters, options) {
      var fields = { filters: filters };
      if (options !== undefined) fields.options = options;
      return request(
        "outbox.query",
        fields,
        function (message) {
          var result = {
            events: Object.freeze(
              Array.isArray(message.events) ? message.events.slice() : []
            )
          };
          if (message.incomplete === true) result.incomplete = true;
          if (typeof message.reason === "string") result.reason = message.reason;
          if (typeof message.error === "string") result.error = message.error;
          return Object.freeze(result);
        },
        "outbox.query.result",
        true,
        null,
        null,
        outboxTimeout(options)
      );
    }
    function outboxSubscribe(filters, options) {
      if (outboxSubscriptions.size >= MAX_NOSTR_SUBSCRIPTIONS) {
        throw new RangeError("NAP-OUTBOX subscription capacity is full");
      }
      var subId = nextCorrelationId();
      var state = { event: new Set(), closed: new Set(), active: true };
      outboxSubscriptions.set(subId, state);
      var fields = { subId: subId, filters: filters };
      if (options !== undefined) fields.options = options;
      try {
        sendCorrelated("outbox.subscribe", fields);
      } catch (error) {
        outboxSubscriptions.delete(subId);
        throw error;
      }
      return Object.freeze({
        on: function (event, handler) {
          if ((event !== "event" && event !== "closed") ||
              typeof handler !== "function") {
            throw new TypeError(
              "outbox subscription on requires event or closed and a function"
            );
          }
          if (!state.active || outboxSubscriptions.get(subId) !== state) {
            throw new Error("NAP-OUTBOX subscription is closed");
          }
          requireHandlerCapacity();
          state[event].add(handler);
        },
        close: function () {
          if (!state.active) return;
          state.active = false;
          outboxSubscriptions.delete(subId);
          state.event.clear();
          state.closed.clear();
          sendCorrelated("outbox.close", { subId: subId });
        }
      });
    }
    function outboxPublish(event, options) {
      var fields = { event: event };
      if (options !== undefined) fields.options = options;
      return request(
        "outbox.publish",
        fields,
        function (message) {
          var result = { ok: message.ok === true };
          if (isObject(message.event)) result.event = message.event;
          if (typeof message.eventId === "string") result.eventId = message.eventId;
          if (isObject(message.relays)) result.relays = message.relays;
          if (typeof message.error === "string") result.error = message.error;
          return Object.freeze(result);
        },
        "outbox.publish.result",
        true,
        null,
        null,
        MAX_OUTBOX_REQUEST_TIMEOUT_MILLIS
      );
    }
    function outboxResolveRelays(target) {
      return request(
        "outbox.resolveRelays",
        { target: target },
        function (message) {
          if (!isObject(message.plan)) {
            throw new Error("outbox.resolveRelays.result missing plan");
          }
          return message.plan;
        },
        null,
        false,
        null,
        null,
        MAX_OUTBOX_REQUEST_TIMEOUT_MILLIS
      );
    }
    napplet.outbox = Object.freeze({
      getEvent: outboxGetEvent,
      query: outboxQuery,
      subscribe: outboxSubscribe,
      publish: outboxPublish,
      resolveRelays: outboxResolveRelays
    });
  }
  if (projectedDomains.indexOf("relay") !== -1) {
    function relaySubscribe(filters, onEvent, onEose, options) {
      if (typeof onEvent !== "function" || typeof onEose !== "function") {
        throw new TypeError("relay.subscribe requires event and EOSE functions");
      }
      if (relaySubscriptions.size >= MAX_NOSTR_SUBSCRIPTIONS) {
        throw new RangeError("NAP-RELAY subscription capacity is full");
      }
      requireHandlerCapacity();
      if (handlerCount() + 1 >= MAX_EVENT_HANDLERS) {
        throw new RangeError("Napplet event handler capacity is full");
      }
      var subId = nextCorrelationId();
      var state = {
        onEvent: onEvent,
        onEose: onEose,
        eose: false,
        active: true
      };
      relaySubscriptions.set(subId, state);
      var fields = {
        subId: subId,
        filters: Array.isArray(filters) ? filters : [filters]
      };
      if (isObject(options) && typeof options.relay === "string") {
        fields.relay = options.relay;
      }
      try {
        sendCorrelated("relay.subscribe", fields);
      } catch (error) {
        relaySubscriptions.delete(subId);
        throw error;
      }
      return closeHandle(function () {
        if (!state.active) return;
        state.active = false;
        relaySubscriptions.delete(subId);
        sendCorrelated("relay.close", { subId: subId });
      });
    }
    function requireRelayEvent(message, operation) {
      if (!isObject(message.event)) {
        throw new Error(operation + ".result missing event");
      }
      return message.event;
    }
    napplet.relay = Object.freeze({
      subscribe: relaySubscribe,
      publish: function (event) {
        return request(
          "relay.publish",
          { event: event },
          function (message) {
            return requireRelayEvent(message, "relay.publish");
          }
        );
      },
      publishEncrypted: function (event, recipient, encryption) {
        var fields = { event: event, recipient: recipient };
        if (encryption !== undefined) fields.encryption = encryption;
        return request(
          "relay.publishEncrypted",
          fields,
          function (message) {
            return requireRelayEvent(message, "relay.publishEncrypted");
          }
        );
      },
      query: function (filters) {
        return request(
          "relay.query",
          { filters: Array.isArray(filters) ? filters : [filters] },
          function (message) {
            var result = Array.isArray(message.events)
              ? message.events.slice()
              : [];
            if (message.incomplete === true) result.incomplete = true;
            if (typeof message.reason === "string") result.reason = message.reason;
            if (typeof message.error === "string") result.error = message.error;
            return Object.freeze(result);
          }
        );
      }
    });
  }
${preludeDomainsSource.DOMAIN_CLIENT_SOURCE}
  if (projectedDomains.indexOf("config") !== -1) {
    var config = {
      registerSchema: function (schema, version) {
        var fields = { schema: schema };
        if (version !== undefined) fields.version = version;
        return request(
          "config.registerSchema",
          fields,
          function (message) {
            if (message.ok !== true) {
              throw new Error(
                (typeof message.code === "string" ? message.code : "invalid-schema") +
                ": " +
                (typeof message.error === "string" ? message.error : "schema rejected")
              );
            }
            configCurrentSchema = schema;
          },
          "config.registerSchema.result",
          true
        );
      },
      get: function () {
        return request(
          "config.get",
          null,
          function (message) { return message.values; },
          "config.values"
        );
      },
      subscribe: function (handler) {
        if (typeof handler !== "function") {
          throw new TypeError("config.subscribe requires a function");
        }
        requireHandlerCapacity();
        var first = configSubscribers.size === 0;
        configSubscribers.add(handler);
        if (first) {
          fireAndForget("config.subscribe");
        } else if (configLastValues !== null) {
          var snapshot = configLastValues;
          queueMicrotask(function () {
            if (!disposed && configSubscribers.has(handler)) {
              try {
                handler(snapshot);
              } catch (_) {}
            }
          });
        }
        var active = true;
        return closeHandle(function () {
          if (!active) return;
          active = false;
          configSubscribers.delete(handler);
          if (configSubscribers.size === 0) {
            fireAndForget("config.unsubscribe");
          }
        });
      },
      openSettings: function (options) {
        var fields = {};
        if (options !== null &&
            options !== undefined &&
            options.section !== undefined) {
          fields.section = options.section;
        }
        fireAndForget("config.openSettings", fields);
      },
      onSchemaError: function (handler) {
        if (typeof handler !== "function") {
          throw new TypeError("config.onSchemaError requires a function");
        }
        requireHandlerCapacity();
        configSchemaErrorHandlers.add(handler);
        var active = true;
        return function () {
          if (!active) return;
          active = false;
          configSchemaErrorHandlers.delete(handler);
        };
      }
    };
    Object.defineProperty(config, "schema", {
      configurable: false,
      enumerable: true,
      get: function () { return configCurrentSchema; }
    });
    napplet.config = Object.freeze(config);
    if (configCurrentSchema !== null) {
      // The manifest-declared schema was extracted from this artifact's own
      // <meta name="napplet-config-schema"> before untrusted code ran. Native
      // must have it registered before the napplet's own bootstrap calls
      // config.subscribe()/config.get(), so the shell submits it here rather
      // than waiting for (and trusting) the napplet to register its own copy.
      // This has to wait for the NAP-SHELL handshake (readyPromise) first --
      // native refuses any request from a mapped session before that
      // handshake completes, and this runs before the napplet's own code
      // gets a chance to run at all.
      readyPromise.then(function () {
        return config.registerSchema(configCurrentSchema);
      }).catch(function (error) {
        forwardConsoleEntry("error", [
          "napplet.config: automatic schema registration failed: " +
            (error && error.message ? error.message : String(error))
        ]);
      });
    }
  }
  // Replaceable on purpose: authority is the envelope and the native grant,
  // not this object, which a napplet could bypass by posting envelopes
  // itself. Napplets bundling the published SDK assign their own client here
  // at load, where a locked property throws in strict mode and kills them.
  Object.defineProperty(window, "napplet",
    { configurable: true, enumerable: true, writable: true, value: Object.freeze(napplet) });
  parent.postMessage({ type: "shell.ready" }, "*");
})();`;
  }

  function sandboxPolicyContent() {
    if (!policySource) {
      throw new Error("The trusted shell policy is unavailable");
    }
    return policySource.innerPolicyContent();
  }

  function isVerifiedArtifactBaseURL(value) {
    return typeof value === "string" &&
      /^nmp-artifact:\/\/[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\/$/.test(value);
  }

  function materialize(artifactHTML, artifactBaseURL, domains) {
    if (typeof global.DOMParser !== "function") {
      throw new Error("The trusted shell requires an HTML parser");
    }
    if (!isVerifiedArtifactBaseURL(artifactBaseURL)) {
      throw new Error("The verified artifact base URL is invalid");
    }

    // Parsing into an inert document is security-critical. String/regex
    // rewriting cannot model the HTML parser's error recovery, and can place
    // the bootstrap after an executable node in malformed-but-valid input
    // such as `<script>…</script><head>`.
    const parser = new global.DOMParser();
    const parsed = parser.parseFromString(artifactHTML, "text/html");
    const head = parsed.head;
    if (!head) {
      throw new Error("The artifact did not produce an HTML head");
    }

    const policy = parsed.createElement("meta");
    policy.setAttribute("http-equiv", "Content-Security-Policy");
    policy.setAttribute("content", sandboxPolicyContent());

    const base = parsed.createElement("base");
    base.setAttribute("href", artifactBaseURL);

    const prelude = parsed.createElement("script");
    prelude.textContent = compatibilityPreludeSource(
      domains,
      manifestConfigSchema(parsed)
    );

    // The enforced policy is the first child and the compatibility bootstrap
    // is the second. DOMParser is inert, so no authored executable node can
    // run before these nodes are serialized into the sandboxed srcdoc.
    head.prepend(prelude);
    head.prepend(base);
    head.prepend(policy);

    return "<!doctype html>\n" + parsed.documentElement.outerHTML;
  }

  global.__nmpTrustedShellMount = function mount(configuration) {
    if (!isPlainObject(configuration) ||
        typeof configuration.session !== "string" ||
        typeof configuration.artifactHTML !== "string" ||
        !isVerifiedArtifactBaseURL(configuration.artifactBaseURL)) {
      return false;
    }
    nativeSessionToken = configuration.session;
    const frame = document.createElement("iframe");
    frame.id = "napplet-frame";
    frame.setAttribute("sandbox", "allow-scripts");
    frame.setAttribute("referrerpolicy", "no-referrer");
    frame.setAttribute("aria-label", configuration.title || "Napplet");
    frame.srcdoc = materialize(
      configuration.artifactHTML,
      configuration.artifactBaseURL,
      configuration.domains
    );
    const surface = document.getElementById("surface");
    surface.replaceChildren(frame);
    activeFrame = frame;
    activeDomains = Object.freeze(
      Array.from(new Set(["shell"].concat(configuration.domains || []))).sort()
    );
    return true;
  };

  global.__nmpTrustedShellReceive = function receive(envelope) {
    if (!activeFrame) {
      return false;
    }
    const projected = projectNativeEnvelope(
      envelope,
      activeDomains.indexOf("resource") !== -1
    );
    if (projected === null) {
      return false;
    }
    activeFrame.contentWindow.postMessage(projected, "*");
    return true;
  };

  if (typeof global.addEventListener === "function") {
    global.addEventListener("message", function receiveNappletMessage(event) {
      const envelope = mappedEnvelope(event, activeFrame);
      if (envelope !== null) {
        forwardToNative(envelope);
      }
    });
  }

  if (typeof module !== "undefined" && module.exports) {
    module.exports = {
      MAX_ENVELOPE_BYTES,
      MAX_RESOURCE_TRANSPORT_BYTES,
      MAX_RESOURCE_BLOB_BYTES,
      isBoundedEnvelope,
      mappedEnvelope,
      projectResourceTerminal,
      projectNativeEnvelope,
      materialize,
      sandboxPolicyContent,
      isVerifiedArtifactBaseURL,
      compatibilityPreludeSource
    };
  }
})(typeof window === "undefined" ? globalThis : window);
