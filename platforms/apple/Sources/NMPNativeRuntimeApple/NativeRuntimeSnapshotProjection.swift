import Foundation
import NMPNativeRuntime

/// Exhaustive, fail-closed resolver for the generated snapshot sum type.
enum NativeRuntimeSnapshotProjection {
    case snapshot(RuntimeSnapshot)
    case refused(
        revision: UInt64,
        profileClosed: Bool,
        refusal: RuntimeRefusal
    )

    init(
        _ projection: RuntimeSnapshotProjection,
        unknownRevision: UInt64,
        unknownProfileClosed: Bool
    ) {
        switch projection {
        case let .snapshot(snapshot):
            self = .snapshot(snapshot)
        case let .refused(revision, closed, refusal):
            self = .refused(
                revision: revision,
                profileClosed: closed,
                refusal: refusal
            )
        @unknown default:
            self = .refused(
                revision: unknownRevision,
                profileClosed: unknownProfileClosed,
                refusal: RuntimeRefusal(
                    code: "unsupported-snapshot-projection",
                    detail: "The generated runtime returned an unknown snapshot projection",
                    occurredAtMillis: 0
                )
            )
        }
    }

    var revision: UInt64 {
        switch self {
        case let .snapshot(snapshot):
            snapshot.revision
        case let .refused(revision, _, _):
            revision
        }
    }

    var profileClosed: Bool {
        switch self {
        case let .snapshot(snapshot):
            snapshot.closed
        case let .refused(_, profileClosed, _):
            profileClosed
        }
    }
}

public enum NativeRuntimeSnapshotProjectionError:
    Error,
    LocalizedError,
    Equatable,
    Sendable
{
    case refused(RuntimeRefusal)

    public var errorDescription: String? {
        switch self {
        case let .refused(refusal):
            "Runtime snapshot projection was refused (\(refusal.code)): \(refusal.detail)"
        }
    }
}

extension NativeRuntimeProfile {
    static func initialSnapshot(
        from projection: RuntimeSnapshotProjection
    ) throws -> RuntimeSnapshot {
        let resolved = NativeRuntimeSnapshotProjection(
            projection,
            unknownRevision: 0,
            unknownProfileClosed: false
        )
        switch resolved {
        case let .snapshot(snapshot):
            return snapshot
        case let .refused(_, _, refusal):
            throw RuntimeNappletOpenError.snapshotRefused(
                code: refusal.code,
                detail: refusal.detail
            )
        }
    }

    func pullSnapshotProjection() -> NativeRuntimeSnapshotProjection {
        let generated = controller.snapshot()
        lock.lock()
        let fallback = lastAcceptedSnapshot
        let projection = NativeRuntimeSnapshotProjection(
            generated,
            unknownRevision: fallback.revision,
            unknownProfileClosed: fallback.closed
        )
        switch projection {
        case let .snapshot(snapshot):
            lastAcceptedSnapshot = snapshot
        case .refused:
            break
        }
        lock.unlock()
        return projection
    }

    func validatedSnapshot() throws -> RuntimeSnapshot {
        switch pullSnapshotProjection() {
        case let .snapshot(snapshot):
            snapshot
        case let .refused(_, _, refusal):
            throw NativeRuntimeSnapshotProjectionError.refused(refusal)
        }
    }
}
