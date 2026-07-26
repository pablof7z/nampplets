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

        // Every wait in this sequence uses the suite's standard budget. A
        // shorter one is not a meaningful optimisation: it only decides which
        // step trips first when the machine is loaded, and these run against
        // the shared desktop session (see #137, #147).
        // Queried by accessibility identifier, like every other control in
        // this suite. A `menuButtons["Workspace Actions"]` label query does
        // not match: the menu is `.labelStyle(.iconOnly)`, so its rendered
        // element carries neither that title nor the `menuButton` type.
        let workspaceActions = app.descendants(matching: .any)[
            "workspace-actions"
        ]
        XCTAssertTrue(
            workspaceActions.waitForExistence(timeout: 10),
            "The workspace actions menu must appear after the review is dismissed"
        )
        workspaceActions.click()
        let activity = app.menuItems["Activity"]
        XCTAssertTrue(
            activity.waitForExistence(timeout: 10),
            "The workspace actions menu must offer the Activity item"
        )
        activity.click()

        let drawer = app.descendants(matching: .any)["runtime-activity-drawer"]
        XCTAssertTrue(
            drawer.waitForExistence(timeout: 10),
            "Activity must present its admitted source, not the fallback"
        )
        XCTAssertFalse(app.staticTexts["Nothing to show yet"].exists)

        // The scope is still fully projected -- one deliberate move away,
        // which is the whole shape of ADR 0008. Asserting it here proves the
        // evidence was relocated rather than dropped.
        let evidence = drawer.descendants(matching: .any)[
            "Which build this is"
        ].firstMatch
        XCTAssertTrue(
            evidence.waitForExistence(timeout: 10),
            "The drawer must offer its exact build as evidence"
        )
        evidence.click()

        let projectedDTag = drawer.staticTexts["good-morning"]
        XCTAssertTrue(
            projectedDTag.waitForExistence(timeout: 10),
            "Opening the evidence must show the admitted exact build verbatim"
        )
    }
}
