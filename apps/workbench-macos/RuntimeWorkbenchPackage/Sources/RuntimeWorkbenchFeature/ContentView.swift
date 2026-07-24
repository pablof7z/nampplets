import Foundation
import NMPNativeRuntimeApple
import SwiftUI

private enum InspectorTab: String, CaseIterable, Identifiable {
    case overview
    case relays

    var id: String { rawValue }

    var title: String {
        switch self {
        case .overview: "Overview"
        case .relays: "Relays"
        }
    }
}

private enum InstalledArtifactPresentation {
    case immediate
    case afterCatalogDismiss
    case restoration

    var focusesExistingWindow: Bool {
        self != .restoration
    }
}

public struct ContentView: View {
    private static let workspaceID = "default"

    private let profile: WorkbenchRuntimeProfile?
    private let bootstrapError: String?
    private let layoutStore: any WorkbenchLayoutPersisting
    private let accountManager: any WorkbenchAccountManaging
    private let catalogClient: any CatalogClient
    private let libraryManager: any WorkbenchLibraryManaging
    private let injectedPermissionManager: (any PermissionReviewManaging)?

    @State private var activity = "Opening application runtime profile"
    @State private var installedArtifacts:
        [WorkbenchExactBuildIdentity: NativeRuntimeInstalledArtifact] = [:]
    @State private var permissionTargetIdentity:
        WorkbenchExactBuildIdentity?
    @State private var deferredPermissionIdentity:
        WorkbenchExactBuildIdentity?
    @State private var deferredLibraryOpenIdentity:
        WorkbenchExactBuildIdentity?
    @State private var runningArtifacts:
        [WorkbenchExactBuildIdentity: NappletArtifact] = [:]
    @State private var reacquiringIdentities:
        Set<WorkbenchExactBuildIdentity> = []
    @State private var launchingIdentities:
        Set<WorkbenchExactBuildIdentity> = []
    @State private var layout: WorkbenchLayoutModel
    @State private var fullWindowRootID: WorkbenchWindowID?
    @State private var fullWindowPath: [WorkbenchWindowID] = []
    @State private var layoutPersistenceError: String?
    @State private var pendingLayoutSave: DispatchWorkItem?
    @State private var accountSnapshot: WorkbenchAccountSnapshot
    @State private var isInspectorPresented = false
    @State private var inspectorTab: InspectorTab = .overview
    @State private var isAccountSheetPresented = false
    @State private var isCatalogSheetPresented = false
    @State private var isLibrarySheetPresented = false
    @State private var isActivitySheetPresented = false
    @State private var isPermissionSheetPresented = false
    @State private var isSettingsSheetPresented = false
    @State private var activitySource: RuntimeWorkbenchActivitySource?
    @State private var activitySheetError: String?
    @State private var permissionManager: (any PermissionReviewManaging)?
    @State private var permissionSheetError: String?
    @State private var settingsSnapshot: WorkbenchSettingsSnapshot?
    @State private var settingsRoute = WorkbenchSettingsRouteState()
    @State private var nativeActionNotice: NativeActionNotice?
    @StateObject private var pendingWrites: RuntimeWorkbenchPendingWriteModel
    @StateObject private var receipts: RuntimeWorkbenchReceiptModel

    @MainActor
    public init(
        profile: WorkbenchRuntimeProfile? = nil,
        bootstrapError: String? = nil,
        layoutStore: (any WorkbenchLayoutPersisting)? = nil,
        accountManager: (any WorkbenchAccountManaging)? = nil,
        catalogClient: (any CatalogClient)? = nil,
        libraryManager: (any WorkbenchLibraryManaging)? = nil,
        permissionManager: (any PermissionReviewManaging)? = nil
    ) {
        self.profile = profile
        self.bootstrapError = bootstrapError
        let resolvedLayoutStore: any WorkbenchLayoutPersisting =
            layoutStore
            ?? profile.map(RuntimeWorkbenchLayoutStore.init(profile:))
            ?? VolatileWorkbenchLayoutStore()
        self.layoutStore = resolvedLayoutStore
        let resolvedAccountManager: any WorkbenchAccountManaging =
            accountManager
            ?? profile.map(RuntimeWorkbenchAccountManager.init(profile:))
            ?? UnavailableWorkbenchAccountManager()
        self.accountManager = resolvedAccountManager
        _accountSnapshot = State(
            initialValue: resolvedAccountManager.snapshot()
        )
        self.catalogClient =
            catalogClient
            ?? profile.map {
                RuntimeWorkbenchCatalogClient(profileBacking: $0)
            }
            ?? RuntimeWorkbenchCatalogClient()
        self.libraryManager =
            libraryManager
            ?? profile.map(RuntimeWorkbenchLibraryManager.init(profile:))
            ?? UnavailableWorkbenchLibraryManager(
                reason: bootstrapError
                    ?? "The application runtime profile is still opening."
            )
        injectedPermissionManager = permissionManager
        _pendingWrites = StateObject(
            wrappedValue: RuntimeWorkbenchPendingWriteModel(profile: profile)
        )
        _receipts = StateObject(
            wrappedValue: RuntimeWorkbenchReceiptModel(profile: profile)
        )

        do {
            let restored = try resolvedLayoutStore.loadLayout(
                workspaceID: Self.workspaceID
            )
            _layout = State(
                initialValue: WorkbenchLayoutModel(
                    snapshot: restored ?? .workbenchDefault
                )
            )
            _layoutPersistenceError = State(initialValue: nil)
        } catch {
            _layout = State(initialValue: WorkbenchLayoutModel())
            _layoutPersistenceError = State(
                initialValue: "Layout was not restored: \(error.localizedDescription)"
            )
        }
    }

