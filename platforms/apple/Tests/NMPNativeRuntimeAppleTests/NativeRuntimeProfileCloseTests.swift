import Foundation
import NMPNativeRuntime
@testable import NMPNativeRuntimeApple
import XCTest

/// Teardown must finish. `close()` held `accountLock` -- a plain, non-recursive
/// `NSLock` -- across `observation.stop()`, every session's `profileDidClose()`
/// and four executor closes, each of which can block until an in-flight runtime
/// callback returns. A callback reaching any account method takes that same
/// lock, and neither side proceeds.
///
/// Coverage limit, stated because it matters: none of these tests reproduce
/// that deadlock. It needs an application callback to be in flight on another
/// thread at the moment `close()` runs, and nothing in the teardown path
/// invokes an application callback synchronously -- `NativeIncActionRouter`
/// nils its handler before cancelling pending work, so it cannot be driven
/// deterministically from a test. What is covered is the property a deadlock
/// would violate first: teardown completes, promptly, and more than once.
final class NativeRuntimeProfileCloseTests: XCTestCase {
    private func temporaryRoot() -> URL {
        FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "runtime-profile-close-\(UUID().uuidString)",
                isDirectory: true
            )
    }

    private func openProfile() throws -> (NativeRuntimeProfile, URL) {
        let root = temporaryRoot()
        let profile = try NativeRuntimeProfile.open(
            configuration: NativeRuntimeProfileConfiguration(storageRoot: root)
        )
        return (profile, root)
    }

    /// The bound is generous on purpose: this asserts "returns at all", not a
    /// performance target. A wedged teardown blocks forever, so any finite
    /// limit separates the two.
    func testCloseReturnsRatherThanBlockingForever() throws {
        let (profile, root) = try openProfile()
        defer { try? FileManager.default.removeItem(at: root) }

        let finished = expectation(description: "close returned")
        DispatchQueue.global().async {
            profile.close()
            finished.fulfill()
        }

        wait(for: [finished], timeout: 20)
    }

    /// `isClosed` is set under `lock` before any teardown call runs, so a
    /// second close returns at the guard. Worth pinning: narrowing the
    /// `accountLock` hold must not make close re-enter its own teardown.
    func testCloseIsIdempotent() throws {
        let (profile, root) = try openProfile()
        defer { try? FileManager.default.removeItem(at: root) }

        profile.close()

        let finished = expectation(description: "second close returned")
        DispatchQueue.global().async {
            profile.close()
            profile.close()
            finished.fulfill()
        }

        wait(for: [finished], timeout: 20)
    }

    /// Concurrent closes contend on `accountLock` around the controller. One
    /// wins the guard and the rest return; none may be left holding it.
    func testConcurrentClosesAllReturn() throws {
        let (profile, root) = try openProfile()
        defer { try? FileManager.default.removeItem(at: root) }

        let finished = expectation(description: "every close returned")
        finished.expectedFulfillmentCount = 8
        for _ in 0 ..< 8 {
            DispatchQueue.global().async {
                profile.close()
                finished.fulfill()
            }
        }

        wait(for: [finished], timeout: 20)
    }

    /// An account call after close must return rather than block. It is the
    /// cheapest observable proof that `close()` did not leave `accountLock`
    /// held, which is what the old shape risked on the deadlocking path.
    func testAnAccountCallAfterCloseStillReturns() throws {
        let (profile, root) = try openProfile()
        defer { try? FileManager.default.removeItem(at: root) }

        profile.close()

        let finished = expectation(description: "account call returned")
        DispatchQueue.global().async {
            _ = profile.accountSnapshot()
            finished.fulfill()
        }

        wait(for: [finished], timeout: 20)
    }
}
