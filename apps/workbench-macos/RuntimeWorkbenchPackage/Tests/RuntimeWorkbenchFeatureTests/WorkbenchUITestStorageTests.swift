@testable import RuntimeWorkbenchFeature
import Foundation
import Testing

private func temporaryContainer() -> URL {
    FileManager.default.temporaryDirectory
        .appendingPathComponent(
            "workbench-ui-test-storage-\(UUID().uuidString)",
            isDirectory: true
        )
}

@Test
func uiTestStorageRootsAreScopedToTheInjectedRun() throws {
    let container = temporaryContainer()
    defer { try? FileManager.default.removeItem(at: container) }

    let first = try WorkbenchUITestStorage.prepareStorageRoot(
        scenario: "good-morning-permission-launch",
        environment: [WorkbenchUITestStorage.runIdentifierKey: "run-a"],
        container: container
    )
    let second = try WorkbenchUITestStorage.prepareStorageRoot(
        scenario: "good-morning-permission-launch",
        environment: [WorkbenchUITestStorage.runIdentifierKey: "run-b"],
        container: container
    )

    #expect(first != second)
    #expect(first.deletingLastPathComponent().lastPathComponent == "run-a")
    #expect(second.deletingLastPathComponent().lastPathComponent == "run-b")
    #expect(first.lastPathComponent == "good-morning-permission-launch")
}

@Test
func uiTestStorageMintsAPrivateRunWhenTheRunnerInjectsNone() throws {
    let container = temporaryContainer()
    defer { try? FileManager.default.removeItem(at: container) }

    let first = try WorkbenchUITestStorage.prepareStorageRoot(
        scenario: "scenario",
        environment: [:],
        container: container
    )
    let second = try WorkbenchUITestStorage.prepareStorageRoot(
        scenario: "scenario",
        environment: [:],
        container: container
    )

    #expect(first != second)
}

@Test
func uiTestStorageClearsOnlyTheCallingRunsScenario() throws {
    let manager = FileManager.default
    let container = temporaryContainer()
    defer { try? manager.removeItem(at: container) }

    let mine = container
        .appendingPathComponent("run-a", isDirectory: true)
        .appendingPathComponent("scenario", isDirectory: true)
    let theirs = container
        .appendingPathComponent("run-b", isDirectory: true)
        .appendingPathComponent("scenario", isDirectory: true)
    for root in [mine, theirs] {
        try manager.createDirectory(at: root, withIntermediateDirectories: true)
    }

    let prepared = try WorkbenchUITestStorage.prepareStorageRoot(
        scenario: "scenario",
        environment: [WorkbenchUITestStorage.runIdentifierKey: "run-a"],
        container: container
    )

    #expect(prepared == mine)
    #expect(!manager.fileExists(atPath: mine.path))
    #expect(manager.fileExists(atPath: theirs.path))
}

@Test
func uiTestStorageRefusesScenarioAndRunNamesOutsideTheFiniteAlphabet() {
    let container = temporaryContainer()
    defer { try? FileManager.default.removeItem(at: container) }

    for scenario in ["../escape", "Scenario", String(repeating: "a", count: 65), ""] {
        #expect(throws: (any Error).self) {
            try WorkbenchUITestStorage.prepareStorageRoot(
                scenario: scenario,
                environment: [:],
                container: container
            )
        }
    }
    for run in ["../escape", "RUN", String(repeating: "a", count: 65), ""] {
        #expect(throws: (any Error).self) {
            try WorkbenchUITestStorage.prepareStorageRoot(
                scenario: "scenario",
                environment: [WorkbenchUITestStorage.runIdentifierKey: run],
                container: container
            )
        }
    }
}

@Test
func uiTestStorageSweepsAbandonedRunRootsAndSparesLiveOnes() throws {
    let manager = FileManager.default
    let container = temporaryContainer()
    defer { try? manager.removeItem(at: container) }

    let abandoned = container.appendingPathComponent("run-dead", isDirectory: true)
    let concurrent = container.appendingPathComponent("run-live", isDirectory: true)
    for root in [abandoned, concurrent] {
        try manager.createDirectory(at: root, withIntermediateDirectories: true)
    }
    let now = Date()
    try manager.setAttributes(
        [.modificationDate: now.addingTimeInterval(
            -WorkbenchUITestStorage.abandonedRunRootAge - 60
        )],
        ofItemAtPath: abandoned.path
    )

    _ = try WorkbenchUITestStorage.prepareStorageRoot(
        scenario: "scenario",
        environment: [WorkbenchUITestStorage.runIdentifierKey: "run-mine"],
        container: container,
        now: now
    )

    #expect(!manager.fileExists(atPath: abandoned.path))
    #expect(manager.fileExists(atPath: concurrent.path))
}