    public var body: some View {
        platformBody
            .sheet(isPresented: $isAccountSheetPresented) {
            WorkbenchAccountSheet(manager: accountManager)
        }
        .sheet(isPresented: $isCatalogSheetPresented) {
            CatalogSheet(
                client: catalogClient,
                onInstalled: handleCatalogInstallation
            )
        }
        .sheet(isPresented: $isLibrarySheetPresented) {
            WorkbenchLibrarySheet(
                manager: libraryManager,
                onOpen: { build in
                    deferredLibraryOpenIdentity = WorkbenchExactBuildIdentity(
                        manifestAuthor: build.exactBuild.manifestAuthor,
                        dTag: build.exactBuild.dTag,
                        aggregateHash: build.exactBuild.aggregateHash
                    )
                    isLibrarySheetPresented = false
                }
            )
        }
        .sheet(isPresented: $isActivitySheetPresented) {
            if
                let activitySource,
                let scope = selectedActivityScope
            {
                ActivityDrawer(
                    source: activitySource,
                    scope: scope
                )
            } else {
                NavigationStack {
                    ContentUnavailableView(
                        "Activity unavailable",
                        systemImage: "waveform.path.ecg.rectangle",
                        description: Text(
                            activitySheetError
                                ?? "The exact-build activity source was not admitted."
                        )
                    )
                    .navigationTitle("Runtime Activity")
                    #if os(macOS)
                    .frame(minWidth: 620, minHeight: 420)
                    #endif
                }
            }
        }
        .sheet(isPresented: $isPermissionSheetPresented) {
            if let permissionManager {
                PermissionReviewSheet(manager: permissionManager)
            } else {
                NavigationStack {
                    ContentUnavailableView(
                        "Permission review unavailable",
                        systemImage: "lock.slash",
                        description: Text(
                            permissionSheetError
                                ?? "The exact-build permission review was not admitted."
                        )
                    )
                    .navigationTitle("Review Permissions")
                    #if os(macOS)
                    .frame(minWidth: 620, minHeight: 420)
                    #endif
                }
            }
        }
        .sheet(isPresented: $isSettingsSheetPresented) {
            if let settingsSnapshot {
                WorkbenchSettingsSheet(
                    snapshot: settingsSnapshot,
                    openDestination: scheduleSettingsDestination
                )
            } else {
                NavigationStack {
                    ContentUnavailableView(
                        "Settings unavailable",
                        systemImage: "gearshape.fill",
                        description: Text(
                            "The bounded runtime profile status could not be displayed."
                        )
                    )
                    .navigationTitle("Settings")
                    #if os(macOS)
                    .frame(minWidth: 620, minHeight: 420)
                    #endif
                }
            }
        }
        .task(id: profile.map(ObjectIdentifier.init)) {
            if let bootstrapError {
                activity = "Refused: \(bootstrapError)"
                return
            }
            guard let profile else {
                activity = "Opening application runtime profile"
                return
            }
            profile.native.setIncActionHandler { action in
                Task { @MainActor in
                    handleNativeAction(action)
                }
            }
            if await restorePersistedCanvasWindows() {
                return
            }
            guard
                ProcessInfo.processInfo.environment[
                    "NMP_WORKBENCH_UI_TEST_SCENARIO"
                ] == "good-morning-permission-launch"
            else {
                activity = "Canvas ready · add a napplet from the live catalog"
                return
            }
            do {
                let fixture = try GoodMorningFixture.load()
                let installed = try await Task.detached {
                    try fixture.install(profile: profile)
                }.value
                try prepareInstalledArtifact(
                    installed,
                    identity: WorkbenchExactBuildIdentity(
                        manifestAuthor: GoodMorningFixture.author,
                        dTag: GoodMorningFixture.dTag,
                        aggregateHash: GoodMorningFixture.aggregateHash
                    )
                )
            } catch {
                activity = "Refused: \(error.localizedDescription)"
            }
        }
        .onChange(of: isAccountSheetPresented) { _, isPresented in
            if !isPresented {
                accountSnapshot = accountManager.snapshot()
            }
        }
        .onChange(of: receipts.receiptIDs) { _, _ in
            scheduleLayoutSave()
        }
        .onChange(of: isCatalogSheetPresented) { _, isPresented in
            guard
                !isPresented,
                let identity = deferredPermissionIdentity
            else {
                return
            }
            deferredPermissionIdentity = nil
            permissionTargetIdentity = identity
            isPermissionSheetPresented = true
        }
        .onChange(of: isLibrarySheetPresented) { _, isPresented in
            guard
                !isPresented,
                let identity = deferredLibraryOpenIdentity
            else {
                return
            }
            deferredLibraryOpenIdentity = nil
            Task {
                await reacquireInstalledArtifact(
                    identity,
                    presentation: .immediate
                )
            }
        }
        .onChange(of: isSettingsSheetPresented) { _, isPresented in
            var route = settingsRoute
            guard
                let destination = route.consumeAfterDismiss(
                    settingsIsPresented: isPresented
                )
            else {
                return
            }
            settingsRoute = route
            DispatchQueue.main.async {
                openSettingsDestination(destination)
            }
        }
        .onChange(of: isPermissionSheetPresented) { _, isPresented in
            guard
                !isPresented,
                let identity = permissionTargetIdentity,
                permissionManager?.snapshot().submissionState == .applied
            else {
                return
            }
            launchInstalledIfPermitted(identity)
        }
        .onDisappear {
            pendingLayoutSave?.cancel()
            persistLayoutImmediately()
            profile?.native.setIncActionHandler(nil)
        }
        #if os(macOS)
        .frame(minWidth: 1_050, minHeight: 660)
        #endif
    }

