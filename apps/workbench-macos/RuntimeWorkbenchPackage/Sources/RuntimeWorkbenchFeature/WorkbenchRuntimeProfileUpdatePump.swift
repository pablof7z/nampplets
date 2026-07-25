import Foundation

final class WorkbenchCatalogChangeMailbox: @unchecked Sendable {
    private let lock = NSLock()
    private let receive: @MainActor @Sendable () -> Void
    private var pending = false
    private var scheduled = false
    private var closed = false

    init(receive: @escaping @MainActor @Sendable () -> Void) {
        self.receive = receive
    }

    func offer() {
        lock.lock()
        guard !closed else {
            lock.unlock()
            return
        }
        pending = true
        let shouldSchedule = !scheduled
        if shouldSchedule {
            scheduled = true
        }
        lock.unlock()
        if shouldSchedule {
            scheduleDrain()
        }
    }

    func close() {
        lock.lock()
        closed = true
        pending = false
        lock.unlock()
    }

    private func scheduleDrain() {
        Task { @MainActor [weak self] in
            self?.drain()
        }
    }

    @MainActor
    private func drain() {
        lock.lock()
        guard !closed else {
            scheduled = false
            lock.unlock()
            return
        }
        pending = false
        lock.unlock()

        receive()

        lock.lock()
        let shouldSchedule = pending && !closed
        if !shouldSchedule {
            scheduled = false
        }
        lock.unlock()
        if shouldSchedule {
            scheduleDrain()
        }
    }
}
