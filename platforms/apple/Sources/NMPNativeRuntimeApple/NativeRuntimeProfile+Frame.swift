import Foundation
import NMPNativeRuntime

// MARK: - Rust observation frame fanout

extension NativeRuntimeProfile {
    public func update(frame: RuntimeObservationFrame) {
        lock.lock()
        if isClosed {
            lock.unlock()
            return
        }
        sessions = sessions.filter { $0.value.value != nil }
        let activeSessions = sessions.values.compactMap(\.value)
        let previousActivityRevision = lastActivityRevision
        let previousLibraryRevision = lastLibraryRevision
        let previousCatalogRevision = lastCatalogSnapshot.revision
        let previousPendingWriteRevision = lastPendingWriteRevision
        let previousReceiptRevision = lastReceiptRevision
        let snapshotProjection = NativeRuntimeSnapshotProjection(
            frame.snapshot,
            unknownRevision: lastAcceptedSnapshot.revision,
            unknownProfileClosed: lastAcceptedSnapshot.closed
        )
        let snapshot: RuntimeSnapshot?
        switch snapshotProjection {
        case let .snapshot(accepted):
            snapshot = accepted
            lastAcceptedSnapshot = accepted
            lastActivityRevision = max(
                lastActivityRevision,
                accepted.revision
            )
            lastPendingWriteRevision = max(
                lastPendingWriteRevision,
                accepted.revision
            )
            lastReceiptRevision = max(
                lastReceiptRevision,
                accepted.revision
            )
        case .refused:
            snapshot = nil
        }
        lastLibraryRevision = max(
            lastLibraryRevision,
            snapshotProjection.revision
        )
        if frame.catalog.revision >= previousCatalogRevision {
            lastCatalogSnapshot = frame.catalog
        }
        let activityObservers = Array(activityObservers.values)
        var libraryDeliveries: [
            (receive: LibraryReceiver, update: NativeRuntimeLibraryUpdate)
        ] = []
        var catalogDeliveries: [
            (receive: CatalogReceiver, update: NativeRuntimeCatalogUpdate)
        ] = []
        var pendingWriteDeliveries: [
            (receive: PendingWriteReceiver, update: NativeRuntimePendingWriteUpdate)
        ] = []
        var receiptDeliveries: [
            (receive: ReceiptReceiver, update: NativeRuntimeReceiptUpdate)
        ] = []
        if snapshotProjection.revision > previousLibraryRevision
            || frame.eventCursorWasStale
        {
            let projection = NativeRuntimeLibraryProjection(snapshotProjection)
            let update = NativeRuntimeLibraryUpdate.next(
                projection,
                predecessorRevision: previousLibraryRevision,
                eventCursorWasStale: frame.eventCursorWasStale,
                lostBeforeBatch: frame.lostBeforeBatch
            )
            for identifier in Array(libraryObservers.keys) {
                guard var observer = libraryObservers[identifier] else {
                    continue
                }
                let isNewer = projection.revision
                    > observer.lastDeliveredRevision
                let isCurrentStaleReplacement = frame.eventCursorWasStale
                    && projection.revision
                        == observer.lastDeliveredRevision
                guard isNewer || isCurrentStaleReplacement else {
                    continue
                }
                if observer.isReadyForNext {
                    observer.lastDeliveredRevision = projection.revision
                    libraryObservers[identifier] = observer
                    libraryDeliveries.append((observer.receive, update))
                    continue
                }
                if let pendingUpdate = observer.pendingUpdate,
                   projection.revision < libraryRevision(of: pendingUpdate)
                {
                    continue
                }
                observer.pendingUpdate = update
                libraryObservers[identifier] = observer
            }
        }
        if frame.catalog.revision > previousCatalogRevision {
            let update = NativeRuntimeCatalogUpdate.next(
                frame.catalog,
                predecessorRevision: previousCatalogRevision
            )
            for identifier in Array(catalogObservers.keys) {
                guard var observer = catalogObservers[identifier],
                      frame.catalog.revision
                          > observer.lastDeliveredRevision
                else {
                    continue
                }
                if observer.isReadyForNext {
                    observer.lastDeliveredRevision = frame.catalog.revision
                    catalogObservers[identifier] = observer
                    catalogDeliveries.append((observer.receive, update))
                    continue
                }
                if let pending = observer.pendingUpdate,
                   catalogRevision(of: pending) > frame.catalog.revision
                {
                    continue
                }
                observer.pendingUpdate = update
                catalogObservers[identifier] = observer
            }
        }
        if let snapshot,
           snapshot.revision > previousPendingWriteRevision
            || frame.eventCursorWasStale
        {
            let projection = NativeRuntimePendingWriteProjection(snapshot)
            let update = NativeRuntimePendingWriteUpdate.next(
                projection,
                predecessorRevision: previousPendingWriteRevision,
                eventCursorWasStale: frame.eventCursorWasStale
            )
            for identifier in Array(pendingWriteObservers.keys) {
                guard var observer = pendingWriteObservers[identifier] else {
                    continue
                }
                let isNewer = projection.revision
                    > observer.lastDeliveredRevision
                let isCurrentStaleReplacement = frame.eventCursorWasStale
                    && projection.revision
                        == observer.lastDeliveredRevision
                guard isNewer || isCurrentStaleReplacement else {
                    continue
                }
                if observer.isReadyForNext {
                    observer.lastDeliveredRevision = projection.revision
                    pendingWriteObservers[identifier] = observer
                    pendingWriteDeliveries.append((observer.receive, update))
                    continue
                }
                if let pendingUpdate = observer.pendingUpdate,
                   projection.revision
                        < pendingWriteRevision(of: pendingUpdate)
                {
                    continue
                }
                observer.pendingUpdate = update
                pendingWriteObservers[identifier] = observer
            }
        }
        if let snapshot,
           snapshot.revision > previousReceiptRevision
            || frame.eventCursorWasStale
        {
            let projection = NativeRuntimeReceiptProjection(snapshot)
            let update = NativeRuntimeReceiptUpdate.next(
                projection,
                predecessorRevision: previousReceiptRevision,
                eventCursorWasStale: frame.eventCursorWasStale
            )
            for identifier in Array(receiptObservers.keys) {
                guard var observer = receiptObservers[identifier] else {
                    continue
                }
                let isNewer = projection.revision > observer.lastDeliveredRevision
                let isCurrentStaleReplacement = frame.eventCursorWasStale
                    && projection.revision == observer.lastDeliveredRevision
                guard isNewer || isCurrentStaleReplacement else { continue }
                if observer.isReadyForNext {
                    observer.lastDeliveredRevision = projection.revision
                    receiptObservers[identifier] = observer
                    receiptDeliveries.append((observer.receive, update))
                    continue
                }
                if let pendingUpdate = observer.pendingUpdate,
                   projection.revision < receiptRevision(of: pendingUpdate)
                {
                    continue
                }
                observer.pendingUpdate = update
                receiptObservers[identifier] = observer
            }
        }
        lock.unlock()
        if let snapshot {
            settingsExecutor.retainRunningSessions(
                Set(
                    snapshot.sessions
                        // A degraded session is still live and still owns its
                        // settings; dropping it here would revoke config from
                        // exactly the sessions already missing something.
                        .filter {
                            $0.state == "running" || $0.state == "running-degraded"
                        }
                        .map(\.id)
                )
            )
        }
        for session in activeSessions {
            session.deliver(frame: frame)
        }
        if let snapshot,
           snapshot.revision > previousActivityRevision
            || frame.eventCursorWasStale
        {
            for observer in activityObservers {
                observer.receive(
                    .next(
                        NativeRuntimeActivityProjection(
                            snapshot,
                            scope: observer.scope
                        ),
                        predecessorRevision: previousActivityRevision,
                        eventCursorWasStale: frame.eventCursorWasStale,
                        lostBeforeBatch: frame.lostBeforeBatch
                    )
                )
            }
        }
        for delivery in libraryDeliveries {
            delivery.receive(delivery.update)
        }
        for delivery in catalogDeliveries {
            delivery.receive(delivery.update)
        }
        for delivery in pendingWriteDeliveries {
            delivery.receive(delivery.update)
        }
        for delivery in receiptDeliveries {
            delivery.receive(delivery.update)
        }
    }
}