    @ViewBuilder
    private var platformBody: some View {
        #if os(iOS)
        if layout.mode == .fullWindow {
            WorkbenchFullWindowView(
                layout: $layout,
                rootID: fullWindowRootID,
                path: $fullWindowPath,
                onExit: exitFullWindow,
                windowContent: windowContent,
                topBars: { topStatusBars }
            )
        } else {
            NavigationStack {
                canvasBody
                    .navigationTitle("Workbench")
                    .navigationBarTitleDisplayMode(.inline)
                    .toolbar {
                        ToolbarItem(placement: .topBarLeading) {
                            accountMenu
                        }
                        ToolbarItemGroup(placement: .topBarTrailing) {
                            Button {
                                isCatalogSheetPresented = true
                            } label: {
                                Label("Add Napplet", systemImage: "plus")
                            }
                            .accessibilityIdentifier("add-napplet")
                            .accessibilityHint("Opens the network napplet catalog")

                            workspaceActionsMenu

                            Button {
                                withAnimation(.easeInOut(duration: 0.18)) {
                                    isInspectorPresented.toggle()
                                }
                            } label: {
                                Label(
                                    isInspectorPresented ? "Hide Inspector" : "Show Inspector",
                                    systemImage: "sidebar.right"
                                )
                            }
                            .accessibilityIdentifier("toggle-napplet-inspector")

                            layoutMenu
                        }
                    }
            }
        }
        #else
        VStack(spacing: 0) {
            workspaceControlStrip
            canvasBody
        }
        #endif
    }

    @ViewBuilder
    private var topStatusBars: some View {
        if let pendingWrite = pendingWrites.writes.first {
            PendingWriteApprovalBar(write: pendingWrite) { approve in
                pendingWrites.decide(
                    pendingWrite,
                    approve: approve,
                    profile: profile
                )
            }
        }
        if let receipt = receipts.receipts.last {
            ReceiptStatusBar(receipt: receipt)
        }
    }

    private var canvasBody: some View {
        VStack(spacing: 0) {
            topStatusBars
            HStack(spacing: 0) {
                WorkbenchWorkspaceView(
                    layout: $layout,
                    onLayoutChange: scheduleLayoutSave,
                    onClose: closeWindow,
                    windowContent: windowContent
                )
                if isInspectorPresented {
                    Divider()
                    nappletInspector
                        .transition(.move(edge: .trailing).combined(with: .opacity))
                }
            }
            #if os(macOS)
            activityBar
            #endif
        }
        .background(.background)
    }

