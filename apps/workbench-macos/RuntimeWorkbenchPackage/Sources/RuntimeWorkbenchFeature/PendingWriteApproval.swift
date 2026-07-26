import Foundation
import NMPNativeRuntimeApple
import SwiftUI

/// Main-actor presentation model for the Rust-owned pending-write projection.
/// It retains no draft authority: approval forwards only the opaque operation
/// id back to the native profile.
@MainActor
final class RuntimeWorkbenchPendingWriteModel: ObservableObject {
    @Published private(set) var writes: [NativeRuntimePendingWrite] = []
    /// Set when `observePendingWrites` itself could not be established. An
    /// empty `writes` array is indistinguishable from "no napplet has asked
    /// to write" — this field is what lets the chrome tell a napplet stuck
    /// waiting on approval apart from a genuinely idle one.
    @Published private(set) var observationFailure: String?

    private var observation: NativeRuntimePendingWriteObservation?

    init(profile: WorkbenchRuntimeProfile?) {
        guard let profile else { return }
        do {
            observation = try profile.native.observePendingWrites {
                [weak self] update in
                Task { @MainActor [weak self] in
                    self?.receive(update)
                }
            }
        } catch {
            writes = []
            observationFailure = error.localizedDescription
        }
    }

    func decide(
        _ write: NativeRuntimePendingWrite,
        approve: Bool,
        profile: WorkbenchRuntimeProfile?
    ) {
        profile?.native.decideProviderWrite(
            operationID: write.id,
            approve: approve
        )
    }

    private func receive(_ update: NativeRuntimePendingWriteUpdate) {
        switch update {
        case let .authoritative(projection),
             let .next(projection, _, _):
            writes = projection.writes
        }
    }

    deinit {
        observation?.cancel()
    }
}

/// Keeps the latest bounded canonical receipt projection visible after the
/// originating napplet/session changes state or closes.
@MainActor
final class RuntimeWorkbenchReceiptModel: ObservableObject {
    @Published private(set) var receipts: [NativeRuntimeReceipt] = []
    @Published private(set) var receiptIDs: [String] = []
    /// Set when `observeReceipts` itself could not be established. Kept
    /// distinct from an empty `receipts` array for the same reason as
    /// `RuntimeWorkbenchPendingWriteModel.observationFailure`: silence must
    /// not read as "nothing has happened yet."
    @Published private(set) var observationFailure: String?

    private var observation: NativeRuntimeReceiptObservation?

    init(profile: WorkbenchRuntimeProfile?) {
        guard let profile else { return }
        do {
            observation = try profile.native.observeReceipts {
                [weak self] update in
                Task { @MainActor [weak self] in
                    self?.receive(update)
                }
            }
        } catch {
            receipts = []
            observationFailure = error.localizedDescription
        }
    }

    private func receive(_ update: NativeRuntimeReceiptUpdate) {
        switch update {
        case let .authoritative(projection),
             let .next(projection, _, _):
            receipts = projection.receipts
            receiptIDs = projection.receipts.map(\.id)
        }
    }

    deinit {
        observation?.cancel()
    }
}

// The approval and receipt views live in dedicated presentation files.
