import Foundation
import SwiftUI
import WebKit

public struct TrustedNappletView {
    private let artifact: NappletArtifact
    private let onActivity: @MainActor @Sendable (TrustedNappletActivity) -> Void

    public init(
        artifact: NappletArtifact,
        onActivity: @escaping @MainActor @Sendable (TrustedNappletActivity) -> Void = { _ in }
    ) {
        self.artifact = artifact
        self.onActivity = onActivity
    }

    @MainActor
    public func makeCoordinator() -> Coordinator {
        Coordinator(artifact: artifact, onActivity: onActivity)
    }

    @MainActor
    private struct SandboxFrameUnavailable: Error {}

    public final class Coordinator: NSObject, WKNavigationDelegate, WKScriptMessageHandler {
        private static let bridgeName = "runtimeBridge"
        private static let bridgeWorld = WKContentWorld.world(name: "io.f7z.nmp.native-runtime.bridge")
        private static let maxEnvelopeBytes = 64 * 1024

        private let artifact: NappletArtifact
        private let sessionID: String
        private let responseSinkOwner = UUID()
        private let artifactSchemeHandler: VerifiedArtifactSchemeHandler
        private let onActivity: @MainActor @Sendable (TrustedNappletActivity) -> Void
        private var trustedShellURL: URL?
        private var navigationGeneration: UInt64 = 0
        private var activeTrustedGeneration: UInt64?
        private var stopped = false
        private weak var currentWebView: WKWebView?
        private var sandboxFrameInfo: WKFrameInfo?

        init(
            artifact: NappletArtifact,
            onActivity: @escaping @MainActor @Sendable (TrustedNappletActivity) -> Void
        ) {
            self.artifact = artifact
            self.onActivity = onActivity
            let sessionID = UUID().uuidString.lowercased()
            self.sessionID = sessionID
            self.artifactSchemeHandler = VerifiedArtifactSchemeHandler(
                sessionID: sessionID,
                reader: artifact.reader
            )
            super.init()
            artifact.runtimeSession?.setResponseSink(
                owner: responseSinkOwner
            ) { [weak self] bytes in
                Task { @MainActor [weak self] in
                    self?.deliverRuntimeResponse(bytes)
                }
            }
            artifact.runtimeSession?.setDiagnosticSink { [weak self] level, message in
                Task { @MainActor [weak self] in
                    guard let self, !self.stopped else { return }
                    self.onActivity(.consoleEntry(level: level, message: message))
                }
            }
        }

        func makeWebView() -> WKWebView {
            let contentController = WKUserContentController()
            contentController.add(
                self,
                contentWorld: Self.bridgeWorld,
                name: Self.bridgeName
            )
            contentController.addUserScript(Self.bridgeRelayScript)

            let configuration = WKWebViewConfiguration()
            configuration.userContentController = contentController
            configuration.websiteDataStore = .nonPersistent()
            configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
            configuration.defaultWebpagePreferences.allowsContentJavaScript = true
            configuration.mediaTypesRequiringUserActionForPlayback = .all
            configuration.setURLSchemeHandler(
                artifactSchemeHandler,
                forURLScheme: VerifiedArtifactSchemeHandler.scheme
            )

            let webView = WKWebView(frame: .zero, configuration: configuration)
            #if DEBUG
            webView.isInspectable = true
            #endif
            currentWebView = webView
            webView.navigationDelegate = self
            webView.underPageBackgroundColor = .clear
            onActivity(.loading)

            guard let shellURL = TrustedShellResources.shellURL else {
                onActivity(.refused(reason: "Trusted shell resource is unavailable"))
                return webView
            }
            trustedShellURL = shellURL.resolvingSymlinksInPath().standardizedFileURL
            webView.loadFileURL(
                shellURL,
                allowingReadAccessTo: shellURL.deletingLastPathComponent()
            )
            return webView
        }

