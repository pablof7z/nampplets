import XCTest

extension RuntimeWorkbenchUITests {
    /// The first Activity presentation is the admitted one, not the truthful
    /// "nothing to show yet" fallback, and the build it is scoped to is
    /// reachable from it.
    ///
    /// Renamed from `testActivityUsesAdmittedSourceOnFirstPresentation`, which
    /// asserted a combined `"Activity for exact build <dTag>"` accessibility
    /// label on the drawer's header. That label no longer exists and should
    /// not: `docs/adr/0008-verdicts-on-the-path.md` keeps identifiers off the
    /// plain path *including* in accessibility text, so a header that read a
    /// dTag and an aggregate hash to VoiceOver was the defect, not the
    /// contract. The admitted-vs-unavailable distinction this test exists to
    /// protect is unchanged; only where the evidence lives has moved.
    @MainActor
    func testActivityOpensOnItsAdmittedBuildWithEvidenceOneStepAway() throws {
        let app = XCUIApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["NMP_WORKBENCH_UI_TEST_SCENARIO"] =
            "good-morning-permission-launch"
        isolateStorage(of: app)
        // The Workbench bundles no napplet. Activity is scoped to a build, so
        // this test supplies the build it expects Activity to be admitted on,
        // from the pinned conformance corpus.
        let seededDTag = try seedGoodMorning(into: app)
        app.launch()
        app.activate()

        let permissionConfirm = app.buttons["permission-confirm"]
        XCTAssertTrue(permissionConfirm.waitForExistence(timeout: 10))
        // The permission sheet's dismissal is "Not Now" rather than "Cancel":
        // it declines access rather than abandoning an edit.
        let declineReview = app.buttons["Not Now"].firstMatch
        XCTAssertTrue(
            declineReview.waitForExistence(timeout: 10),
            "The permission review must offer a way to decline"
        )
        declineReview.click()
        XCTAssertTrue(
            waitForNonexistence(of: permissionConfirm, timeout: 10)
        )

        // `.firstMatch` because SwiftUI propagates an `accessibilityIdentifier`
        // from a control onto the children of its label, so an `.any` query
        // resolves both the toolbar Button and the image inside it and a click
        // fails with "Multiple matching elements found". The product declares
        // this identifier exactly once; there is one control, not two. The
        // first match is the outermost element, which is the clickable one.
        // Same reason "Not Now" and the evidence disclosure below use it.
        let inspector = app.buttons["toggle-inspector"].firstMatch
        XCTAssertTrue(
            inspector.waitForExistence(timeout: 10),
            "The Inspector control must appear after the review is dismissed"
        )
        inspector.click()

        // Same propagation trap as `toggle-inspector` above, and it would have
        // surfaced the moment that one passed. `ContentView+Inspector.swift:123`
        // declares this as a `Button`, so the type scope is checked, not guessed.
        let activity = app.buttons["inspector-activity"].firstMatch
        XCTAssertTrue(
            activity.waitForExistence(timeout: 10),
            "Activity must be reachable from the Inspector"
        )
        activity.click()

        // Discriminated by the evidence disclosure rather than by a container
        // identifier. The disclosure exists only in the admitted drawer -- the
        // truthful fallback has no scope to offer -- so its presence proves
        // both that Activity was admitted and that the exact build was
        // relocated rather than dropped, which is the whole shape of ADR 0008.
        // Matched by label because an identifier on an enclosing view
        // propagates over its children in SwiftUI, which is what broke the
        // previous version of this assertion.
        let evidence = app.disclosureTriangles["Which build this is"].firstMatch
        XCTAssertTrue(
            evidence.waitForExistence(timeout: 10),
            "Activity must present its admitted source, not the fallback"
        )

        // AppKit exposes this SwiftUI DisclosureGroup as one labeled
        // DisclosureTriangle. A pointer click on its label does not toggle it
        // reliably, so focus it and use the standard disclosure keyboard
        // action before querying its children.
        evidence.click()
        evidence.typeKey(.rightArrow, modifierFlags: [])
        let expanded = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "value == 1"),
            object: evidence
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [expanded], timeout: 10),
            .completed,
            "The build evidence disclosure must expand"
        )

        let projectedDTag = app.staticTexts.matching(
            NSPredicate(format: "value == %@", seededDTag)
        ).firstMatch
        XCTAssertTrue(
            projectedDTag.waitForExistence(timeout: 10),
            "Opening the evidence must show the admitted exact build verbatim"
        )
    }
}
