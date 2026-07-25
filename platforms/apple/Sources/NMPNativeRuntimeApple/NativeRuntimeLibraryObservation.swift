import Foundation
import NMPNativeRuntime

// MARK: - Installed-library observer fanout types

/// Pushed replacement semantics for the later profile-owned observer fanout.
public enum NativeRuntimeLibraryUpdate:
    Equatable,
    Sendable
{
    case authoritative(NativeRuntimeLibraryProjection)
    case next(
        NativeRuntimeLibraryProjection,
        predecessorRevision: UInt64,
        eventCursorWasStale: Bool,
    )
}

public enum NativeRuntimeLibraryObservationError:
    Error,
    LocalizedError,
    Equatable,
    Sendable
{
    case profileClosed
    case observerCapacity(maximum: Int)

    public var errorDescription: String? {
        switch self {
        case .profileClosed:
            "The native runtime profile is closed."
        case let .observerCapacity(maximum):
            "The native library observer limit of \(maximum) was reached."
        }
    }
}

/// Idempotent cancellation handle for one future profile-owned observer.
public final class NativeRuntimeLibraryObservation:
    @unchecked Sendable
{
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
