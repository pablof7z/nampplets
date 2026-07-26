import Foundation

extension WorkbenchRuntimeProfile {
    /// Opens a fresh, transient profile for one named UI-test scenario.
    ///
    /// The root remains inside the app's sandboxed temporary directory. A
    /// finite scenario name selects one reusable directory, which is cleared
    /// before launch so persisted grants and accounts cannot leak between
    /// developer runs or CI machines.
    public static func openForUITesting(
        scenario: String
    ) throws -> WorkbenchRuntimeProfile {
        guard
            !scenario.isEmpty,
            scenario.utf8.count <= 64,
            scenario.unicodeScalars.allSatisfy({
                (CharacterSet.lowercaseLetters.contains($0)
                    || CharacterSet.decimalDigits.contains($0)
                    || $0 == "-")
                    && $0.isASCII
            })
        else {
            throw CocoaError(.fileReadInvalidFileName)
        }
        let storageRoot = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "io.f7z.nmp.native-runtime.workbench-ui-tests",
                isDirectory: true
            )
            .appendingPathComponent(scenario, isDirectory: true)
        if FileManager.default.fileExists(atPath: storageRoot.path) {
            try FileManager.default.removeItem(at: storageRoot)
        }
        let profile = try open(
            storageRoot: storageRoot,
            accountPersistence: .transient,
            permissionMode: scenario == "full-window-layout-transition"
                ? .demoPinnedGoodMorning
                : .interactive
        )
        if scenario == "full-window-layout-transition" {
            let registration = profile.native.registerLocalAccount(
                secretKey: String(repeating: "0", count: 63) + "1"
            )
            guard
                registration.accepted,
                let handle = registration.handle,
                profile.native.activateLocalAccount(handle: handle).accepted
            else {
                profile.close()
                throw CocoaError(.userCancelled)
            }
        }
        return profile
    }
}
