import AppKit
import SwiftUI
import RuntimeWorkbenchFeature

final class RuntimeWorkbenchAppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_: Notification) {
        activateWorkbenchWindow()
        DispatchQueue.main.async { [weak self] in
            self?.activateWorkbenchWindow()
        }
        // Deliberately no `didResize` observer. An earlier revision re-fitted
        // the window on every resize, but `setFrame` itself raises
        // `didResize`, so it could ping-pong with SwiftUI's own sizing and
        // spin the main thread. That is what hung the first UI test at launch
        // for the full 60s allowance while the app never became responsive.
        //
        // It is also unnecessary. The real defect was a minimum window size
        // (1050x660) that a 1024pt display could not satisfy; with the floor
        // lowered, AppKit's own `constrainFrameRect(_:to:)` keeps a new window
        // inside the visible frame without any help from us.
    }

    private func activateWorkbenchWindow() {
        NSApplication.shared.activate(ignoringOtherApps: true)
        guard let window = NSApplication.shared.windows.first else {
            return
        }
        window.makeKeyAndOrderFront(nil)
        fitToVisibleScreen(window)
    }

    /// Pulls the window out from under the Dock on displays too short for its
    /// ideal size. See `WorkbenchWindowFitting` for why this is a correctness
    /// fix and not a cosmetic one.
    private func fitToVisibleScreen(_ window: NSWindow) {
        // Sheets are windows too and will raise these notifications. Their
        // height is bounded by `PermissionReviewSheetGeometry`; moving one by
        // hand only fights AppKit's own sheet placement.
        guard !window.isSheet else { return }
        guard let visibleFrame = (window.screen ?? NSScreen.main)?.visibleFrame
        else {
            return
        }
        let current = window.frame
        let fitted = WorkbenchWindowFitting.fitted(current, into: visibleFrame)
        // Observability, so the next CI run distinguishes "never ran" from "ran
        // and was overridden" instead of leaving it to be inferred from an
        // unchanged button frame. The previous attempt could not tell the two
        // apart and cost a full cycle to learn nothing.
        func describe(_ rect: NSRect) -> String {
            "{{\(rect.minX), \(rect.minY)}, {\(rect.width), \(rect.height)}}"
        }
        NSLog(
            "workbench-window-fit: visible=%@ current=%@ fitted=%@ applied=%@",
            describe(visibleFrame),
            describe(current),
            describe(fitted),
            WorkbenchWindowFitting.fits(current, in: visibleFrame)
                ? "no-already-fits" : "yes"
        )
        // Terminates: once the window fits, this returns before setting a frame,
        // so the resize notification it raises does not recur.
        guard !WorkbenchWindowFitting.fits(current, in: visibleFrame) else {
            return
        }
        window.setFrame(fitted, display: true)
    }
}

@main
struct RuntimeWorkbenchApp: App {
    @NSApplicationDelegateAdaptor(RuntimeWorkbenchAppDelegate.self)
    private var appDelegate
    @State private var runtimeProfile: WorkbenchRuntimeProfile?
    @State private var runtimeError: String?
    @State private var isOpeningRuntime = false
    @AppStorage(WorkbenchAppearance.storageKey)
    private var appearance = WorkbenchAppearance.system

    var body: some Scene {
        WindowGroup {
            Group {
                if let runtimeProfile {
                    ContentView(
                        profile: runtimeProfile,
                        profileAction: { action in
                            try await performProfileAction(action)
                        }
                    )
                        .id(ObjectIdentifier(runtimeProfile))
                } else if let runtimeError {
                    ContentView(bootstrapError: runtimeError)
                } else {
                    // These were `minWidth`/`minHeight`, which SwiftUI turns
                    // into the *window's* minimum size. 1050pt is wider than a
                    // 1024pt display, so the window could not be shrunk to fit
                    // and AppKit produced exactly the 1050x712 seen in CI
                    // (660 content + 52 chrome). A loading placeholder must
                    // express a preference, not a floor the window cannot honour.
                    ProgressView("Opening runtime…")
                        .frame(idealWidth: 1_050, idealHeight: 660)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .preferredColorScheme(appearance.colorScheme)
            .onAppear {
                NSApplication.shared.activate(ignoringOtherApps: true)
            }
            .task {
                await openRuntimeIfNeeded()
            }
        }
        .defaultSize(width: 1180, height: 780)
        .windowToolbarStyle(.unified(showsTitle: false))

        Settings {
            WorkbenchSettingsView(
                snapshot: currentSettingsSnapshot,
                performAction: { action in
                    try await performProfileAction(action)
                }
            )
            .id(runtimeProfile.map(ObjectIdentifier.init))
            .preferredColorScheme(appearance.colorScheme)
        }
        .windowToolbarStyle(.unified(showsTitle: false))
    }

    private var currentSettingsSnapshot: WorkbenchSettingsSnapshot? {
        if let runtimeProfile {
            return runtimeProfile.settingsSnapshot()
        }
        return WorkbenchSettingsSnapshot(
            unavailableReason: runtimeError
                ?? "Settings are unavailable while the runtime is opening."
        )
    }

    @MainActor
    private func openRuntimeIfNeeded() async {
        guard runtimeProfile == nil,
              runtimeError == nil,
              !isOpeningRuntime
        else {
            return
        }
        isOpeningRuntime = true
        defer { isOpeningRuntime = false }
        let uiTestScenario = ProcessInfo.processInfo.environment[
            "NMP_WORKBENCH_UI_TEST_SCENARIO"
        ]
        do {
            runtimeProfile = try await Task.detached {
                if let uiTestScenario {
                    return try WorkbenchRuntimeProfile.openForUITesting(
                        scenario: uiTestScenario
                    )
                }
                return try WorkbenchRuntimeProfile.openDefault()
            }.value
        } catch {
            runtimeError = error.localizedDescription
        }
    }

    @MainActor
    private func performProfileAction(
        _ action: WorkbenchProfileAction
    ) async throws {
        guard let profile = runtimeProfile else {
            throw WorkbenchPreferencesError.unavailable(
                "Preferences are unavailable while the app is opening."
            )
        }
        let restartRequired = try await Task.detached {
            switch action {
            case let .savePreferences(preferences):
                return try profile.savePreferences(preferences)
            case .clearNetworkCache:
                try profile.clearNetworkCache()
                return true
            }
        }.value
        guard restartRequired else {
            return
        }
        do {
            let reopened = try await Task.detached {
                profile.close()
                return try profile.reopened()
            }.value
            runtimeError = nil
            runtimeProfile = reopened
        } catch {
            runtimeProfile = nil
            runtimeError = error.localizedDescription
            throw error
        }
    }
}
