import Foundation
import NMPNativeRuntimeApple
import SwiftUI

enum InspectorTab: String, CaseIterable, Identifiable {
    case overview
    case relays
    case console

    var id: String { rawValue }

    var title: String {
        switch self {
        case .overview: "Napplet"
        case .relays: "Network"
        case .console: "Console"
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
    let libraryManager: any WorkbenchLibraryManaging
    let injectedPermissionManager: (any PermissionReviewManaging)?
    let profileAction: WorkbenchProfileActionHandler

    @State var activity: WorkbenchActivityStatus = .preparing
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
    /// Exact builds the Rust-owned installed-library projection currently
    /// reports at least one `.running` session for. `runningArtifacts`
    /// tracks *window* bookkeeping (removed only when the operator closes
    /// the window) and must not be read as a live session-state claim; this
    /// set is what the Inspector's "Status" row actually reflects.
    @State var runningLibrarySessionBuilds: Set<WorkbenchLibraryExactBuild> = []
    @State var librarySessionSubscription: (any WorkbenchLibrarySubscription)?
    @State var reacquiringIdentities:
        Set<WorkbenchExactBuildIdentity> = []
    @State var launchingIdentities:
        Set<WorkbenchExactBuildIdentity> = []
    @State var consoleLog = NappletConsoleLog()
    @State var layout: WorkbenchLayoutModel
    @State var fullWindowRootID: WorkbenchWindowID?
    @State var fullWindowPath: [WorkbenchWindowID] = []
    @State var layoutPersistenceError: String?
    @State var pendingLayoutSave: DispatchWorkItem?
    @State var accountSnapshot: WorkbenchAccountSnapshot
    @State var isInspectorPresented = false
    @State var inspectorTab: InspectorTab = .overview
    @State var accountSheetRoute: WorkbenchAccountSheetRoute?
    var isAccountSheetPresented: Bool {
        get { accountSheetRoute != nil }
        nonmutating set {
            accountSheetRoute = newValue ? .manage : nil
        }
    }
    @State var isCatalogSheetPresented = false
    @State var isLibrarySheetPresented = false
    @State var isPermissionSheetPresented = false
    @State var isSettingsSheetPresented = false
    @State var activitySheetPresentation: ActivitySheetPresentation?
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
        permissionManager: (any PermissionReviewManaging)? = nil,
        profileAction: @escaping WorkbenchProfileActionHandler = { _ in
            throw WorkbenchPreferencesError.unavailable(
                "Preferences are unavailable while the app is opening."
            )
        }
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
                    ?? "Your napplets are still getting ready."
            )
        injectedPermissionManager = permissionManager
        self.profileAction = profileAction
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
            .sheet(item: $accountSheetRoute) { route in
            WorkbenchAccountSheet(
                manager: accountManager,
                route: route
            )
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
        .sheet(item: $activitySheetPresentation) { presentation in
            ActivitySheetHost(presentation: presentation)
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
                openDestination: scheduleSettingsDestination,
                performAction: profileAction
            )
        }
        .task(id: profile.map(ObjectIdentifier.init)) {
            await bootstrapProfile()
        }
        .onChange(of: accountSheetRoute) { _, route in
            if route == nil {
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
            profile?.native.setIntentActivationHandler(nil)
            librarySessionSubscription?.cancel()
            librarySessionSubscription = nil
        }
        #if os(macOS)
        // This is the root cause of the Dock overlap, and it is a hard floor,
        // not a preference: SwiftUI promotes a root view's `minWidth`/
        // `minHeight` to the *window's* minimum size. 1050 is wider than a
        // 1024pt display and 660 + 52pt of chrome is 712 -- precisely the
        // unshrinkable 1050x712 window measured in CI. No amount of
        // `setFrame` can beat a minimum size, which is why the first fix
        // changed nothing.
        //
        // `WorkbenchContentSizing` keeps the same intent as an ideal size and
        // lowers the floor to something a small display can actually satisfy.
        .frame(
            minWidth: WorkbenchContentSizing.minimumWidth,
            idealWidth: WorkbenchContentSizing.idealWidth,
            minHeight: WorkbenchContentSizing.minimumHeight,
            idealHeight: WorkbenchContentSizing.idealHeight
        )
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
