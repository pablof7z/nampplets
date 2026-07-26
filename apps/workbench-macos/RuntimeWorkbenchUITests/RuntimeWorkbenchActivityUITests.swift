import XCTest

extension RuntimeWorkbenchUITests {
    @MainActor
    func testActivityUsesAdmittedSourceOnFirstPresentation() throws {
        let app = XCUIApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["NMP_WORKBENCH_UI_TEST_SCENARIO"] =
            "good-morning-permission-launch"
        app.launch()
        app.activate()

        let permissionConfirm = app.buttons["permission-confirm"]
        XCTAssertTrue(permissionConfirm.waitForExistence(timeout: 10))
        let cancelReview = app.buttons["Cancel"].firstMatch
        XCTAssertTrue(cancelReview.waitForExistence(timeout: 2))
        cancelReview.click()
        XCTAssertTrue(
            waitForNonexistence(of: permissionConfirm, timeout: 10)
        )

        let workspaceActions = app.menuButtons["Workspace Actions"]
        XCTAssertTrue(workspaceActions.waitForExistence(timeout: 5))
        workspaceActions.click()
        let activity = app.menuItems["Activity"]
        XCTAssertTrue(activity.waitForExistence(timeout: 2))
        activity.click()

        let drawer = app.descendants(matching: .any)
            .matching(
                NSPredicate(
                    format: "label BEGINSWITH %@ OR value BEGINSWITH %@",
                    "Activity for exact build good-morning",
                    "Activity for exact build good-morning"
                )
            )
            .firstMatch
        XCTAssertTrue(
            drawer.waitForExistence(timeout: 10),
            "The first Activity presentation must show its admitted exact build"
        )
        XCTAssertFalse(app.staticTexts["Activity unavailable"].exists)
    }
}
