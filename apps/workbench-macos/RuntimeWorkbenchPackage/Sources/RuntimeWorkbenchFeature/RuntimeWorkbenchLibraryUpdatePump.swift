import Foundation
import NMPNativeRuntimeApple

@MainActor
final class RuntimeWorkbenchLibrarySubscription:
    WorkbenchLibrarySubscription
{
    private var cancellation: (@MainActor @Sendable () -> Void)?

    init(cancellation: @escaping @MainActor @Sendable () -> Void) {
        self.cancellation = cancellation
    }

    func cancel() {
        let cancellation = cancellation
        self.cancellation = nil
        cancellation?()
    }

    deinit {
        let cancellation = cancellation
        DispatchQueue.main.async {
            MainActor.assumeIsolated {
                cancellation?()
            }
        }
    }
}

/// One-slot replacement mailbox. There is at most one scheduled main-queue
/// drain and at most one retained update; newer complete replacements coalesce
/// older pending replacements.
final class RuntimeWorkbenchLibraryMailbox: @unchecked Sendable {
    typealias Handler =
        @MainActor @Sendable (NativeRuntimeLibraryUpdate) -> Void

    private let lock = NSLock()
    private var handler: Handler?
    private var pending: NativeRuntimeLibraryUpdate?
    private var isScheduled = false
    private var isClosed = false

    @MainActor
    func bind(_ handler: @escaping Handler) {
        lock.lock()
        guard !isClosed else {
            lock.unlock()
            return
        }
        self.handler = handler
        let shouldSchedule = pending != nil && !isScheduled
        if shouldSchedule {
            isScheduled = true
        }
        lock.unlock()
        if shouldSchedule {
            scheduleDrain()
        }
    }

    func offer(_ update: NativeRuntimeLibraryUpdate) {
        lock.lock()
        guard !isClosed else {
            lock.unlock()
            return
        }
        pending = pending.map {
            Self.preferredPendingUpdate(current: $0, offered: update)
        } ?? update
        let shouldSchedule = handler != nil && !isScheduled
        if shouldSchedule {
            isScheduled = true
        }
        lock.unlock()
        if shouldSchedule {
            scheduleDrain()
        }
    }

    private static func preferredPendingUpdate(
        current: NativeRuntimeLibraryUpdate,
        offered: NativeRuntimeLibraryUpdate
    ) -> NativeRuntimeLibraryUpdate {
        let currentRevision = revision(of: current)
        let offeredRevision = revision(of: offered)
        if offeredRevision > currentRevision {
            return offered
        }
        if offeredRevision < currentRevision {
            return current
        }

        // A same-revision `next` retains predecessor metadata that can expose
        // a coalesced delivery gap. An initial authoritative replacement has
        // no such information and must not erase it merely by arriving later.
        return switch (current, offered) {
        case (.next, .authoritative):
            current
        case (.authoritative, .next):
            offered
        case (.authoritative, .authoritative), (.next, .next):
            offered
        }
    }

    private static func revision(
        of update: NativeRuntimeLibraryUpdate
    ) -> UInt64 {
        switch update {
        case .authoritative(let projection),
             .next(let projection, _, _):
            projection.revision
        }
    }

    func close() {
        lock.lock()
        isClosed = true
        pending = nil
        handler = nil
        lock.unlock()
    }

    private func scheduleDrain() {
        DispatchQueue.main.async { [weak self] in
            self?.drainOnMainQueue()
        }
    }

    private func drainOnMainQueue() {
        lock.lock()
        guard !isClosed, let update = pending, let handler else {
            isScheduled = false
            lock.unlock()
            return
        }
        pending = nil
        lock.unlock()

        MainActor.assumeIsolated {
            handler(update)
        }

        lock.lock()
        let shouldSchedule = !isClosed && pending != nil && self.handler != nil
        if !shouldSchedule {
            isScheduled = false
        }
        lock.unlock()
        if shouldSchedule {
            scheduleDrain()
        }
    }

    deinit {
        close()
    }
}