        func stop(_ webView: WKWebView) {
            guard !stopped else { return }
            stopped = true
            activeTrustedGeneration = nil
            currentWebView = nil
            artifact.runtimeSession?.clearResponseSink(
                owner: responseSinkOwner
            )
            artifact.runtimeSession?.setDiagnosticSink(nil)
            artifactSchemeHandler.teardown()
            webView.stopLoading()
            webView.navigationDelegate = nil
            webView.configuration.userContentController.removeScriptMessageHandler(
                forName: Self.bridgeName,
                contentWorld: Self.bridgeWorld
            )
            webView.configuration.userContentController.removeAllUserScripts()
        }

        /// Diagnostic-only: evaluates script inside the sandboxed napplet
        /// iframe rather than the trusted outer document. Production code
        /// never needs this -- native/sandbox communication is entirely
        /// message-based -- but tooling that inspects a napplet's own
        /// rendered DOM (e.g. an inspector or an integration test driving a
        /// real click) has no other way to reach it, since the sandbox's
        /// opaque origin blocks ordinary `iframe.contentDocument` access.
        public func evaluateJavaScriptInSandbox(_ script: String) async throws -> Any? {
            guard let webView = currentWebView, let frame = sandboxFrameInfo else {
                throw SandboxFrameUnavailable()
            }
            return try await webView.evaluateJavaScript(script, in: frame, contentWorld: .page)
        }

        public func userContentController(
            _ userContentController: WKUserContentController,
            didReceive message: WKScriptMessage
        ) {
            guard !stopped else { return }
            guard activeTrustedGeneration == navigationGeneration,
                  message.frameInfo.isMainFrame,
                  isTrustedShellURL(message.frameInfo.request.url),
                  isTrustedShellURL(message.webView?.url)
            else {
                onActivity(.refused(reason: "Bridge message did not originate in the trusted main frame"))
                return
            }
            guard let raw = message.body as? String,
                  raw.utf8.count <= Self.maxEnvelopeBytes,
                  let data = raw.data(using: .utf8),
                  let bridgeMessage = try? JSONSerialization.jsonObject(with: data)
                    as? [String: Any],
                  let envelope = bridgeMessage["envelope"] as? [String: Any],
                  let messageType = envelope["type"] as? String,
                  !messageType.isEmpty,
                  let encodedEnvelope = try? JSONSerialization.data(
                    withJSONObject: envelope
                  ),
                  encodedEnvelope.count <= Self.maxEnvelopeBytes
            else {
                onActivity(.refused(reason: "Malformed or oversized bridge envelope"))
                return
            }

            // The caller-supplied session is deliberately ignored. Session
            // authority comes from the sealed Rust session attached to this
            // exact mapped frame.
            route(
                messageType: messageType,
                encodedEnvelope: encodedEnvelope
            )
        }

        public func webView(
            _ webView: WKWebView,
            didStartProvisionalNavigation navigation: WKNavigation!
        ) {
            guard !stopped else { return }
            navigationGeneration &+= 1
            activeTrustedGeneration = nil
        }

