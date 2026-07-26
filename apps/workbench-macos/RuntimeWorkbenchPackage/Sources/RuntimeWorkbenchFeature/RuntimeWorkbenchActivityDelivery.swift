import Foundation
import NMPNativeRuntimeApple

@MainActor
final class RuntimeWorkbenchActivitySubscription: ActivitySubscription {
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

/// One-slot replacement mailbox. At most one main-queue work item is pending;
/// newer runtime updates replace the pending value and retain predecessor
/// evidence so the presentation can make a delivery gap visible.
final class RuntimeActivityUpdateMailbox: @unchecked Sendable {
    typealias Handler =
        @MainActor @Sendable (NativeRuntimeActivityUpdate) -> Void

    private let lock = NSLock()
    private var handler: Handler?
    private var pending: NativeRuntimeActivityUpdate?
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

    func offer(_ update: NativeRuntimeActivityUpdate) {
        lock.lock()
        guard !isClosed else {
            lock.unlock()
            return
        }
        pending = update
        let shouldSchedule = handler != nil && !isScheduled
        if shouldSchedule {
            isScheduled = true
        }
        lock.unlock()
        if shouldSchedule {
            scheduleDrain()
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
