import SwiftUI

extension ContentView {
    var nappletInspector: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Label("Napplet Inspector", systemImage: "info.circle")
                    .font(.headline)
                Spacer()
                Button {
                    withAnimation(.easeInOut(duration: 0.18)) {
                        isInspectorPresented = false
                    }
                } label: {
                    Image(systemName: "xmark")
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Close napplet inspector")
            }

            Picker("Inspector section", selection: $inspectorTab) {
                ForEach(InspectorTab.allCases) { tab in
                    Text(tab.title).tag(tab)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .accessibilityIdentifier("inspector-tab-picker")

            Divider()

            switch inspectorTab {
            case .overview:
                inspectorOverviewTab
            case .relays:
                inspectorRelaysTab
            }

            Spacer()
        }
        .padding(16)
        .frame(width: 290)
        .background(.bar)
        .accessibilityIdentifier("napplet-inspector")
    }

    @ViewBuilder
    private var inspectorOverviewTab: some View {
        if let window = layout.selectedWindow {
            VStack(alignment: .leading, spacing: 12) {
                Text(window.title)
                    .font(.title3.weight(.semibold))
                LabeledContent(
                    "Status",
                    value: window.exactBuild.flatMap {
                        runningArtifacts[$0]
                    } == nil ? "Not running" : "Running"
                )
                LabeledContent("Layout", value: layout.mode.title)
                LabeledContent(
                    "Window",
                    value: "\(Int(window.frame.width)) × \(Int(window.frame.height))"
                )
                if let exactBuild = window.exactBuild {
                    LabeledContent("Build") {
                        Text(String(exactBuild.aggregateHash.prefix(12)))
                            .font(.system(.caption, design: .monospaced))
                            .textSelection(.enabled)
                    }
                }
            }

            if let nativeActionNotice {
                Divider()
                VStack(alignment: .leading, spacing: 8) {
                    Label(
                        nativeActionNotice.title,
                        systemImage: nativeActionNotice.kind == .composeOpen
                            ? "square.and.pencil"
                            : "arrow.up.right"
                    )
                    .font(.subheadline.weight(.semibold))
                    Text(nativeActionNotice.target)
                        .font(.system(.caption, design: .monospaced))
                        .textSelection(.enabled)
                    Text(nativeActionNotice.detail)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Button("Dismiss action") {
                        self.nativeActionNotice = nil
                    }
                    .buttonStyle(.borderless)
                }
                .accessibilityIdentifier("native-action-notice")
            }

            Divider()

            Button("Review Permissions", systemImage: "lock.shield") {
                openPermissionReview()
            }
            Button("View Activity", systemImage: "waveform.path.ecg") {
                openActivityDrawer()
            }
        } else {
            ContentUnavailableView(
                "No napplet selected",
                systemImage: "cursorarrow.click",
                description: Text("Select a napplet window to inspect it.")
            )
        }
    }

    @ViewBuilder
    private var inspectorRelaysTab: some View {
        if let profile {
            RelayDiagnosticsInspectorView(
                source: RuntimeWorkbenchRelayDiagnosticsSource(profile: profile)
            )
        } else {
            ContentUnavailableView(
                "Relays unavailable",
                systemImage: "antenna.radiowaves.left.and.right.slash",
                description: Text(
                    bootstrapError ?? "The application runtime profile is unavailable."
                )
            )
        }
    }
}
