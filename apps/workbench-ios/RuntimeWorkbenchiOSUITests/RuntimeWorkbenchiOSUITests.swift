import XCTest

final class RuntimeWorkbenchiOSUITests: XCTestCase {
    override func setUpWithError() throws {
        // Put setup code here. This method is called before the invocation of each test method in the class.

        // In UI tests it is usually best to stop immediately when a failure occurs.
        continueAfterFailure = false

        // In UI tests it’s important to set the initial state - such as interface orientation - required for your tests before they run. The setUp method is a good place to do this.
    }

    override func tearDownWithError() throws {
        // Put teardown code here. This method is called after the invocation of each test method in the class.
    }

    @MainActor
    func testExample() throws {
        // UI tests must launch the application that they test.
        let app = XCUIApplication()
        app.launch()

        // Use XCTAssert and related functions to verify your tests produce the correct results.
        XCTAssertTrue(true)
    }

    @MainActor
    func testRunningNappletSurvivesFullWindowLayoutTransition() {
        let app = XCUIApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["NMP_WORKBENCH_UI_TEST_SCENARIO"] =
            "full-window-layout-transition"
        app.launch()

        let mountedContent = app.descendants(matching: .any)
            .matching(
                NSPredicate(format: "label == %@", "View mode")
            )
            .firstMatch
        XCTAssertTrue(
            mountedContent.waitForExistence(timeout: 15),
            "The verified Good Morning napplet must mount before changing layout"
        )

        let layoutMenu = app.descendants(matching: .any)[
            "layout-mode-menu"
        ]
        XCTAssertTrue(layoutMenu.waitForExistence(timeout: 10))
        layoutMenu.tap()
        let fullWindow = app.buttons["Full Window"]
        XCTAssertTrue(fullWindow.waitForExistence(timeout: 10))
        fullWindow.tap()

        let fullWindowSurface = app.descendants(matching: .any)
            .matching(
                NSPredicate(
                    format: "identifier BEGINSWITH %@",
                    "full-window-napplet-"
                )
            )
            .firstMatch
        XCTAssertTrue(fullWindowSurface.waitForExistence(timeout: 10))
        XCTAssertTrue(
            mountedContent.waitForExistence(timeout: 10),
            "Changing layout must remount the same live Rust session"
        )
        XCTAssertFalse(app.staticTexts["Preparing verified napplet…"].exists)
    }
}
