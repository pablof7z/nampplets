@testable import RuntimeWorkbenchFeature
import Testing

/// The library sheet shows only `refusals.last`. With no counts beside it, one
/// banner reads as one refusal — and the ones the runtime evicted to stay
/// inside its bound leave no trace at all.
@MainActor
@Test func aLoneRefusalStatesNoCounts() {
    let fields = WorkbenchLibrarySheet.refusalEvidenceFields(
        libraryRefusal,
        retainedCount: 1,
        droppedCount: 0
    )

    #expect(fields.map(\.label) == [
        "Code",
        "Detail",
        "Occurred at milliseconds",
    ])
}

/// More refusals than the one on screen: say so, without claiming any were
/// discarded, because none were.
@MainActor
@Test func retainedRefusalsBeyondTheShownOneAreCounted() {
    let fields = WorkbenchLibrarySheet.refusalEvidenceFields(
        libraryRefusal,
        retainedCount: 4,
        droppedCount: 0
    )

    #expect(fields.first { $0.label == "Refusals recorded" }?.value
        == "4, showing the most recent")
    #expect(!fields.contains { $0.label == "Older refusals discarded" })
}

/// The case the retained list cannot express on its own: the runtime refused
/// twelve times, kept two, and destroyed ten. Reporting "2" would understate
/// it by an order of magnitude.
@MainActor
@Test func discardedRefusalsAreCountedIntoTheRecordedTotal() {
    let fields = WorkbenchLibrarySheet.refusalEvidenceFields(
        libraryRefusal,
        retainedCount: 2,
        droppedCount: 10
    )

    #expect(fields.first { $0.label == "Refusals recorded" }?.value
        == "12, showing the most recent")
    #expect(fields.first { $0.label == "Older refusals discarded" }?.value
        == "10")
}

/// Everything discarded and nothing retained is still a truthful count, and it
/// is the shape where silence would be most misleading.
@MainActor
@Test func discardsAreStatedEvenWhenOnlyOneRefusalSurvived() {
    let fields = WorkbenchLibrarySheet.refusalEvidenceFields(
        libraryRefusal,
        retainedCount: 1,
        droppedCount: 9
    )

    #expect(fields.first { $0.label == "Refusals recorded" }?.value
        == "10, showing the most recent")
    #expect(fields.first { $0.label == "Older refusals discarded" }?.value
        == "9")
}

private let libraryRefusal = WorkbenchLibraryRefusal(
    code: "workspace-projection",
    message: "The workspace could not be projected.",
    occurredAtMillis: 1
)!
