import SwiftUI

extension ContentView {
    var nappletInspector: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Inspector")
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
                .help("Close Inspector")
                .accessibilityLabel("Close Inspector")
            }

            Picker("Inspector section", selection: $inspectorTab) {
                ForEach(InspectorTab.allCases) { tab in
                    Text(tab.title).tag(tab)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .help("Choose what to inspect")

            Divider()

            switch inspectorTab {
            case .overview:
                inspectorOverviewTab
            case .relays:
                inspectorRelaysTab
            case .console:
                inspectorConsoleTab
            }

            Spacer()
        }
        .padding(16)
        .frame(width: 320)
        .background(.bar)
    }

    @ViewBuilder
    private var inspectorOverviewTab: some View {
        if let window = layout.selectedWindow {
            VStack(alignment: .leading, spacing: NappletMetrics.snug) {
                Text(window.title)
                    .font(.title3.weight(.semibold))
                LabeledContent("Status", value: nappletStatus(for: window))
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
                    Text(nativeActionNotice.summary)
                        .font(.callout)
                        .fixedSize(horizontal: false, vertical: true)
                    if !nativeActionNotice.evidence.isEmpty {
                        NappletEvidence {
                            NappletFieldGrid(
                                fields: nativeActionNotice.evidence
                            )
                        }
                        .font(.caption)
                    }
                    Button("Dismiss") {
                        self.nativeActionNotice = nil
                    }
                    .buttonStyle(.borderless)
                }
            }

            Divider()

            Button("Access", systemImage: "lock.shield") {
                openPermissionReview()
            }
            .help("Review this napplet’s access")
            .accessibilityIdentifier("inspector-access")

            Button("Activity", systemImage: "waveform.path.ecg") {
                openActivityDrawer()
            }
            .help("View this napplet’s activity")
            .accessibilityIdentifier("inspector-activity")

            DisclosureGroup("Runtime details") {
                VStack(alignment: .leading, spacing: 8) {
                    Text(activity.message)
                    if let layoutNotice =
                        layoutPersistenceError
                        ?? layout.capacityWarningMessage
                    {
                        Text(layoutNotice)
                    }
                }
                .font(.caption)
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
                .padding(.top, 4)
            }
            .font(.caption.weight(.medium))
            .accessibilityIdentifier("inspector-runtime-details")
        } else {
            ContentUnavailableView(
                "Nothing selected",
                systemImage: "cursorarrow.click",
                description: Text("Pick a napplet window to see its details.")
            )
        }
    }

    @ViewBuilder
    private var inspectorConsoleTab: some View {
        let identity = layout.selectedWindow?.exactBuild
        NappletConsoleTabView(
            entries: consoleLog.entries(for: identity)
        ) {
            if let identity {
                consoleLog.clear(for: identity)
            }
        }
    }

    @ViewBuilder
    private var inspectorRelaysTab: some View {
        if let profile {
            RelayDiagnosticsInspectorView(
                source: RuntimeWorkbenchRelayDiagnosticsSource(profile: profile)
            )
            .accessibilityIdentifier("inspector-network")
        } else {
            let presentation = WorkbenchUnavailablePresentation.relays(
                detail: bootstrapError ?? "No runtime profile was available."
            )
            VStack(alignment: .leading, spacing: NappletMetrics.snug) {
                ContentUnavailableView(
                    presentation.title,
                    systemImage: presentation.symbol,
                    description: Text(presentation.message)
                )
                NappletEvidence {
                    NappletFieldGrid(fields: presentation.evidenceFields)
                }
                .font(.caption)
            }
        }
    }

    /// Answers "is this exact build actually still running" from the
    /// Rust-owned session list, not from `runningArtifacts` alone.
    ///
    /// `runningArtifacts` is keyed by identity and removed only when the
    /// user closes the window (`ContentView+Canvas.swift`'s
    /// `removeValue(forKey:)`); it is never cleared when Rust itself ends
    /// the session (crash, revoke, stop). Before this fix the Inspector
    /// read that dictionary alone, so a crashed napplet's window kept
    /// reporting "Running" until the user closed it -- indistinguishable
    /// from a napplet that was actually still working.
    private func nappletStatus(for window: WorkbenchCanvasWindow) -> String {
        guard let exactBuild = window.exactBuild,
            runningArtifacts[exactBuild] != nil
        else {
            return "Not open"
        }
        let hasRunningSession = libraryManager.refresh().builds.contains { build in
            build.exactBuild.manifestAuthor == exactBuild.manifestAuthor
                && build.exactBuild.dTag == exactBuild.dTag
                && build.exactBuild.aggregateHash == exactBuild.aggregateHash
                && build.sessions.contains { $0.state == .running }
        }
        return hasRunningSession ? "Running" : "Session ended"
    }
}
