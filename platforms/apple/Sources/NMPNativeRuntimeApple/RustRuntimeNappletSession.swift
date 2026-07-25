import Foundation
import NMPNativeRuntime

// MARK: - Borrowed exact-build napplet session

protocol TrustedNappletRuntimeSession: VerifiedArtifactByteReader {
    var sessionID: UInt64 { get }

    func setResponseSink(_ sink: (@Sendable (Data) -> Void)?)
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
    private var responseSink: (@Sendable (Data) -> Void)?
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
        responseSink = sink
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
        let sink = responseSink
        let stopped = isStopped
        lock.unlock()
        guard !stopped, let sink else { return }

        for event in frame.events
        where (event.kind == "envelope-handled"
            || event.kind == "provider-push")
            && event.sessionId == sessionID {
            guard let response = event.responseJson,
                  let bytes = response.data(using: .utf8)
            else {
                continue
            }
            sink(bytes)
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
