import Foundation
import NMPNativeRuntime

// MARK: - Relay diagnostics observation types

public enum NativeRuntimeRelayDiagnosticsObservationError:
    Error,
    LocalizedError,
    Equatable
{
    case profileClosed
    case refused(code: String, detail: String)

    public var errorDescription: String? {
        switch self {
        case .profileClosed:
            "The native runtime profile is closed."
        case let .refused(code, detail):
            "The runtime refused relay diagnostics (\(code)): \(detail)"
        }
    }
}

/// Idempotent cancellation for one relay diagnostics observer.
///
/// Unlike the activity and catalog fanouts, this handle owns real Rust-side
/// work: the NMP diagnostics observation is withdrawn once the last handle is
/// cancelled or released.
public final class NativeRuntimeRelayDiagnosticsObservation: @unchecked Sendable {
    private let lock = NSLock()
    private var cancellation: (@Sendable () -> Void)?

    init(cancellation: @escaping @Sendable () -> Void) {
        self.cancellation = cancellation
    }

    public func cancel() {
        lock.lock()
        let cancellation = cancellation
        self.cancellation = nil
        lock.unlock()
        cancellation?()
    }

    deinit {
        cancel()
    }
}

final class NativeRuntimeRelayDiagnosticsForwarder:
    RuntimeRelayDiagnosticsObserver,
    @unchecked Sendable
{
    private let receive:
        @Sendable (NativeRuntimeRelayDiagnosticsSnapshot) -> Void

    init(
        receive: @escaping @Sendable (NativeRuntimeRelayDiagnosticsSnapshot)
            -> Void
    ) {
        self.receive = receive
    }

    func update(snapshot: RuntimeRelayDiagnosticsSnapshot) {
        receive(snapshot)
    }
}
