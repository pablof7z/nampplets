import NMPNativeRuntime
import XCTest

private enum SnapshotProjectionTestError: Error {
    case refused
    case unknown
}

extension GeneratedBindingTests {
    func requireSnapshot(
        _ projection: RuntimeSnapshotProjection
    ) throws -> RuntimeSnapshot {
        switch projection {
        case let .snapshot(snapshot):
            snapshot
        case let .refused(_, _, refusal):
            XCTFail(
                "runtime snapshot refused: \(refusal.code): \(refusal.detail)"
            )
            throw SnapshotProjectionTestError.refused
        @unknown default:
            XCTFail("runtime returned an unknown snapshot projection")
            throw SnapshotProjectionTestError.unknown
        }
    }
}
