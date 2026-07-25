import Foundation

/// Run-scoped storage roots for Workbench UI-test profiles.
///
/// A root keyed only by scenario name is one fixed path that every UI-test run
/// on the machine both writes to and deletes on launch, so one run can clear
/// another's live profile. The Workbench is sandboxed, so that path is the
/// app's container tmp rather than `$TMPDIR`, which narrows the hazard to
/// concurrent instances of this app — but shared mutable state that
/// self-deletes is worth removing on its own merits.
///
/// Each run instead claims a directory named by its own run identifier and
/// only ever deletes inside that directory. The identifier is injected by the
/// test runner (`NMP_WORKBENCH_UI_TEST_RUN_ID`) so it stays stable if the same
/// run relaunches the app; a launch without one still gets a private root.
///
/// Cleanup is the app's job, not the runner's: the container tmp is reachable
/// only from inside the sandbox, so the test bundle cannot see the path it
/// would need to remove. Every launch therefore reclaims the roots left by
/// runs that are already over.
enum WorkbenchUITestStorage {
    /// Directory grouping every run's transient UI-test profiles.
    static let containerName = "io.f7z.nmp.native-runtime.workbench-ui-tests"

    /// Launch environment key carrying the runner's run identifier.
    static let runIdentifierKey = "NMP_WORKBENCH_UI_TEST_RUN_ID"

    /// Longest a run root may go untouched before it counts as abandoned.
    ///
    /// Modification time is the only liveness signal available across
    /// processes, so this stays far longer than any UI-test run: reclaiming
    /// disk late is harmless, reclaiming a live run's root is not.
    static let abandonedRunRootAge: TimeInterval = 60 * 60

    static var defaultContainer: URL {
        FileManager.default.temporaryDirectory
            .appendingPathComponent(containerName, isDirectory: true)
    }

    /// Returns an empty storage root owned solely by the calling run.
    ///
    /// The scenario directory is cleared if a previous launch of *this* run
    /// left one behind, and roots abandoned by dead runners are swept. No
    /// other live run's root is ever touched.
    static func prepareStorageRoot(
        scenario: String,
        environment: [String: String] = ProcessInfo.processInfo.environment,
        container: URL = defaultContainer,
        now: Date = Date()
    ) throws -> URL {
        let scenarioComponent = try validatedComponent(scenario)
        let run = try runIdentifier(from: environment)
        let storageRoot = container
            .appendingPathComponent(run, isDirectory: true)
            .appendingPathComponent(scenarioComponent, isDirectory: true)
        if FileManager.default.fileExists(atPath: storageRoot.path) {
            try FileManager.default.removeItem(at: storageRoot)
        }
        sweepAbandonedRunRoots(in: container, keeping: run, now: now)
        return storageRoot
    }

    /// Resolves the run identifier, minting a private one when none is set.
    ///
    /// An identifier supplied by the runner is held to the same finite
    /// character rule as the scenario name, so neither component can widen
    /// the path beyond one lowercase ASCII directory name.
    static func runIdentifier(
        from environment: [String: String]
    ) throws -> String {
        guard let supplied = environment[runIdentifierKey] else {
            return UUID().uuidString.lowercased()
        }
        return try validatedComponent(supplied)
    }

    /// Accepts one finite lowercase ASCII path component, or refuses.
    static func validatedComponent(_ value: String) throws -> String {
        guard
            !value.isEmpty,
            value.utf8.count <= 64,
            value.unicodeScalars.allSatisfy({
                (CharacterSet.lowercaseLetters.contains($0)
                    || CharacterSet.decimalDigits.contains($0)
                    || $0 == "-")
                    && $0.isASCII
            })
        else {
            throw CocoaError(.fileReadInvalidFileName)
        }
        return value
    }

    /// Removes the roots of runs that are already over.
    static func sweepAbandonedRunRoots(
        in container: URL,
        keeping current: String,
        now: Date
    ) {
        let manager = FileManager.default
        guard
            let entries = try? manager.contentsOfDirectory(
                at: container,
                includingPropertiesForKeys: [.contentModificationDateKey],
                options: [.skipsHiddenFiles]
            )
        else {
            return
        }
        for entry in entries where entry.lastPathComponent != current {
            let modified = try? entry.resourceValues(
                forKeys: [.contentModificationDateKey]
            ).contentModificationDate
            guard
                let modified,
                now.timeIntervalSince(modified) > abandonedRunRootAge
            else {
                continue
            }
            try? manager.removeItem(at: entry)
        }
    }
}
