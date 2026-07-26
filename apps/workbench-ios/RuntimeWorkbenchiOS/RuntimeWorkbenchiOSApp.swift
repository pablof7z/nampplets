import RuntimeWorkbenchFeature
import SwiftUI

@main
struct RuntimeWorkbenchiOSApp: App {
    @State private var runtimeProfile: WorkbenchRuntimeProfile?
    @State private var runtimeError: String?
    @State private var isOpeningRuntime = false

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
                }
            }
            .task {
                await openRuntimeIfNeeded()
            }
        }
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
