import SwiftUI

public struct WorkbenchSettingsSheet: View {
    @Environment(\.dismiss) private var dismiss

    private let snapshot: WorkbenchSettingsSnapshot
    private let openDestination: (WorkbenchSettingsDestination) -> Void
    private let performAction: WorkbenchProfileActionHandler

    @State private var draft: WorkbenchProfilePreferences
    @State private var actionError: String?
    @State private var isSaving = false
    @State private var isClearing = false
    @State private var showsClearConfirmation = false

    public init(
        snapshot: WorkbenchSettingsSnapshot,
        openDestination: @escaping (WorkbenchSettingsDestination) -> Void,
        performAction: @escaping WorkbenchProfileActionHandler
    ) {
        self.snapshot = snapshot
        self.openDestination = openDestination
        self.performAction = performAction
        _draft = State(
            initialValue: snapshot.preferences
                ?? WorkbenchProfilePreferences(
                    appRelays: [],
                    indexerRelays: [],
                    permissionDefault: .askEveryTime
                )
        )
    }

    public var body: some View {
        NavigationStack {
            Form {
                availabilitySection
                connectionsSection
                permissionsSection
                storageSection
                moreSection
            }
            .formStyle(.grouped)
            .navigationTitle("Preferences")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(isSaving ? "Saving…" : "Save") {
                        save()
                    }
                    .disabled(!canSave)
                    .accessibilityIdentifier("settings-save")
                }
            }
        }
        .alert(
            "Clear Network Cache?",
            isPresented: $showsClearConfirmation
        ) {
            Button("Cancel", role: .cancel) {}
            Button("Clear Cache", role: .destructive) {
                clearNetworkCache()
            }
        } message: {
            Text(
                "This removes saved network activity and delivery history, "
                    + "including anything still waiting to send. Installed "
                    + "napplets, permissions, settings, and your account stay."
            )
        }
        #if os(macOS)
        .frame(minWidth: 640, minHeight: 620)
        #endif
    }

    @ViewBuilder
    private var availabilitySection: some View {
        if case let .unavailable(reason) = snapshot.profileStatus {
            Section {
                Label(reason, systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.secondary)
            }
        }
        if let actionError {
            Section {
                Label(actionError, systemImage: "exclamationmark.circle")
                    .foregroundStyle(.red)
                    .accessibilityIdentifier("settings-error")
            }
        }
    }

    private var connectionsSection: some View {
        Section {
            WorkbenchRelayLaneEditor(
                title: "Everyday relays",
                detail: "Keep your napplets connected and in sync.",
                identifierPrefix: "app",
                relays: $draft.appRelays
            )
            WorkbenchRelayLaneEditor(
                title: "Search relays",
                detail: "Help find napplets and people.",
                identifierPrefix: "indexer",
                relays: $draft.indexerRelays
            )
        } header: {
            Text("Connections")
        } footer: {
            Text("Only secure wss:// addresses are accepted.")
        }
        .disabled(snapshot.preferences == nil || isBusy)
    }

    private var permissionsSection: some View {
        Section {
            Picker(
                "When a new napplet asks",
                selection: $draft.permissionDefault
            ) {
                ForEach(WorkbenchPermissionDefault.allCases) { choice in
                    Text(choice.title).tag(choice)
                }
            }
            Text(draft.permissionDefault.detail)
                .font(.caption)
                .foregroundStyle(.secondary)
        } header: {
            Text("Permission choices")
        } footer: {
            Text(
                "This is the choice selected on the review screen. "
                    + "You always approve before a napplet opens."
            )
        }
        .disabled(snapshot.preferences == nil || isBusy)
    }

    private var storageSection: some View {
        Section {
            storageRow(
                "Network cache",
                bytes: snapshot.storage?.networkBytes
            )
            storageRow(
                "Napplets and settings",
                bytes: snapshot.storage?.appBytes
            )
            storageRow(
                snapshot.storage?.isEstimate == true
                    ? "Total (at least)"
                    : "Total",
                bytes: snapshot.storage?.totalBytes
            )
            Button(role: .destructive) {
                showsClearConfirmation = true
            } label: {
                if isClearing {
                    Label("Clearing…", systemImage: "hourglass")
                } else {
                    Label("Clear Network Cache…", systemImage: "trash")
                }
            }
            .disabled(snapshot.storage == nil || isBusy)
            .accessibilityIdentifier("settings-clear-network-cache")
        } header: {
            Text("Storage")
        } footer: {
            Text(
                "Clearing the network cache keeps napplets, preferences, "
                    + "permissions, and accounts."
            )
        }
    }

    private var moreSection: some View {
        Section("More") {
            ForEach(
                [
                    WorkbenchSettingsDestination.account,
                    .installedLibrary,
                    .activity,
                ],
                id: \.self
            ) { destination in
                Button {
                    openDestination(destination)
                    dismiss()
                } label: {
                    Label {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(destination.title)
                            Text(destination.detail)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    } icon: {
                        Image(systemName: destination.systemImage)
                    }
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier(
                    destination.accessibilityIdentifier
                )
            }
        }
    }

    private var isBusy: Bool {
        isSaving || isClearing
    }

    private var normalizedDraft: WorkbenchProfilePreferences? {
        try? draft.normalized()
    }

    private var canSave: Bool {
        guard
            !isBusy,
            let original = snapshot.preferences,
            let normalizedDraft
        else {
            return false
        }
        return normalizedDraft != original
    }

    private func storageRow(
        _ title: String,
        bytes: UInt64?
    ) -> some View {
        HStack {
            Text(title)
            Spacer()
            Text(bytes.map(Self.formattedBytes) ?? "Unavailable")
                .foregroundStyle(.secondary)
                .monospacedDigit()
        }
    }

    private static func formattedBytes(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(
            fromByteCount: Int64(clamping: bytes),
            countStyle: .file
        )
    }

    private func save() {
        guard let normalizedDraft else {
            actionError = (try? draft.normalized()) == nil
                ? "Check that each relay is a unique secure wss:// address."
                : nil
            return
        }
        isSaving = true
        actionError = nil
        Task { @MainActor in
            do {
                try await performAction(
                    .savePreferences(normalizedDraft)
                )
                dismiss()
            } catch {
                actionError = error.localizedDescription
                isSaving = false
            }
        }
    }

    private func clearNetworkCache() {
        isClearing = true
        actionError = nil
        Task { @MainActor in
            do {
                try await performAction(.clearNetworkCache)
                dismiss()
            } catch {
                actionError = error.localizedDescription
                isClearing = false
            }
        }
    }
}