    private var workspaceControlStrip: some View {
        HStack(spacing: 10) {
            accountMenu

            Text("Workbench")
                .font(.title3.weight(.semibold))
            Spacer()

            Button {
                isCatalogSheetPresented = true
            } label: {
                Label("Add Napplet", systemImage: "plus")
            }
            .buttonStyle(.borderedProminent)
            .keyboardShortcut("n", modifiers: [.command])
            .accessibilityIdentifier("add-napplet")
            .accessibilityHint("Opens the network napplet catalog")

            workspaceActionsMenu

            Button {
                withAnimation(.easeInOut(duration: 0.18)) {
                    isInspectorPresented.toggle()
                }
            } label: {
                Label(
                    isInspectorPresented ? "Hide Inspector" : "Show Inspector",
                    systemImage: "sidebar.right"
                )
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            .keyboardShortcut("i", modifiers: [.command, .option])
            .accessibilityIdentifier("toggle-napplet-inspector")

            layoutMenu
        }
        .padding(.horizontal, 14)
        .frame(height: 50)
        .background(.bar)
    }

    private var accountMenu: some View {
        Menu {
            Section("Switch account") {
                if accountSnapshot.accounts.isEmpty {
                    Text("No stored accounts")
                } else {
                    ForEach(accountSnapshot.accounts) { account in
                        Button {
                            Task {
                                await accountManager.activate(
                                    handle: account.handle
                                )
                                accountSnapshot = accountManager.snapshot()
                            }
                        } label: {
                            if accountSnapshot.activeHandle == account.handle {
                                Label(
                                    "\(accountDisplayName(account)) · "
                                        + account.connectionKind.title,
                                    systemImage: "checkmark"
                                )
                            } else {
                                Label(
                                    "\(accountDisplayName(account)) · "
                                        + account.connectionKind.title,
                                    systemImage: accountMenuSymbol(account)
                                )
                            }
                        }
                    }
                }

                if accountSnapshot.activeAccount != nil {
                    Button("Sign Out", systemImage: "rectangle.portrait.and.arrow.right") {
                        Task {
                            await accountManager.logout()
                            accountSnapshot = accountManager.snapshot()
                        }
                    }
                }
            }

            Section("Add account") {
                Button("Signer-backed Account…", systemImage: "key") {
                    isAccountSheetPresented = true
                }
                Button(
                    "Read-only Identity…",
                    systemImage: "person.text.rectangle"
                ) {
                    isAccountSheetPresented = true
                }
            }

            Button("Manage Accounts…", systemImage: "person.2") {
                isAccountSheetPresented = true
            }
        } label: {
            Label(
                activeAccountLabel,
                systemImage: accountSnapshot.activeAccount == nil
                    ? "person.crop.circle.badge.xmark"
                    : "person.crop.circle.badge.checkmark"
            )
        }
        .menuStyle(.borderlessButton)
        .accessibilityLabel("Account switcher")
        .accessibilityValue(activeAccountLabel)
        .accessibilityIdentifier("account-switcher")
    }

    private var workspaceActionsMenu: some View {
        Menu {
            Button("Installed Napplets", systemImage: "square.stack.3d.up") {
                isLibrarySheetPresented = true
            }
            .keyboardShortcut("l", modifiers: [.command, .shift])

            Button("Activity", systemImage: "waveform.path.ecg") {
                openActivityDrawer()
            }
            .keyboardShortcut("a", modifiers: [.command, .shift])

            Button("Permissions", systemImage: "lock.shield") {
                openPermissionReview()
            }
            .keyboardShortcut("p", modifiers: [.command, .shift])

            Divider()

            Button("Settings", systemImage: "gearshape") {
                openSettings()
            }
            .keyboardShortcut(",", modifiers: [.command])
        } label: {
            Label("Workspace Actions", systemImage: "ellipsis.circle")
        }
        .labelStyle(.iconOnly)
        .menuStyle(.borderlessButton)
        .accessibilityHint(
            "Opens installed napplets, activity, permissions, or settings"
        )
    }

    private var availableLayoutModes: [WorkbenchLayoutMode] {
        #if os(iOS)
        WorkbenchLayoutMode.allCases
        #else
        WorkbenchLayoutMode.allCases.filter { $0 != .fullWindow }
        #endif
    }

    private var layoutMenu: some View {
        Menu {
            Section("Window layout") {
                ForEach(availableLayoutModes, id: \.self) { mode in
                    Button {
                        setLayoutMode(mode)
                    } label: {
                        if layout.mode == mode {
                            Label(mode.title, systemImage: "checkmark")
                        } else {
                            Label(mode.title, systemImage: mode.systemImage)
                        }
                    }
                }
            }
        } label: {
            Label(layout.mode.title, systemImage: layout.mode.systemImage)
        }
        .accessibilityHint(
            "Switches between freely arranged, automatically tiled, and full "
                + "window napplet display"
        )
        .accessibilityIdentifier("layout-mode-menu")
    }

    private var nappletInspector: some View {
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

    private var activityBar: some View {
        HStack(spacing: 8) {
            Image(systemName: activitySymbol)
                .foregroundStyle(activityColor)
            Text(activity)
            if let layoutPersistenceError {
                Divider()
                    .frame(height: 16)
                Label(layoutPersistenceError, systemImage: "externaldrive.badge.exclamationmark")
                    .foregroundStyle(.orange)
            }
            Spacer()
            Text("Direct napplet network denied · ephemeral WebKit store")
                .foregroundStyle(.secondary)
        }
        .font(.caption)
        .padding(.horizontal, 16)
        .frame(height: 34)
        .background(.bar)
        .accessibilityIdentifier("runtime-activity")
    }

    @ViewBuilder
    private func windowContent(_ window: WorkbenchCanvasWindow) -> some View {
        if
            let identity = window.exactBuild,
            let artifact = runningArtifacts[identity]
        {
            nappletSurface(artifact, title: window.title)
        } else if
            let identity = window.exactBuild,
            launchingIdentities.contains(identity)
                || reacquiringIdentities.contains(identity)
        {
            VStack(spacing: 12) {
                ProgressView()
                Text("Launching verified napplet")
                    .font(.headline)
                Text("The exact build is opening inside its isolated runtime.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .accessibilityIdentifier("napplet-launching")
        } else if
            let identity = window.exactBuild,
            installedArtifacts[identity] != nil
        {
            ContentUnavailableView {
                Label("Permission review required", systemImage: "lock.shield")
            } description: {
                Text(
                    "This exact verified build is installed. Review its required "
                        + "capabilities before it runs."
                )
            } actions: {
                Button("Review Permissions") {
                    permissionTargetIdentity = identity
                    openPermissionReview()
                }
                .accessibilityIdentifier("review-installed-permissions")
            }
            .accessibilityIdentifier("installed-napplet-awaiting-permission")
        } else if let identity = window.exactBuild {
            ContentUnavailableView {
                Label("Napplet is not running", systemImage: "app.badge")
            } description: {
                Text(
                    "Reopen this installed exact build without changing its grants."
                )
            } actions: {
                Button("Open Installed Napplet") {
                    Task {
                        await reacquireInstalledArtifact(
                            identity,
                            presentation: .immediate
                        )
                    }
                }
                .accessibilityIdentifier("reopen-installed-napplet")
            }
        } else {
            ContentUnavailableView(
                "Napplet is not running",
                systemImage: "app.badge",
                description: Text("This canvas window has no exact installed build.")
            )
        }
    }

    @ViewBuilder
    private func nappletSurface(
        _ artifact: NappletArtifact,
        title: String
    ) -> some View {
        TrustedNappletView(artifact: artifact) { event in
            switch event {
            case .loading:
                activity = "Loading trusted shell"
            case .mounted:
                activity = "Signed \(title) napplet mounted"
            case .request(let type):
                activity = "Mapped \(type) from napplet window"
            case .refused(let reason):
                activity = "Refused: \(reason)"
            case .crashed:
                activity = "\(title) WebView crashed"
            }
        }
        .accessibilityIdentifier("bundled-napplet")
    }

    private var activitySymbol: String {
        activity.hasPrefix("Refused") || activity.contains("crashed")
            ? "exclamationmark.triangle.fill"
            : "checkmark.shield.fill"
    }

    private var activityColor: Color {
        activity.hasPrefix("Refused") || activity.contains("crashed")
            ? .orange
            : .green
    }

    private var selectedActivityScope: ActivityExactBuildScope? {
        guard let identity = layout.selectedWindow?.exactBuild else {
            return nil
        }
        return ActivityExactBuildScope(
            manifestAuthor: identity.manifestAuthor,
            dTag: identity.dTag,
            aggregateHash: identity.aggregateHash
        )
    }

    @MainActor
    private func openActivityDrawer() {
        activitySheetError = nil
        guard let profile else {
            activitySheetError =
                bootstrapError ?? "The application runtime profile is unavailable."
            isActivitySheetPresented = true
            return
        }
        guard let scope = selectedActivityScope else {
            activitySheetError =
                "Select an exact-build napplet window to view its activity."
            isActivitySheetPresented = true
            return
        }
        activitySource = nil
        do {
            activitySource = try RuntimeWorkbenchActivitySource(
                profile: profile,
                scope: scope
            )
        } catch {
            activitySheetError = error.localizedDescription
        }
        isActivitySheetPresented = true
    }

    @MainActor
    private func handleCatalogInstallation(
        _ build: CatalogInstalledBuild
    ) {
        guard let profile else {
            activity = "Refused: the application runtime profile is unavailable"
            return
        }
        let identity = WorkbenchExactBuildIdentity(
            manifestAuthor: build.manifestAuthor,
            dTag: build.dTag,
            aggregateHash: build.exactAggregateHash
        )
        guard
            let installed = profile.installedCatalogArtifact(for: identity)
        else {
            activity =
                "Refused: the verified artifact handle is unavailable for this profile"
            return
        }
        do {
            try prepareInstalledArtifact(
                installed,
                identity: identity,
                presentation: .afterCatalogDismiss
            )
        } catch {
            activity = "Refused: \(error.localizedDescription)"
        }
    }

    @MainActor
    private func prepareInstalledArtifact(
        _ installed: NativeRuntimeInstalledArtifact,
        identity: WorkbenchExactBuildIdentity,
        presentation: InstalledArtifactPresentation = .immediate
    ) throws {
        guard let profile else {
            throw RuntimeWorkbenchPermissionError.malformed(
                "the application runtime profile is unavailable"
            )
        }
        let previouslyDisplayed = fullWindowPath.last ?? fullWindowRootID
        let targetWindowID: WorkbenchWindowID
        if let existing = layout.windows.first(where: {
            $0.exactBuild == identity
        }) {
            if presentation.focusesExistingWindow {
                mutateLayout {
                    $0.bringToFront(existing.id)
                }
            }
            targetWindowID = existing.id
        } else {
            var next = layout
            let window = WorkbenchCanvasWindow.installed(
                title: installed.title,
                identity: identity,
                offset: Double(next.windows.count % 8) * 24
            )
            guard next.addWindow(window) else {
                throw RuntimeWorkbenchPermissionError.refused(
                    code: "workspace-capacity",
                    detail: "Close a canvas window before adding another napplet."
                )
            }
            layout = next
            scheduleLayoutSave()
            targetWindowID = window.id
        }
        pushFullWindowIfNeeded(
            target: targetWindowID,
            previouslyDisplayed: previouslyDisplayed
        )
        installedArtifacts[identity] = installed
        permissionTargetIdentity = identity
        let principal = try permissionPrincipal(identity)
        permissionManager = try RuntimeWorkbenchPermissionManager(
            profile: profile,
            principal: principal
        )
        let nativeReview = profile.native.permissionReview(
            for: installed.permissionCoordinate
        )
        guard nativeReview.refusal == nil,
              let review = nativeReview.review
        else {
            throw RuntimeWorkbenchPermissionError.refused(
                code: nativeReview.refusal?.code ?? "missing-review",
                detail: nativeReview.refusal?.detail
                    ?? "Rust returned no permission review"
            )
        }
        if review.launchPermitted {
            launchInstalledIfPermitted(identity)
        } else {
            activity = "Permission review required before launch"
            switch presentation {
            case .immediate:
                isPermissionSheetPresented = true
            case .afterCatalogDismiss:
                deferredPermissionIdentity = identity
            case .restoration:
                break
            }
        }
    }

    @MainActor
    @discardableResult
    private func reacquireInstalledArtifact(
        _ identity: WorkbenchExactBuildIdentity,
        presentation: InstalledArtifactPresentation
    ) async -> Bool {
        guard let profile else {
            activity = "Refused: the application runtime profile is unavailable"
            return false
        }
        guard
            !reacquiringIdentities.contains(identity),
            !launchingIdentities.contains(identity)
        else {
            return false
        }
        reacquiringIdentities.insert(identity)
        activity = "Reopening installed exact build"
        let result = await Task.detached {
            profile.reacquirePersistedCanvasArtifact(for: identity)
        }.value
        reacquiringIdentities.remove(identity)
        switch result {
        case let .refused(failure):
            activity = "Refused: \(failure.code): \(failure.detail)"
            return false
        case let .installed(installation):
            guard
                presentation != .restoration
                    || layout.windows.contains(where: {
                        $0.exactBuild == identity
                    })
            else {
                return false
            }
            do {
                try prepareInstalledArtifact(
                    installation.installedArtifact,
                    identity: identity,
                    presentation: presentation
                )
                return true
            } catch {
                activity = "Refused: \(error.localizedDescription)"
                return false
            }
        }
    }

    @MainActor
    private func restorePersistedCanvasWindows() async -> Bool {
        let plan = WorkbenchRestoredCanvasLaunchPlan(layout: layout)
        guard !plan.identities.isEmpty else {
            return false
        }
        activity =
            "Reopening \(plan.identities.count) persisted napplet"
            + (plan.identities.count == 1 ? "" : "s")
        for identity in plan.identities {
            _ = await reacquireInstalledArtifact(
                identity,
                presentation: .restoration
            )
        }
        return true
    }

    @MainActor
    private func launchInstalledIfPermitted(
        _ identity: WorkbenchExactBuildIdentity
    ) {
        guard
            let profile,
            let installed =
                installedArtifacts[identity]
                ?? profile.installedCatalogArtifact(for: identity),
            !launchingIdentities.contains(identity),
            runningArtifacts[identity] == nil
        else {
            return
        }
        let review = profile.native.permissionReview(
            for: installed.permissionCoordinate
        )
        guard review.refusal == nil,
              review.review?.launchPermitted == true
        else {
            activity = "Permission review still requires a decision"
            return
        }
        launchingIdentities.insert(identity)
        activity = "Launching signed exact build"
        Task {
            defer { launchingIdentities.remove(identity) }
            do {
                let launched = try await Task.detached {
                    try profile.native.launchInstalled(installed)
                }.value
                guard layout.windows.contains(where: {
                    $0.exactBuild == identity
                }) else {
                    return
                }
                runningArtifacts[identity] = launched
                if permissionTargetIdentity == identity {
                    permissionTargetIdentity = nil
                }
                activity = "Signed exact-build session ready"
            } catch {
                activity = "Refused: \(error.localizedDescription)"
            }
        }
    }

    private func permissionPrincipal(
        _ identity: WorkbenchExactBuildIdentity
    )
        throws -> PermissionExactBuildPrincipal
    {
        guard
            let principal = PermissionExactBuildPrincipal(
                manifestAuthorPublicKey: identity.manifestAuthor,
                dTag: identity.dTag,
                aggregateHash: identity.aggregateHash
            )
        else {
            throw RuntimeWorkbenchPermissionError.malformed(
                "the selected exact-build identity is invalid"
            )
        }
        return principal
    }

    @MainActor
    private func openPermissionReview() {
        permissionSheetError = nil
        permissionManager = nil
        if let injectedPermissionManager {
            permissionManager = injectedPermissionManager
            isPermissionSheetPresented = true
            return
        }
        guard let profile else {
            permissionSheetError =
                bootstrapError ?? "The application runtime profile is unavailable."
            isPermissionSheetPresented = true
            return
        }
        guard
            let identity =
                permissionTargetIdentity ?? layout.selectedWindow?.exactBuild
        else {
            permissionSheetError =
                "Select an exact-build napplet window to review permissions."
            isPermissionSheetPresented = true
            return
        }
        do {
            guard
                installedArtifacts[identity] != nil
                    || profile.installedCatalogArtifact(for: identity) != nil
            else {
                throw RuntimeWorkbenchPermissionError.refused(
                    code: "artifact-handle-unavailable",
                    detail: "Reinstall this exact build to reopen its verified artifact."
                )
            }
            let principal = try permissionPrincipal(identity)
            permissionManager = try RuntimeWorkbenchPermissionManager(
                profile: profile,
                principal: principal
            )
            permissionTargetIdentity = identity
        } catch {
            permissionSheetError = error.localizedDescription
        }
        isPermissionSheetPresented = true
    }

    @MainActor
    private func openSettings() {
        let unavailableReason =
            bootstrapError ?? "The application runtime profile is still opening."
        settingsSnapshot = WorkbenchSettingsSnapshot(
            profileAvailable: profile != nil,
            unavailableReason: profile == nil ? unavailableReason : nil
        )
        settingsRoute = WorkbenchSettingsRouteState()
        isSettingsSheetPresented = true
    }

    @MainActor
    private func scheduleSettingsDestination(
        _ destination: WorkbenchSettingsDestination
    ) {
        settingsRoute.schedule(destination)
    }

    @MainActor
    private func openSettingsDestination(
        _ destination: WorkbenchSettingsDestination
    ) {
        switch destination {
        case .account:
            isAccountSheetPresented = true
        case .installedLibrary:
            isLibrarySheetPresented = true
        case .activity:
            openActivityDrawer()
        }
    }

    private var activeAccountLabel: String {
        guard
            let activeHandle = accountSnapshot.activeHandle,
            let account = accountSnapshot.accounts.first(where: {
                $0.handle == activeHandle
            })
        else {
            return "Signed Out"
        }
        return accountDisplayName(account)
    }

    private func accountDisplayName(
        _ account: WorkbenchStoredAccount
    ) -> String {
        let projectedIdentity = account.npub.isEmpty
            ? account.publicKeyHex
            : account.npub
        guard projectedIdentity.count > 16 else {
            return projectedIdentity.isEmpty
                ? account.connectionKind.title
                : projectedIdentity
        }
        return "\(projectedIdentity.prefix(8))…\(projectedIdentity.suffix(6))"
    }

    private func accountMenuSymbol(
        _ account: WorkbenchStoredAccount
    ) -> String {
        switch account.connectionKind {
        case .localSigner:
            "key"
        case .remoteSigner:
            "network"
        case .readOnly:
            "eye"
        }
    }

    private func mutateLayout(
        _ mutation: (inout WorkbenchLayoutModel) -> Void
    ) {
        var next = layout
        mutation(&next)
        guard next != layout else {
            return
        }
        layout = next
        scheduleLayoutSave()
    }

    @MainActor
    private func closeWindow(_ window: WorkbenchCanvasWindow) {
        mutateLayout {
            $0.removeWindow(id: window.id)
        }
        if let identity = window.exactBuild {
            runningArtifacts.removeValue(forKey: identity)
            installedArtifacts.removeValue(forKey: identity)
            reacquiringIdentities.remove(identity)
            launchingIdentities.remove(identity)
            if permissionTargetIdentity == identity {
                permissionTargetIdentity = nil
            }
            if deferredPermissionIdentity == identity {
                deferredPermissionIdentity = nil
            }
        }
        activity = "Closed \(window.title) without uninstalling it"
    }

    private func scheduleLayoutSave() {
        guard pendingLayoutSave == nil else {
            return
        }
        let pending = DispatchWorkItem {
            guard !(pendingLayoutSave?.isCancelled ?? true) else {
                pendingLayoutSave = nil
                return
            }
            pendingLayoutSave = nil
            do {
                try layoutStore.saveLayout(
                    layout.snapshot,
                    workspaceID: Self.workspaceID,
                    retainedReceiptIDs: receipts.receiptIDs
                )
                layoutPersistenceError = nil
            } catch {
                layoutPersistenceError =
                    "Layout was not saved: \(error.localizedDescription)"
            }
        }
        pendingLayoutSave = pending
        DispatchQueue.main.async(execute: pending)
    }

    private func persistLayoutImmediately() {
        do {
            try layoutStore.saveLayout(
                layout.snapshot,
                workspaceID: Self.workspaceID,
                retainedReceiptIDs: receipts.receiptIDs
            )
            layoutPersistenceError = nil
        } catch {
            layoutPersistenceError =
                "Layout was not saved: \(error.localizedDescription)"
        }
    }

    @MainActor
    private func handleNativeAction(_ action: NativeWorkbenchAction) {
        let identity = WorkbenchExactBuildIdentity(
            manifestAuthor: action.manifestAuthor,
            dTag: action.dTag,
            aggregateHash: action.aggregateHash
        )
        guard let window = layout.windows.first(where: {
            $0.exactBuild == identity
        }) else {
            activity = "Refused: INC action came from an unopened exact build"
            return
        }
        guard let notice = NativeActionNotice.decode(action) else {
            activity = "Refused: INC action payload was not recognized"
            return
        }
        let previouslyDisplayed = fullWindowPath.last ?? fullWindowRootID
        nativeActionNotice = notice
        mutateLayout { $0.bringToFront(window.id) }
        pushFullWindowIfNeeded(
            target: window.id,
            previouslyDisplayed: previouslyDisplayed
        )
        isInspectorPresented = true
        activity = "\(notice.title) from \(window.title)"
    }

    /// Pushes onto the full-window navigation stack when a different napplet
    /// becomes active while `.fullWindow` is engaged, so opening one napplet
    /// from another reads as a normal iOS push rather than an in-place swap.
    /// The very first napplet displayed becomes the stack root instead.
    @MainActor
    private func pushFullWindowIfNeeded(
        target: WorkbenchWindowID,
        previouslyDisplayed: WorkbenchWindowID?
    ) {
        guard layout.mode == .fullWindow else {
            return
        }
        guard let previouslyDisplayed else {
            fullWindowRootID = target
            return
        }
        guard previouslyDisplayed != target else {
            return
        }
        fullWindowPath.append(target)
    }

    @MainActor
    private func setLayoutMode(_ mode: WorkbenchLayoutMode) {
        mutateLayout { $0.setMode(mode) }
        if mode == .fullWindow {
            fullWindowRootID = layout.selectedWindow?.id
            fullWindowPath = []
        } else {
            fullWindowRootID = nil
            fullWindowPath = []
        }
    }

    @MainActor
    private func exitFullWindow() {
        setLayoutMode(.freeform)
    }
}
