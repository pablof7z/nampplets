import Foundation
import NMPNativeRuntimeApple

/// Real Workbench adapter over the profile's single pushed installed-library
/// observation.
///
/// The adapter retains one complete bounded replacement. Cross-thread native
/// delivery enters a one-slot coalescing mailbox, and subscriber fanout is
/// finite. Commands are forwarded exactly and never mutate Swift state
/// optimistically.
@MainActor
public final class RuntimeWorkbenchLibraryManager:
    WorkbenchLibraryManaging
{
    private struct Subscriber {
        let receive: @MainActor (WorkbenchLibraryUpdate) -> Void
    }

    private static let maximumSubscribers = 16

    private let native: any RuntimeWorkbenchNativeLibraryService
    private let mailbox: RuntimeWorkbenchLibraryMailbox
    private var nativeObservation:
        (any RuntimeWorkbenchNativeLibraryObservation)?
    private var observationFailureReason: String?
    private var current: WorkbenchLibrarySnapshot
    private var subscribers: [UUID: Subscriber] = [:]

    public private(set) var latestAdmissionRefusal:
        RuntimeWorkbenchLibraryAdmissionRefusal?

    public convenience init(profile: WorkbenchRuntimeProfile) {
        self.init(native: ProfileNativeLibraryService(profile: profile))
    }

    init(native: any RuntimeWorkbenchNativeLibraryService) {
        self.native = native
        current = Self.project(native.projection())
        let mailbox = RuntimeWorkbenchLibraryMailbox()
        self.mailbox = mailbox
        nativeObservation = nil
        observationFailureReason = nil
        mailbox.bind { [weak self] update in
            self?.receive(update)
        }
        do {
            nativeObservation = try native.observe { [mailbox] update in
                mailbox.offer(update)
            }
        } catch {
            let reason =
                "Installed-library observation was refused: "
                + Self.displaySafeReason(
                    error.localizedDescription,
                    fallback: "The native observer was unavailable."
                )
            observationFailureReason = reason
            current = Self.unavailableSnapshot(
                revision: current.revision,
                reason: reason
            )
        }
    }

    public func subscribe(
        receive: @escaping @MainActor (WorkbenchLibraryUpdate) -> Void
    ) -> any WorkbenchLibrarySubscription {
        guard subscribers.count < Self.maximumSubscribers else {
            latestAdmissionRefusal = .subscriberCapacity(
                maximum: Self.maximumSubscribers
            )
            receive(.authoritative(current))
            return RuntimeWorkbenchLibrarySubscription(cancellation: {})
        }

        let identifier = UUID()
        subscribers[identifier] = Subscriber(receive: receive)
        receive(.authoritative(current))
        return RuntimeWorkbenchLibrarySubscription { [weak self] in
            self?.subscribers.removeValue(forKey: identifier)
        }
    }

    public func refresh() -> WorkbenchLibrarySnapshot {
        let refreshed: WorkbenchLibrarySnapshot
        if let observationFailureReason {
            refreshed = Self.unavailableSnapshot(
                revision: native.projection().revision,
                reason: observationFailureReason
            )
        } else {
            refreshed = Self.project(native.projection())
        }
        if refreshed.revision > current.revision {
            current = refreshed
        }
        return current
    }

    public func setFilter(_ query: String) {
        native.setFilter(query)
    }

    public func suspend(sessionID: UInt64) {
        native.suspend(sessionID: sessionID)
    }

    public func resume(sessionID: UInt64) {
        native.resume(sessionID: sessionID)
    }

    public func assign(
        _ exactBuild: WorkbenchLibraryExactBuild,
        toWorkspaceID workspaceID: String
    ) {
        native.assign(
            Self.nativeExactBuild(exactBuild),
            toWorkspaceID: workspaceID
        )
    }

    public func clearAssignment(
        _ exactBuild: WorkbenchLibraryExactBuild,
        fromWorkspaceID workspaceID: String
    ) {
        native.clearAssignment(
            Self.nativeExactBuild(exactBuild),
            fromWorkspaceID: workspaceID
        )
    }

    public func uninstall(_ exactBuild: WorkbenchLibraryExactBuild) {
        native.uninstall(Self.nativeExactBuild(exactBuild))
    }

    private func receive(_ update: NativeRuntimeLibraryUpdate) {
        switch update {
        case .authoritative(let projection):
            let projected = Self.project(projection)
            guard projected.revision > current.revision else {
                return
            }
            observationFailureReason = nil
            current = projected
            for subscriber in subscribers.values {
                subscriber.receive(.authoritative(current))
            }

        case .next(
            let projection,
            let predecessorRevision,
            _,
            let lostBeforeBatch
        ):
            let projected = Self.project(projection)
            // The frame layer re-delivers at the same revision when the cursor
            // was stale, precisely so a loss is not swallowed. Returning early
            // here on that replacement discarded the only notice of it, so the
            // freshness guard now only governs whether the snapshot advances.
            guard projected.revision > current.revision
                || lostBeforeBatch > 0
            else {
                return
            }
            observationFailureReason = nil
            if projected.revision > current.revision {
                current = projected
            }
            for subscriber in subscribers.values {
                subscriber.receive(
                    .next(
                        current,
                        predecessorRevision: predecessorRevision,
                        lostBeforeBatch: lostBeforeBatch
                    )
                )
            }
        }
    }

    deinit {
        nativeObservation?.cancel()
        mailbox.close()
    }
}
