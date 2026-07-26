import Foundation
import XCTest

final class RuntimeWorkbenchUITests: XCTestCase {
    static let liveCatalogOptInMarker =
        "/tmp/nampplets-run-live-catalog-ui-test"
    static let maximumLiveReviewAttempts = 8
    static let uiTestSigningSecret =
        String(repeating: "0", count: 63) + "1"
    static let uiTestSigningPublicKey =
        "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
    /// Mirrors `WorkbenchUITestStorage.runIdentifierKey`; the UI test bundle
    /// drives the app out of process and does not link the feature package.
    private static let runIdentifierKey = "NMP_WORKBENCH_UI_TEST_RUN_ID"

    /// Names the transient storage root this run owns, so no other run of the
    /// Workbench can clear it. One identifier per test keeps the root stable
    /// across every launch the test makes.
    private var runIdentifier = UUID().uuidString.lowercased()

    /// When the running test method started, so the app-liveness diagnostic
    /// below can report *how far in* a failure landed.
    private(set) var testStartedAt = Date()

    override func setUpWithError() throws {
        continueAfterFailure = false
        testStartedAt = Date()
        runIdentifier = UUID().uuidString.lowercased()
    }

    /// Hands the app under test the storage root it may clear.
    ///
    /// The runner deliberately does not remove the root itself: the Workbench
    /// is sandboxed, so the directory lives in its container tmp and is not
    /// visible from this process. The app reclaims finished runs' roots on its
    /// next launch instead.
    func isolateStorage(of app: XCUIApplication) {
        app.launchEnvironment[Self.runIdentifierKey] = runIdentifier
    }

    /// Captures the app-liveness diagnostic while XCTest records the failure,
    /// early enough to distinguish app termination from connection loss.
    override func record(_ issue: XCTIssue) {
        logAppLivenessDiagnostic(for: issue)
        super.record(issue)
    }

    @MainActor
    func testWorkbenchReviewsPermissionsThenLaunchesSignedGoodMorning() throws {
        let app = XCUIApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["NMP_WORKBENCH_UI_TEST_SCENARIO"] =
            "good-morning-permission-launch"
        isolateStorage(of: app)
        // The Workbench bundles no napplet, so this test supplies the one it
        // reviews and launches, from the pinned conformance corpus.
        try seedGoodMorning(into: app)
        app.launch()
        app.activate()

        let initialPermissionConfirm = app.buttons["permission-confirm"]
        XCTAssertTrue(
            initialPermissionConfirm.waitForExistence(timeout: 10),
            "The exact build must enter native permission review"
        )
        let declineInitialReview = app.buttons["Not Now"].firstMatch
        XCTAssertTrue(
            declineInitialReview.waitForExistence(timeout: 10),
            "The native permission review must offer a way to decline"
        )
        declineInitialReview.click()
        XCTAssertTrue(
            waitForNonexistence(of: initialPermissionConfirm, timeout: 10)
        )

        registerAndActivateDeterministicAccount(in: app)

        let reopenReview = app.descendants(matching: .any)[
            "review-installed-permissions"
        ]
        XCTAssertTrue(
            reopenReview.waitForExistence(timeout: 10),
            "Installation must place a recoverable permission action on the canvas"
        )
        reopenReview.click()

        // Every domain the fixture declares is required now that no runtime
        // code pins a profile onto its identity. `link` and `resource` have
        // no provider on this build, so they stay at their default denial and
        // launch drops them; `theme` does have one, so it must be granted or
        // the launch is refused.
        for domain in ["identity", "inc", "outbox", "theme"] {
            grantPermission(
                domain: domain,
                in: app,
                message: "The \(domain) switch must be reachable in the review"
            )
        }

        let confirm = app.descendants(matching: .any)["permission-confirm"]
        XCTAssertTrue(
            confirm.waitForExistence(timeout: 10),
            "The review must offer confirmation once every domain is decided"
        )
        confirm.click()
        XCTAssertTrue(
            waitForNonexistence(of: confirm, timeout: 20),
            "The Good Morning exact permission batch must apply before launch"
        )

        XCTAssertTrue(
            app.groups["bundled-napplet"].waitForExistence(timeout: 10)
        )
        XCTAssertTrue(
            app.radioGroups["View mode"].waitForExistence(timeout: 10),
            "Good Morning must pass its essential NAP check after launch"
        )
        XCTAssertFalse(
            app.staticTexts["good-morning can't start here"].exists
        )
        XCTAssertEqual(
            app.staticTexts.matching(
                NSPredicate(
                    format: "value CONTAINS %@ OR label CONTAINS %@",
                    "NAP-OUTBOX",
                    "NAP-OUTBOX"
                )
            ).count,
            0,
            "No full or partial runtime warning may report NAP-OUTBOX absent"
        )
    }
}
