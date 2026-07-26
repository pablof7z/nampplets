import SwiftUI

extension ContentView {
    var nappletInspector: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Label("Details", systemImage: "info.circle")
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
            VStack(alignment: .leading, spacing: NappletMetrics.snug) {
                Text(window.title)
                    .font(.title3.weight(.semibold))
                LabeledContent(
                    "Status",
                    value: window.exactBuild.flatMap {
                        runningArtifacts[$0]
                    } == nil ? "Not open" : "Running"
                )
                LabeledContent("Layout", value: layout.mode.title)
                LabeledContent(
                    "Size",
                    value: "\(Int(window.frame.width)) × \(Int(window.frame.height))"
                )

                // A twelve-character prefix of a hash is the worst of both:
                // meaningless to a person, and useless for the comparison a
                // technical reader would want. The whole value lives here.
                if let exactBuild = window.exactBuild {
                    NappletEvidence {
                        NappletFieldGrid(fields: [
                            NappletField(
                                "Publisher key",
                                exactBuild.manifestAuthor
                            ),
                            NappletField("dTag", exactBuild.dTag),
                            NappletField(
                                "Aggregate hash",
                                exactBuild.aggregateHash
                            ),
                        ])
                    }
                    .font(.caption)
                }
            }

            if let nativeActionNotice {
                Divider()
                VStack(alignment: .leading, spacing: NappletMetrics.tight) {
                    Label(
                        nativeActionNotice.title,
                        systemImage: nativeActionNotice.kind == .composeOpen
                            ? "square.and.pencil"
                            : "arrow.up.right"
                    )
                    .font(.subheadline.weight(.semibold))
                    Text(nativeActionNotice.target)
                        .font(.callout)
                        .textSelection(.enabled)
                        .lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)
                    Text(nativeActionNotice.detail)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    Button("Dismiss") {
                        self.nativeActionNotice = nil
                    }
                    .buttonStyle(.borderless)
                }
                .accessibilityIdentifier("native-action-notice")
            }

            Divider()

            Button("Permissions", systemImage: "lock.shield") {
                openPermissionReview()
            }
            Button("Activity", systemImage: "waveform.path.ecg") {
                openActivityDrawer()
            }
        } else {
            ContentUnavailableView(
                "Nothing selected",
                systemImage: "cursorarrow.click",
                description: Text("Pick a napplet window to see its details.")
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