        public func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            guard !stopped else { return }
            guard isTrustedShellURL(webView.url) else {
                onActivity(.refused(reason: "A non-shell navigation cannot activate the bridge"))
                return
            }
            let generation = navigationGeneration
            activeTrustedGeneration = generation
            let artifactHTML: String
            do {
                guard let index = try artifact.reader.readSealed(logicalPath: "/index.html"),
                      index.logicalPath == "/index.html",
                      index.bytes.count <= VerifiedArtifactSchemeLimits.production.maximumFileBytes,
                      let decoded = String(data: index.bytes, encoding: .utf8)
                else {
                    throw VerifiedArtifactReaderError.unavailable
                }
                artifactHTML = decoded
            } catch {
                activeTrustedGeneration = nil
                onActivity(.refused(reason: "Verified artifact index is unavailable"))
                return
            }
            let configuration: [String: Any] = [
                "session": sessionID,
                "artifactHTML": artifactHTML,
                "artifactBaseURL": artifactSchemeHandler.baseURL.absoluteString,
                "title": artifact.title,
                "domains": artifact.negotiatedDomains
            ]
            Task { @MainActor [weak self, weak webView] in
                guard let self else { return }
                guard let webView else { return }
                do {
                    _ = try await webView.callAsyncJavaScript(
                        "return window.__nmpTrustedShellMount(configuration)",
                        arguments: ["configuration": configuration],
                        in: nil,
                        contentWorld: .page
                    )
                    guard !self.stopped,
                          self.activeTrustedGeneration == generation,
                          self.isTrustedShellURL(webView.url)
                    else {
                        return
                    }
                    self.onActivity(.mounted)
                } catch {
                    if !self.stopped {
                        self.onActivity(.refused(reason: Self.mountFailureReason(error)))
                    }
                }
            }
        }

        private static let maxMountFailureDetailBytes = 256

        /// `callAsyncJavaScript` surfaces a thrown mount-time exception (for
        /// example the trusted shell rejecting a negotiated domain it cannot
        /// project) only through this error. The detail is untrusted —
        /// `configuration["artifactHTML"]` is attacker-controlled and parsed
        /// during mount — so it is bounded and stripped of control
        /// characters before it reaches a user-facing activity string.
        private static func mountFailureReason(_ error: Error) -> String {
            let fallback = "The trusted shell refused the artifact"
            let nsError = error as NSError
            // `callAsyncJavaScript` reports a thrown mount-time exception as a
            // bare WKError whose `localizedDescription` is the unhelpful
            // generic "A JavaScript exception occurred" -- the actual message
            // (and, often, source location) lives in `userInfo` instead.
            let detail =
                (nsError.userInfo["WKJavaScriptExceptionMessage"] as? String)
                ?? nsError.localizedDescription
            let sanitized = String(
                detail.unicodeScalars.filter { scalar in
                    !CharacterSet.newlines.contains(scalar)
                        && !CharacterSet.controlCharacters.contains(scalar)
                }
            )
            guard !sanitized.isEmpty else {
                return fallback
            }
            let bounded = sanitized.utf8.count > maxMountFailureDetailBytes
                ? String(decoding: Array(sanitized.utf8.prefix(maxMountFailureDetailBytes)), as: UTF8.self)
                : sanitized
            return "\(fallback): \(bounded)"
        }

        public func webView(
            _ webView: WKWebView,
            decidePolicyFor navigationAction: WKNavigationAction,
            decisionHandler: @escaping @MainActor (WKNavigationActionPolicy) -> Void
        ) {
            guard let url = navigationAction.request.url else {
                decisionHandler(.cancel)
                return
            }

            let isMainFrame = navigationAction.targetFrame?.isMainFrame == true
            if isMainFrame, isTrustedShellURL(url) {
                decisionHandler(.allow)
                return
            }

            if url.scheme == "about", !isMainFrame {
                sandboxFrameInfo = navigationAction.targetFrame
                decisionHandler(.allow)
                return
            }

            if isMainFrame {
                onActivity(.refused(reason: "Trusted shell navigation was denied"))
            }
            decisionHandler(.cancel)
        }

