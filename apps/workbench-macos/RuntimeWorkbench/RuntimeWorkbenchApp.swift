import AppKit
import SwiftUI
import RuntimeWorkbenchFeature

final class RuntimeWorkbenchAppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_: Notification) {
        activateWorkbenchWindow()
        DispatchQueue.main.async { [weak self] in
            self?.activateWorkbenchWindow()
        }
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
        guard let visibleFrame = (window.screen ?? NSScreen.main)?.visibleFrame
        else {
            return
        }
        guard !WorkbenchWindowFitting.fits(window.frame, in: visibleFrame)
        else {
            return
        }
        window.setFrame(
            WorkbenchWindowFitting.fitted(window.frame, into: visibleFrame),
            display: true
        )
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
                    ProgressView("Opening runtime…")
                        .frame(minWidth: 1_050, minHeight: 660)
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
