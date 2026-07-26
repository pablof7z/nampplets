import Foundation
import NMPNativeRuntimeApple
@testable import RuntimeWorkbenchFeature
import Testing

@MainActor
@Test func nativeActivityAdapterProjectsOnlyMechanicallyTypedBuildState() throws {
    let root = temporaryActivityRuntimeRoot()
    defer { try? FileManager.default.removeItem(at: root) }

    let profile = try WorkbenchRuntimeProfile.open(storageRoot: root)
    defer { profile.close() }
    let fixture = try GoodMorningFixture.load()
    let artifact = try installApproveAndLaunchGoodMorning(
        fixture: fixture,
        profile: profile
    )
    let scope = try #require(goodMorningActivityScope())
    let source = try RuntimeWorkbenchActivitySource(
        profile: profile,
        scope: scope
    )

    var update: ActivityUpdate?
    let subscription = source.subscribe(to: scope) {
        update = $0
    }
    defer { subscription.cancel() }

    guard case let .authoritative(snapshot) = update else {
        Issue.record("The real source did not synchronously replace activity")
        return
    }

    #expect(snapshot.scope == scope)
    #expect(snapshot.inventory.activeSessions == 1)
    #expect(snapshot.inventory.activeBindings == 0)
    #expect(snapshot.inventory.activeResources == 0)
    #expect(snapshot.inventory.pendingReceipts == 0)
    #expect(snapshot.facts.isEmpty)
    #expect(snapshot.omittedFactCount > 0)

    _ = artifact
}

@MainActor
@Test func nativeActivityAdapterDoesNotLeakAnotherExactBuild() throws {
    let root = temporaryActivityRuntimeRoot()
    defer { try? FileManager.default.removeItem(at: root) }

    let profile = try WorkbenchRuntimeProfile.open(storageRoot: root)
    defer { profile.close() }
    let fixture = try GoodMorningFixture.load()
    let artifact = try installApproveAndLaunchGoodMorning(
        fixture: fixture,
        profile: profile
    )
    let scope = try #require(goodMorningActivityScope())
    let source = try RuntimeWorkbenchActivitySource(
        profile: profile,
        scope: scope
    )
    let unrelatedScope = try #require(
        ActivityExactBuildScope(
            manifestAuthor: String(repeating: "f", count: 64),
            dTag: GoodMorningFixture.dTag,
            aggregateHash: GoodMorningFixture.aggregateHash
        )
    )

    do {
        _ = try source.refresh(scope: unrelatedScope)
        Issue.record("A cross-build refresh returned a fabricated snapshot")
    } catch let refusal as RuntimeWorkbenchActivitySourceRefusal {
        #expect(refusal == .scopeMismatch)
    }
    #expect(source.latestAdmissionRefusal == .scopeMismatch)

    _ = artifact
}

@MainActor
@Test func nativeActivitySubscriberCapacityRefusalIsObservable() throws {
    let root = temporaryActivityRuntimeRoot()
    defer { try? FileManager.default.removeItem(at: root) }

    let profile = try WorkbenchRuntimeProfile.open(storageRoot: root)
    defer { profile.close() }
    let scope = try #require(goodMorningActivityScope())
    let source = try RuntimeWorkbenchActivitySource(
        profile: profile,
        scope: scope
    )
    var subscriptions: [any ActivitySubscription] = []
    for _ in 0..<16 {
        subscriptions.append(source.subscribe(to: scope) { _ in })
    }

    let refused = source.subscribe(to: scope) { _ in }

    #expect(
        source.latestAdmissionRefusal
            == .subscriberCapacity(maximum: 16)
    )
    refused.cancel()
    for subscription in subscriptions {
        subscription.cancel()
    }
}

@MainActor
@Test func nativeProfileActivityFanoutIsBoundedAndCancellationReleasesCapacity()
    throws
{
    let root = temporaryActivityRuntimeRoot()
    defer { try? FileManager.default.removeItem(at: root) }

    let profile = try WorkbenchRuntimeProfile.open(storageRoot: root)
    defer { profile.close() }
    let scope = try #require(goodMorningActivityScope())
    var sources = try (0..<8).map { _ in
        try RuntimeWorkbenchActivitySource(profile: profile, scope: scope)
    }

    do {
        _ = try RuntimeWorkbenchActivitySource(
            profile: profile,
            scope: scope
        )
        Issue.record("The ninth native activity observer was admitted")
    } catch {
        #expect(error.localizedDescription.contains("limit of 8"))
    }

    sources.removeLast()
    let replacement = try RuntimeWorkbenchActivitySource(
        profile: profile,
        scope: scope
    )
    sources.append(replacement)
    #expect(sources.count == 8)
}

@Test func detailFieldsCarryTheRuntimeClassificationWithoutReinterpreting()
    throws
{
    let withheld = try #require(
        ActivityDetailField(
            NativeRuntimeActivityDetail(
                key: "approved-draft",
                value: .redacted
            )
        )
    )
    // The runtime classified this one public. The old Swift heuristic would
    // have redacted it on the strength of the key alone.
    let shown = try #require(
        ActivityDetailField(
            NativeRuntimeActivityDetail(
                key: "token-relay",
                value: .visible("wss://relay.example")
            )
        )
    )

    #expect(withheld.isRedacted)
    #expect(withheld.displayValue == "[REDACTED]")
    #expect(!shown.isRedacted)
    #expect(shown.displayValue == "wss://relay.example")
}

private func goodMorningActivityScope() -> ActivityExactBuildScope? {
    ActivityExactBuildScope(
        manifestAuthor: GoodMorningFixture.author,
        dTag: GoodMorningFixture.dTag,
        aggregateHash: GoodMorningFixture.aggregateHash
    )
}

private func temporaryActivityRuntimeRoot() -> URL {
    FileManager.default.temporaryDirectory
        .appendingPathComponent(
            "nmp-native-runtime-activity-\(UUID().uuidString)",
            isDirectory: true
        )
}
