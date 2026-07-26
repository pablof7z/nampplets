import Foundation
import NMPNativeRuntime

// MARK: - Conflated observer drain loops and revision accessors

extension NativeRuntimeProfile {
    func drainPendingLibraryUpdates(for identifier: UUID) {
        while true {
            lock.lock()
            guard !isClosed, var observer = libraryObservers[identifier] else {
                lock.unlock()
                return
            }
            guard let pendingUpdate = observer.pendingUpdate else {
                observer.isReadyForNext = true
                libraryObservers[identifier] = observer
                lock.unlock()
                return
            }
            observer.pendingUpdate = nil
            observer.lastDeliveredRevision = max(
                observer.lastDeliveredRevision,
                libraryRevision(of: pendingUpdate)
            )
            libraryObservers[identifier] = observer
            let receive = observer.receive
            lock.unlock()
            receive(pendingUpdate)
        }
    }

    func drainPendingCatalogUpdates(for identifier: UUID) {
        while true {
            lock.lock()
            guard !isClosed, var observer = catalogObservers[identifier] else {
                lock.unlock()
                return
            }
            guard let pendingUpdate = observer.pendingUpdate else {
                observer.isReadyForNext = true
                catalogObservers[identifier] = observer
                lock.unlock()
                return
            }
            observer.pendingUpdate = nil
            observer.lastDeliveredRevision = max(
                observer.lastDeliveredRevision,
                catalogRevision(of: pendingUpdate)
            )
            catalogObservers[identifier] = observer
            let receive = observer.receive
            lock.unlock()
            receive(pendingUpdate)
        }
    }

    func drainPendingWriteUpdates(for identifier: UUID) {
        while true {
            lock.lock()
            guard !isClosed, var observer = pendingWriteObservers[identifier]
            else {
                lock.unlock()
                return
            }
            guard let pendingUpdate = observer.pendingUpdate else {
                observer.isReadyForNext = true
                pendingWriteObservers[identifier] = observer
                lock.unlock()
                return
            }
            observer.pendingUpdate = nil
            observer.lastDeliveredRevision = max(
                observer.lastDeliveredRevision,
                pendingWriteRevision(of: pendingUpdate)
            )
            pendingWriteObservers[identifier] = observer
            let receive = observer.receive
            lock.unlock()
            receive(pendingUpdate)
        }
    }

    func drainReceiptUpdates(for identifier: UUID) {
        while true {
            lock.lock()
            guard !isClosed, var observer = receiptObservers[identifier]
            else {
                lock.unlock()
                return
            }
            guard let pendingUpdate = observer.pendingUpdate else {
                observer.isReadyForNext = true
                receiptObservers[identifier] = observer
                lock.unlock()
                return
            }
            observer.pendingUpdate = nil
            observer.lastDeliveredRevision = max(
                observer.lastDeliveredRevision,
                receiptRevision(of: pendingUpdate)
            )
            receiptObservers[identifier] = observer
            let receive = observer.receive
            lock.unlock()
            receive(pendingUpdate)
        }
    }

    func libraryRevision(
        of update: NativeRuntimeLibraryUpdate
    ) -> UInt64 {
        switch update {
        case let .authoritative(projection),
             let .next(projection, _, _, _):
            projection.revision
        }
    }

    func catalogRevision(
        of update: NativeRuntimeCatalogUpdate
    ) -> UInt64 {
        switch update {
        case let .authoritative(snapshot),
             let .next(snapshot, _):
            snapshot.revision
        }
    }

    func pendingWriteRevision(
        of update: NativeRuntimePendingWriteUpdate
    ) -> UInt64 {
        switch update {
        case let .authoritative(projection),
             let .next(projection, _, _):
            projection.revision
        }
    }

    func receiptRevision(
        of update: NativeRuntimeReceiptUpdate
    ) -> UInt64 {
        switch update {
        case let .authoritative(projection),
             let .next(projection, _, _):
            projection.revision
        }
    }
}
