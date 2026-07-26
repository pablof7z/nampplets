import XCTest

extension RuntimeWorkbenchUITests {
    @MainActor
    func testPreferencesUsePlainLanguageAndExposeBoundedChoices() {
        let app = XCUIApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["NMP_WORKBENCH_UI_TEST_SCENARIO"] =
            "preferences"
        app.launch()
        app.activate()

        let workspaceActions = app.descendants(matching: .any)[
            "workspace-actions"
        ]
        XCTAssertTrue(workspaceActions.waitForExistence(timeout: 10))
        workspaceActions.click()

        let settings = app.menuItems["Settings"]
        XCTAssertTrue(settings.waitForExistence(timeout: 10))
        settings.click()

        XCTAssertTrue(
            app.staticTexts["Connections"].waitForExistence(timeout: 10)
        )
        XCTAssertTrue(app.staticTexts["Everyday relays"].exists)
        XCTAssertTrue(app.staticTexts["Search relays"].exists)
        XCTAssertTrue(app.staticTexts["Permission choices"].exists)
        XCTAssertTrue(app.staticTexts["Storage"].exists)
        XCTAssertTrue(app.buttons["settings-clear-network-cache"].exists)
        XCTAssertFalse(app.staticTexts["Runtime profile"].exists)
        XCTAssertFalse(app.staticTexts["Data ownership"].exists)
    }
}
