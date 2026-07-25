import Foundation
import NMPNativeRuntimeApple
import SwiftUI

enum InspectorTab: String, CaseIterable, Identifiable {
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

enum InstalledArtifactPresentation {
    case immediate
    case afterCatalogDismiss
    case restoration

    var focusesExistingWindow: Bool {
        self != .restoration
    }
}

public struct ContentView: View {
    static let workspaceID = "default"

    let profile: WorkbenchRuntimeProfile?
    let bootstrapError: String?
    let layoutStore: any WorkbenchLayoutPersisting
    let accountManager: any WorkbenchAccountManaging
    private let catalogClient: any CatalogClient
    private let libraryManager: any WorkbenchLibraryManaging
    let injectedPermissionManager: (any PermissionReviewManaging)?

    @State var activity = "Opening application runtime profile"
    @State var installedArtifacts:
        [WorkbenchExactBuildIdentity: NativeRuntimeInstalledArtifact] = [:]
    @State var permissionTargetIdentity:
        WorkbenchExactBuildIdentity?
    @State var deferredPermissionIdentity:
        WorkbenchExactBuildIdentity?
    @State private var deferredLibraryOpenIdentity:
        WorkbenchExactBuildIdentity?
    @State var runningArtifacts:
        [WorkbenchExactBuildIdentity: NappletArtifact] = [:]
    @State var reacquiringIdentities:
        Set<WorkbenchExactBuildIdentity> = []
    @State var launchingIdentities:
        Set<WorkbenchExactBuildIdentity> = []
    @State var layout: WorkbenchLayoutModel
    @State var fullWindowRootID: WorkbenchWindowID?
    @State var fullWindowPath: [WorkbenchWindowID] = []
    @State var layoutPersistenceError: String?
    @State var pendingLayoutSave: DispatchWorkItem?
    @State var accountSnapshot: WorkbenchAccountSnapshot
    @State var isInspectorPresented = false
    @State var inspectorTab: InspectorTab = .overview
    @State var isAccountSheetPresented = false
    @State var isCatalogSheetPresented = false
    @State var isLibrarySheetPresented = false
    @State var isActivitySheetPresented = false
    @State var isPermissionSheetPresented = false
    @State var isSettingsSheetPresented = false
    @State var activitySource: RuntimeWorkbenchActivitySource?
    @State var activitySheetError: String?
    @State var permissionManager: (any PermissionReviewManaging)?
    @State var permissionSheetError: String?
    @State var settingsSnapshot: WorkbenchSettingsSnapshot?
    @State var settingsRoute = WorkbenchSettingsRouteState()
    @State var nativeActionNotice: NativeActionNotice?
    @StateObject var pendingWrites: RuntimeWorkbenchPendingWriteModel
    @StateObject var receipts: RuntimeWorkbenchReceiptModel

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
            ActivitySheetHost(
                source: activitySource,
                scope: selectedActivityScope,
                error: activitySheetError
            )
        }
        .sheet(isPresented: $isPermissionSheetPresented) {
            PermissionSheetHost(
                manager: permissionManager,
                error: permissionSheetError
            )
        }
        .sheet(isPresented: $isSettingsSheetPresented) {
            SettingsSheetHost(
                snapshot: settingsSnapshot,
                openDestination: scheduleSettingsDestination
            )
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

    var selectedActivityScope: ActivityExactBuildScope? {
        guard let identity = layout.selectedWindow?.exactBuild else {
            return nil
        }
        return ActivityExactBuildScope(
            manifestAuthor: identity.manifestAuthor,
            dTag: identity.dTag,
            aggregateHash: identity.aggregateHash
        )
    }
}
