import Foundation
import NMPNativeRuntime

// MARK: - Application observer registration and cancellation

extension NativeRuntimeProfile {
    /// Returns the latest bounded set of Rust-retained provider writes.
    public func pendingWriteProjection()
        throws -> NativeRuntimePendingWriteProjection
    {
        NativeRuntimePendingWriteProjection(
            try validatedSnapshot()
        )
    }

    /// Observes the profile-owned pending-write replacement stream. The
    /// callback receives an authoritative replacement synchronously, followed
    /// by conflated latest updates from the permanent Rust observation.
    public func observePendingWrites(
        _ receive: @escaping @Sendable (NativeRuntimePendingWriteUpdate) -> Void
    ) throws -> NativeRuntimePendingWriteObservation {
        let snapshot = try validatedSnapshot()
        lock.lock()
        guard !isClosed else {
            lock.unlock()
            throw NativeRuntimePendingWriteObservationError.profileClosed
        }
        guard pendingWriteObservers.count
            < Self.maximumApplicationPendingWriteObservers
        else {
            lock.unlock()
            throw NativeRuntimePendingWriteObservationError.observerCapacity(
                maximum: Self.maximumApplicationPendingWriteObservers
            )
        }
        let identifier = UUID()
        let authoritative = NativeRuntimePendingWriteProjection(
            snapshot
        )
        pendingWriteObservers[identifier] = PendingWriteObserverEntry(
            receive: receive,
            lastDeliveredRevision: authoritative.revision
        )
        lock.unlock()

        let observation = NativeRuntimePendingWriteObservation { [weak self] in
            self?.removePendingWriteObserver(identifier)
        }
        receive(.authoritative(authoritative))
        drainPendingWriteUpdates(for: identifier)
        return observation
    }

    /// Observes the bounded durable receipt replacement owned by the profile.
    /// Delivery state is presented mechanically; native does not infer an
    /// outcome from relay payloads.
    public func observeReceipts(
        _ receive: @escaping @Sendable (NativeRuntimeReceiptUpdate) -> Void
    ) throws -> NativeRuntimeReceiptObservation {
        let snapshot = try validatedSnapshot()
        lock.lock()
        guard !isClosed else {
            lock.unlock()
            throw NativeRuntimeReceiptObservationError.profileClosed
        }
        guard receiptObservers.count < Self.maximumApplicationReceiptObservers
        else {
            lock.unlock()
            throw NativeRuntimeReceiptObservationError.observerCapacity(
                maximum: Self.maximumApplicationReceiptObservers
            )
        }
        let identifier = UUID()
        let authoritative = NativeRuntimeReceiptProjection(
            snapshot
        )
        receiptObservers[identifier] = ReceiptObserverEntry(
            receive: receive,
            lastDeliveredRevision: authoritative.revision
        )
        lock.unlock()

        let observation = NativeRuntimeReceiptObservation { [weak self] in
            self?.removeReceiptObserver(identifier)
        }
        receive(.authoritative(authoritative))
        drainReceiptUpdates(for: identifier)
        return observation
    }

    /// Returns the latest complete installed-library replacement from the
    /// Rust-owned profile snapshot.
    public func installedLibraryProjection()
        -> NativeRuntimeLibraryProjection
    {
        NativeRuntimeLibraryProjection(
            pullSnapshotProjection()
        )
    }

    /// Returns the latest complete, bounded runtime activity replacement.
    public func activityProjection(
        for scope: NativeRuntimeActivityScope
    ) throws -> NativeRuntimeActivityProjection {
        NativeRuntimeActivityProjection(
            try validatedSnapshot(),
            scope: scope
        )
    }

    /// Returns the last NMP relay and wire-subscription read-out.
    ///
    /// It is only refreshed while an observation is open. Check `observing`:
    /// empty `relays` with `observing` false means the read-out is not
    /// currently accounted, never that the engine planned no relay session.
    public func relayDiagnostics() -> NativeRuntimeRelayDiagnosticsSnapshot {
        controller.relayDiagnostics()
    }

    /// Opens the Rust-owned NMP diagnostics observation for as long as the
    /// returned handle lives, and delivers the current read-out synchronously.
    ///
    /// Unlike the activity and catalog observers, this starts and stops real
    /// NMP relay accounting rather than joining an always-running fanout.
    public func observeRelayDiagnostics(
        _ receive: @escaping @Sendable (NativeRuntimeRelayDiagnosticsSnapshot)
            -> Void
    ) throws -> NativeRuntimeRelayDiagnosticsObservation {
        lock.lock()
        guard !isClosed else {
            lock.unlock()
            throw NativeRuntimeRelayDiagnosticsObservationError.profileClosed
        }
        lock.unlock()

        let start = controller.observeRelayDiagnostics(
            observer: NativeRuntimeRelayDiagnosticsForwarder(receive: receive)
        )
        guard let observation = start.observation else {
            let refusal = start.refusal
            throw NativeRuntimeRelayDiagnosticsObservationError.refused(
                code: refusal?.code ?? "relay-diagnostics-observe",
                detail: refusal?.detail
                    ?? "the runtime returned no diagnostics observation"
            )
        }
        return NativeRuntimeRelayDiagnosticsObservation {
            observation.stop()
        }
    }

