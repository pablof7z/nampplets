import Foundation

extension WorkbenchRuntimeProfile {
    /// Opens a fresh, transient profile for one named UI-test scenario.
    ///
    /// The root remains inside the app's sandboxed temporary directory, but
    /// `WorkbenchUITestStorage` scopes it to this run so persisted grants and
    /// accounts cannot leak between runs and concurrent runs on one machine
    /// cannot clear each other's profile.
    public static func openForUITesting(
        scenario: String
    ) throws -> WorkbenchRuntimeProfile {
        try open(
            storageRoot: WorkbenchUITestStorage.prepareStorageRoot(
                scenario: scenario
            ),
            accountPersistence: .transient
        )
    }
}
