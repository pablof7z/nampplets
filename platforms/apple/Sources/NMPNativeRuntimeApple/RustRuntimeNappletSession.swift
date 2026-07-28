import Foundation
import NMPNativeRuntime

// MARK: - Borrowed exact-build napplet session

protocol TrustedNappletRuntimeSession: VerifiedArtifactByteReader {
    var sessionID: UInt64 { get }

    func setResponseSink(_ sink: (@Sendable (Data) -> Void)?)
    /// Receives the runtime's own classification of a napplet diagnostic.
    ///
    /// Native renders these; it does not decide which envelopes are
    /// diagnostics. That judgement is the runtime's, and arrives already
    /// levelled and bounded.
    func setDiagnosticSink(_ sink: (@Sendable (String, String) -> Void)?)
    func setResponseSink(
        owner: UUID,
        _ sink: @escaping @Sendable (Data) -> Void
    )
    func clearResponseSink(owner: UUID)
    func mappedEnvelope(_ bytes: Data)
    func stop()
    func crash(reason: String)
}

/// A sealed, exact-build session. The generated controller is the only owner
/// of identity, grants, lifecycle, provider routing, and artifact reads.
final class RustRuntimeNappletSession: TrustedNappletRuntimeSession, @unchecked Sendable {
    let sessionID: UInt64

    private weak var profile: NativeRuntimeProfile?
    private let maximumReadBytes: UInt64
    private let lock = NSLock()
    private var responseSink:
        (owner: UUID, receive: @Sendable (Data) -> Void)?
    private var diagnosticSink: (@Sendable (String, String) -> Void)?
    private var isStopped = false

    init(
        profile: NativeRuntimeProfile,
        sessionID: UInt64,
        maximumReadBytes: UInt64
    ) {
        self.profile = profile
        self.sessionID = sessionID
        self.maximumReadBytes = maximumReadBytes
    }

    func readSealed(logicalPath: String) throws -> SealedArtifactBytes? {
        guard let profile else { return nil }
        switch profile.readVerified(
            sessionID: sessionID,
            logicalPath: logicalPath,
            maximumBytes: maximumReadBytes
        ) {
        case let .bytes(bytes, _, sha256):
            return SealedArtifactBytes(
                logicalPath: logicalPath,
                sha256: sha256,
                bytes: bytes
            )
        case .refused:
            return nil
        }
    }

    func setResponseSink(_ sink: (@Sendable (Data) -> Void)?) {
        lock.lock()
        responseSink = sink.map { (UUID(), $0) }
        lock.unlock()
    }

    func setResponseSink(
        owner: UUID,
        _ sink: @escaping @Sendable (Data) -> Void
    ) {
        lock.lock()
        guard !isStopped else {
            lock.unlock()
            return
        }
        responseSink = (owner, sink)
        lock.unlock()
    }

    func clearResponseSink(owner: UUID) {
        lock.lock()
        if responseSink?.owner == owner {
            responseSink = nil
        }
        lock.unlock()
    }

    func setDiagnosticSink(_ sink: (@Sendable (String, String) -> Void)?) {
        lock.lock()
        diagnosticSink = sink
        lock.unlock()
    }

    func mappedEnvelope(_ bytes: Data) {
        lock.lock()
        let stopped = isStopped
        lock.unlock()
        guard !stopped else { return }
        profile?.mappedEnvelope(sessionID: sessionID, bytes: bytes)
    }

    func deliver(frame: RuntimeObservationFrame) {
        lock.lock()
        let sink = responseSink?.receive
        let diagnostics = diagnosticSink
        let stopped = isStopped
        lock.unlock()
        guard !stopped else { return }

        for event in frame.events where event.sessionId == sessionID {
            switch event.kind {
            case "envelope-handled", "provider-push":
                guard let sink,
                      let response = event.responseJson,
                      let bytes = response.data(using: .utf8)
                else {
                    continue
                }
                sink(bytes)
            case "napplet-diagnostic":
                // `detail` is the runtime's own level and `responseJson` its
                // bounded message. Neither is parsed out of a napplet-authored
                // envelope here.
                guard let diagnostics, let message = event.responseJson else {
                    continue
                }
                diagnostics(event.detail, message)
            default:
                continue
            }
        }
    }

    func stop() {
        lock.lock()
        guard !isStopped else {
            lock.unlock()
            return
        }
        isStopped = true
        responseSink = nil
        diagnosticSink = nil
        let profile = profile
        self.profile = nil
        lock.unlock()

        profile?.stopSession(sessionID)
    }

    func crash(reason: String) {
        lock.lock()
        let stopped = isStopped
        lock.unlock()
        guard !stopped else { return }
        profile?.crashSession(sessionID, reason: reason)
    }

    func profileDidClose() {
        lock.lock()
        isStopped = true
        responseSink = nil
        profile = nil
        lock.unlock()
    }

    deinit {
        stop()
    }
}