    /// Adds one bounded application observer to the profile's permanent NMP
    /// catalog feed. Registration synchronously delivers the latest complete
    /// replacement; subsequent updates are conflated to one pending latest
    /// value while that authoritative callback is in flight.
    public func observeCatalog(
        _ receive: @escaping @Sendable (NativeRuntimeCatalogUpdate) -> Void
    ) throws -> NativeRuntimeCatalogObservation {
        lock.lock()
        guard !isClosed else {
            lock.unlock()
            throw NativeRuntimeCatalogObservationError.profileClosed
        }
        guard catalogObservers.count
            < Self.maximumApplicationCatalogObservers
        else {
            lock.unlock()
            throw NativeRuntimeCatalogObservationError.observerCapacity(
                maximum: Self.maximumApplicationCatalogObservers
            )
        }
        let identifier = UUID()
        let authoritative = lastCatalogSnapshot
        catalogObservers[identifier] = CatalogObserverEntry(
            receive: receive,
            lastDeliveredRevision: authoritative.revision
        )
        lock.unlock()

        let observation = NativeRuntimeCatalogObservation { [weak self] in
            self?.removeCatalogObserver(identifier)
        }
        receive(.authoritative(authoritative))
        drainPendingCatalogUpdates(for: identifier)
        return observation
    }

    /// Adds one bounded application observer to the profile's single Rust
    /// observation stream. Admission refusal is explicit, and the receiver is
    /// called synchronously with an authoritative replacement before return.
    public func observeActivity(
        scope: NativeRuntimeActivityScope,
        _ receive: @escaping @Sendable (NativeRuntimeActivityUpdate) -> Void
    ) throws -> NativeRuntimeActivityObservation {
        let snapshot = try validatedSnapshot()
        lock.lock()
        guard !isClosed else {
            lock.unlock()
            throw NativeRuntimeActivityObservationError.profileClosed
        }
        guard activityObservers.count
            < Self.maximumApplicationActivityObservers
        else {
            lock.unlock()
            throw NativeRuntimeActivityObservationError.observerCapacity(
                maximum: Self.maximumApplicationActivityObservers
            )
        }
        let identifier = UUID()
        activityObservers[identifier] = ActivityObserverEntry(
            scope: scope,
            receive: receive
        )
        lock.unlock()

        let observation = NativeRuntimeActivityObservation {
            [weak self] in
            self?.removeActivityObserver(identifier)
        }
        receive(
            .authoritative(
                NativeRuntimeActivityProjection(
                    snapshot,
                    scope: scope
                )
            )
        )
        return observation
    }

    /// Adds one bounded application observer to the installed-library view on
    /// the profile's existing Rust observation stream.
    public func observeInstalledLibrary(
        _ receive: @escaping @Sendable (NativeRuntimeLibraryUpdate) -> Void
    ) throws -> NativeRuntimeLibraryObservation {
        let pull = pullSnapshotProjection()
        lock.lock()
        guard !isClosed else {
            lock.unlock()
            throw NativeRuntimeLibraryObservationError.profileClosed
        }
        guard libraryObservers.count
            < Self.maximumApplicationLibraryObservers
        else {
            lock.unlock()
            throw NativeRuntimeLibraryObservationError.observerCapacity(
                maximum: Self.maximumApplicationLibraryObservers
            )
        }
        let identifier = UUID()
        let authoritative = NativeRuntimeLibraryProjection(
            pull
        )
        libraryObservers[identifier] = LibraryObserverEntry(
            receive: receive,
            lastDeliveredRevision: authoritative.revision
        )
        lock.unlock()

        let observation = NativeRuntimeLibraryObservation { [weak self] in
            self?.removeLibraryObserver(identifier)
        }
        receive(.authoritative(authoritative))
        drainPendingLibraryUpdates(for: identifier)
        return observation
    }

    private func removeActivityObserver(_ identifier: UUID) {
        lock.lock()
        activityObservers.removeValue(forKey: identifier)
        lock.unlock()
    }

    private func removeLibraryObserver(_ identifier: UUID) {
        lock.lock()
        libraryObservers.removeValue(forKey: identifier)
        lock.unlock()
    }

    private func removeCatalogObserver(_ identifier: UUID) {
        lock.lock()
        catalogObservers.removeValue(forKey: identifier)
        lock.unlock()
    }

    private func removePendingWriteObserver(_ identifier: UUID) {
        lock.lock()
        pendingWriteObservers.removeValue(forKey: identifier)
        lock.unlock()
    }

    private func removeReceiptObserver(_ identifier: UUID) {
        lock.lock()
        receiptObservers.removeValue(forKey: identifier)
        lock.unlock()
    }
}