        public func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
            guard !stopped else { return }
            activeTrustedGeneration = nil
            artifact.runtimeSession?.crash(
                reason: "The mapped WebKit content process terminated"
            )
            onActivity(.crashed)
        }

        private func isTrustedShellURL(_ candidate: URL?) -> Bool {
            guard let trustedShellURL, let candidate, candidate.isFileURL else {
                return false
            }
            return candidate.resolvingSymlinksInPath().standardizedFileURL == trustedShellURL
        }

        /// Every bridge message goes to the runtime, unconditionally.
        ///
        /// Whether one is part of the NAP envelope protocol is a
        /// protocol-membership judgement, and Rust owns those: it reserves the
        /// `debug.*` domain, classifies the envelope itself, bounds it, and
        /// keeps it away from providers. Native asserting that here — on a
        /// `type` the sandboxed iframe supplies — would let any content that
        /// reaches the bridge choose what the runtime observes, simply by
        /// naming itself well.
        ///
        /// The Inspector's console is fed from the runtime's own classified
        /// diagnostic, delivered back through `setDiagnosticSink`, so nothing
        /// here reads a napplet-authored payload to decide what to draw.
        private func route(
            messageType: String,
            encodedEnvelope: Data
        ) {
            onActivity(.request(type: messageType))
            artifact.runtimeSession?.mappedEnvelope(encodedEnvelope)
        }

        private func deliverRuntimeResponse(_ bytes: Data) {
            guard !stopped,
                  activeTrustedGeneration == navigationGeneration,
                  let webView = currentWebView,
                  isTrustedShellURL(webView.url),
                  bytes.count <= Self.maxEnvelopeBytes,
                  let envelope = try? JSONSerialization.jsonObject(with: bytes),
                  envelope is [String: Any]
            else {
                return
            }
            Task { @MainActor [weak self, weak webView] in
                guard let self,
                      !self.stopped,
                      self.activeTrustedGeneration == self.navigationGeneration,
                      let webView,
                      self.isTrustedShellURL(webView.url)
                else {
                    return
                }
                _ = try? await webView.callAsyncJavaScript(
                    "return window.__nmpTrustedShellReceive(envelope)",
                    arguments: ["envelope": envelope],
                    in: nil,
                    contentWorld: .page
                )
            }
        }

        private static let bridgeRelayScript = WKUserScript(
            source: """
            document.addEventListener("nmp-native-envelope", function () {
              const root = document.documentElement;
              const raw = root.getAttribute("data-nmp-native-envelope");
              if (raw !== null) {
                window.webkit.messageHandlers.runtimeBridge.postMessage(raw);
              }
            });
            """,
            injectionTime: .atDocumentStart,
            forMainFrameOnly: true,
            in: bridgeWorld
        )
    }
}

#if os(macOS)
extension TrustedNappletView: NSViewRepresentable {
    public func makeNSView(context: Context) -> WKWebView {
        context.coordinator.makeWebView()
    }

    public func updateNSView(_ webView: WKWebView, context: Context) {}

    public static func dismantleNSView(_ webView: WKWebView, coordinator: Coordinator) {
        coordinator.stop(webView)
    }
}
#elseif os(iOS)
extension TrustedNappletView: UIViewRepresentable {
    public func makeUIView(context: Context) -> WKWebView {
        context.coordinator.makeWebView()
    }

    public func updateUIView(_ webView: WKWebView, context: Context) {}

    public static func dismantleUIView(_ webView: WKWebView, coordinator: Coordinator) {
        coordinator.stop(webView)
    }
}
#endif

enum TrustedShellResources {
    static var shellURL: URL? {
        Bundle.module.url(
            forResource: "trusted-shell",
            withExtension: "html",
            subdirectory: "TrustedShell"
        )
    }

    static var fixtureURL: URL? {
        Bundle.module.url(
            forResource: "minimal-conformant-napplet",
            withExtension: "html",
            subdirectory: "TrustedShell/fixtures"
        )
    }

    static func externalFixtureURL(_ logicalPath: String) -> URL? {
        guard logicalPath.first == "/" else { return nil }
        let relative = String(logicalPath.dropFirst())
        let path = (relative as NSString).deletingPathExtension
        let pathExtension = (relative as NSString).pathExtension
        return Bundle.module.url(
            forResource: (path as NSString).lastPathComponent,
            withExtension: pathExtension.isEmpty ? nil : pathExtension,
            subdirectory: "TrustedShell/fixtures/external-assets/"
                + (path as NSString).deletingLastPathComponent
        )
    }
}
