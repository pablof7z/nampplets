import Foundation
import Network
import WebKit
import XCTest
@testable import NMPNativeRuntimeApple

@MainActor
final class TrustedNappletWebKitBoundaryTests: XCTestCase {
    func testPackagedFixtureRunsOnlyInsideMappedRevocableSrcdocBoundary()
        async throws
    {
        let fixtureURL = try XCTUnwrap(TrustedShellResources.fixtureURL)
        let fixtureBytes = try Data(contentsOf: fixtureURL)
        let artifact = try XCTUnwrap(NappletArtifact.bundledCompatibilityFixture())
        let sealed = try XCTUnwrap(
            try artifact.reader.readSealed(logicalPath: "/index.html")
        )
        XCTAssertEqual(sealed.logicalPath, "/index.html")
        XCTAssertEqual(sealed.bytes, fixtureBytes)
        XCTAssertFalse(
            String(decoding: sealed.bytes, as: UTF8.self)
                .contains("Content-Security-Policy"),
            "runtime policy must remain outside the sealed authored bytes"
        )

        let listener = try NWListener(using: .tcp, on: .any)
        let listenerReady = expectation(description: "network probe listening")
        let unexpectedTransport = expectation(
            description: "opaque authored frame reached ambient transport"
        )
        unexpectedTransport.isInverted = true
        let connections = BoundaryLockedCounter()
        listener.stateUpdateHandler = { state in
            if case .ready = state {
                listenerReady.fulfill()
            }
        }
        listener.newConnectionHandler = { connection in
            connections.increment()
            unexpectedTransport.fulfill()
            connection.cancel()
        }
        listener.start(
            queue: DispatchQueue(
                label: "io.f7z.nmp.native-runtime.webkit-boundary"
            )
        )
        defer { listener.cancel() }
        await fulfillment(of: [listenerReady], timeout: 5)
        let port = try XCTUnwrap(listener.port?.rawValue)

        let mounted = expectation(description: "trusted fixture mounted")
        let authoredRequest = expectation(
            description: "fixture request crossed its mapped source window"
        )
        let forgedSessionRequest = expectation(
            description: "mapped source ignored its caller session field"
        )
        let alienRequest = expectation(
            description: "sibling source impersonated the mapped frame"
        )
        alienRequest.isInverted = true
        let lateActivity = expectation(
            description: "stopped presentation emitted late activity"
        )
        lateActivity.isInverted = true
        var requestCount = 0
        var stopped = false
        let view = TrustedNappletView(artifact: artifact) { activity in
            if stopped {
                lateActivity.fulfill()
                return
            }
            if activity == .mounted {
                mounted.fulfill()
            }
            if activity == .request(type: "shell.ping") {
                requestCount += 1
                switch requestCount {
                case 1:
                    authoredRequest.fulfill()
                case 2:
                    forgedSessionRequest.fulfill()
                default:
                    alienRequest.fulfill()
                }
            }
        }
        let coordinator = view.makeCoordinator()
        let webView = coordinator.makeWebView()
        defer {
            if !stopped {
                coordinator.stop(webView)
            }
        }

        await fulfillment(of: [mounted, authoredRequest], timeout: 10)

        let materialized = try await webView.callAsyncJavaScript(
            """
            const frame = document.getElementById("napplet-frame");
            const parsed = new DOMParser().parseFromString(
              frame.getAttribute("srcdoc"),
              "text/html"
            );
            const head = parsed.head;
            const policy = head.children[0];
            const base = head.children[1];
            const prelude = head.children[2];
            const scripts = Array.from(parsed.querySelectorAll("script"));
            return {
              frameSourceIsOnlySrcdoc: frame.getAttribute("src") === null,
              sandbox: frame.getAttribute("sandbox"),
              referrerPolicy: frame.getAttribute("referrerpolicy"),
              policyIsFirst:
                policy.tagName === "META" &&
                policy.getAttribute("http-equiv") ===
                  "Content-Security-Policy",
              policy: policy.getAttribute("content"),
              baseIsSecond: base.tagName === "BASE",
              baseHref: base.getAttribute("href"),
              preludeIsThird:
                prelude.tagName === "SCRIPT" &&
                prelude.textContent.includes("MAX_PENDING_REQUESTS"),
              authoredScriptCount: scripts.length - 1,
              authoredScriptsFollowPrelude: scripts.slice(1).every(
                script => Boolean(
                  prelude.compareDocumentPosition(script) &
                    Node.DOCUMENT_POSITION_FOLLOWING
                )
              )
            };
            """,
            arguments: [:],
            in: nil,
            contentWorld: .page
        )
        let materializedState = try XCTUnwrap(materialized as? [String: Any])
        XCTAssertEqual(materializedState["frameSourceIsOnlySrcdoc"] as? Bool, true)
        XCTAssertEqual(materializedState["sandbox"] as? String, "allow-scripts")
        XCTAssertEqual(materializedState["referrerPolicy"] as? String, "no-referrer")
        XCTAssertEqual(materializedState["policyIsFirst"] as? Bool, true)
        XCTAssertTrue(
            try XCTUnwrap(materializedState["policy"] as? String)
                .contains("default-src 'none'")
        )
        XCTAssertTrue(
            try XCTUnwrap(materializedState["policy"] as? String)
                .contains("connect-src 'none'")
        )
        XCTAssertEqual(materializedState["baseIsSecond"] as? Bool, true)
        XCTAssertTrue(
            try XCTUnwrap(materializedState["baseHref"] as? String)
                .hasPrefix("nmp-artifact://")
        )
        XCTAssertEqual(materializedState["preludeIsThird"] as? Bool, true)
        XCTAssertEqual(materializedState["authoredScriptCount"] as? Int, 1)
        XCTAssertEqual(materializedState["authoredScriptsFollowPrelude"] as? Bool, true)

        let ambient = try await coordinator.evaluateJavaScriptInSandbox(
            """
            (() => {
              let hostDOMDenied = false;
              let localStorageDenied = false;
              let sessionStorageDenied = false;
              let cookieDenied = false;
              try { void parent.document.documentElement; }
              catch (_) { hostDOMDenied = true; }
              try { localStorage.setItem("forbidden", "1"); }
              catch (_) { localStorageDenied = true; }
              try { sessionStorage.setItem("forbidden", "1"); }
              catch (_) { sessionStorageDenied = true; }
              try {
                document.cookie = "forbidden=1";
                cookieDenied = document.cookie.indexOf("forbidden=1") < 0;
              } catch (_) { cookieDenied = true; }

              const http = "http://127.0.0.1:\(port)";
              const websocket = "ws://127.0.0.1:\(port)";
              fetch(http + "/fetch").catch(() => {});
              try {
                const socket = new WebSocket(websocket + "/socket");
                socket.onerror = () => socket.close();
              } catch (_) {}
              const image = new Image();
              image.src = http + "/image";
              const script = document.createElement("script");
              script.src = http + "/script";
              document.head.appendChild(script);
              try {
                const worker = new Worker(http + "/worker");
                worker.onerror = () => worker.terminate();
              } catch (_) {}

              let serviceWorkerRegistrationError = "";
              try {
                const registration =
                  navigator.serviceWorker.register("./service-worker.js");
                if (registration && typeof registration.catch === "function") {
                  registration.catch(() => {});
                }
              } catch (error) {
                serviceWorkerRegistrationError =
                  String(error && error.name ? error.name : error);
              }

              return {
                hostDOMDenied,
                localStorageDenied,
                sessionStorageDenied,
                cookieDenied,
                nativeBridgeAbsent: !(
                  window.webkit &&
                  window.webkit.messageHandlers &&
                  window.webkit.messageHandlers.runtimeBridge
                ),
                windowNostrAbsent: typeof window.nostr === "undefined",
                // WebKit always exposes `navigator.serviceWorker`, so probing
                // for the property proves nothing. What matters is that
                // registering one is refused: the frame is sandboxed without
                // `allow-same-origin`, so its origin is opaque and WebKit
                // rejects registration outright.
                serviceWorkerRegistrationDenied: serviceWorkerRegistrationError !== "",
                serviceWorkerRegistrationError
              };
            })()
            """
        )
        let ambientState = try XCTUnwrap(ambient as? [String: Any])
        for key in [
            "hostDOMDenied",
            "localStorageDenied",
            "sessionStorageDenied",
            "cookieDenied",
            "nativeBridgeAbsent",
            "windowNostrAbsent",
            "serviceWorkerRegistrationDenied",
        ] {
            XCTAssertEqual(ambientState[key] as? Bool, true, "\(key) failed")
        }
        // Pin the reason, not just the refusal: an opaque sandboxed origin is
        // why registration is impossible, so a different error here means the
        // boundary changed shape and must be re-examined.
        XCTAssertEqual(
            ambientState["serviceWorkerRegistrationError"] as? String,
            "SecurityError"
        )
        XCTAssertFalse(webView.configuration.websiteDataStore.isPersistent)
        await fulfillment(of: [unexpectedTransport], timeout: 0.75)
        XCTAssertEqual(connections.value, 0)

        _ = try await coordinator.evaluateJavaScriptInSandbox(
            """
            parent.postMessage({
              type: "shell.ping",
              requestId: "mapped-forged-session",
              session: "caller-controlled"
            }, "*");
            true
            """
        )
        await fulfillment(of: [forgedSessionRequest], timeout: 5)

        _ = try await webView.callAsyncJavaScript(
            """
            return await new Promise(resolve => {
              const sibling = document.createElement("iframe");
              sibling.setAttribute("sandbox", "allow-scripts");
              sibling.onload = () => resolve(true);
              sibling.srcdoc = `<script>
                parent.postMessage({
                  type: "shell.ping",
                  requestId: "alien-source",
                  session: "caller-controlled"
                }, "*");
              <\\/script>`;
              document.getElementById("surface").appendChild(sibling);
            });
            """,
            arguments: [:],
            in: nil,
            contentWorld: .page
        )
        await fulfillment(of: [alienRequest], timeout: 0.5)
        XCTAssertEqual(requestCount, 2)

        stopped = true
        coordinator.stop(webView)
        coordinator.stop(webView)
        do {
            _ = try await coordinator.evaluateJavaScriptInSandbox("true")
            XCTFail("stopped coordinator retained its mapped frame")
        } catch {
            // Expected: teardown clears the exact web view and frame mapping.
        }
        coordinator.webViewWebContentProcessDidTerminate(webView)
        coordinator.webView(webView, didFinish: nil)
        await fulfillment(of: [lateActivity], timeout: 0.5)
        XCTAssertEqual(requestCount, 2)
    }
}

private final class BoundaryLockedCounter: @unchecked Sendable {
    private let lock = NSLock()
    private var storage = 0

    var value: Int {
        lock.lock()
        defer { lock.unlock() }
        return storage
    }

    func increment() {
        lock.lock()
        storage += 1
        lock.unlock()
    }
}
