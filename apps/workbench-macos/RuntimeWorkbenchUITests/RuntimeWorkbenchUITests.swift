import Foundation
import XCTest

final class RuntimeWorkbenchUITests: XCTestCase {
    private static let liveCatalogOptInMarker =
        "/tmp/nampplets-run-live-catalog-ui-test"
    private static let maximumLiveReviewAttempts = 8
    static let uiTestSigningSecret =
        String(repeating: "0", count: 63) + "1"
    static let uiTestSigningPublicKey =
        "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"

    /// When the running test method started, so the app-liveness diagnostic
    /// below can report *how far in* a failure landed. The reported deaths
    /// are spread across the timeline (1.8s / 3s / 17s / 25s in #137), and
    /// that spread is only interpretable against a start time.
    private(set) var testStartedAt = Date()

    override func setUpWithError() throws {
        // Put setup code here. This method is called before the invocation of each test method in the class.

        // In UI tests it is usually best to stop immediately when a failure occurs.
        continueAfterFailure = false
        testStartedAt = Date()

        // In UI tests it’s important to set the initial state - such as interface orientation - required for your tests before they run. The setUp method is a good place to do this.
    }

    override func tearDownWithError() throws {
        // Put teardown code here. This method is called after the invocation of each test method in the class.
    }

    /// Captures the app-liveness diagnostic at the instant a failure is
    /// recorded.
    ///
    /// This override is the whole point: `record(_:)` runs while XCTest is
    /// still recording the issue, which is early enough for "is the app
    /// still there?" to have a meaningful answer. A `tearDown`-based check
    /// would be worthless — by then XCTest may already have reaped the app,
    /// so it would report "gone" for termination and connection loss alike,
    /// which is precisely the distinction it exists to draw.
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
        app.launch()
        app.activate()

        let initialPermissionConfirm = app.buttons["permission-confirm"]
        XCTAssertTrue(
            initialPermissionConfirm.waitForExistence(timeout: 10),
            "The exact build must enter native permission review"
        )
        // "Not Now" rather than "Cancel": this dismissal declines access
        // rather than abandoning an edit.
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

        // Queried by identifier, not label: the visible wording is product
        // copy and is expected to change, while the identifier is the stable
        // contract (the same rule `workspace-actions` already follows).
        // `descendants(matching: .any)`, like every other identifier query in
        // this suite: the rendered element is not reliably typed as a button.
        let reopenReview = app.descendants(matching: .any)[
            "review-installed-permissions"
        ]
        XCTAssertTrue(
            reopenReview.waitForExistence(timeout: 10),
            "Installation must place a recoverable permission action on the canvas"
        )
        reopenReview.click()

        for domain in ["identity", "inc", "outbox"] {
            let decision = scrollPermissionDecisionIntoView(
                domain: domain,
                in: app
            )
            let allow = app.descendants(matching: .any)[
                "permission-\(domain)-allowExactBuild"
            ]
            XCTAssertTrue(openDecisionMenu(decision, revealing: allow, in: app))
            allow.click()
        }

        let confirm = app.descendants(matching: .any)["permission-confirm"]
        XCTAssertTrue(
            confirm.waitForExistence(timeout: 10),
            "The review must offer a confirmation control once every domain is decided"
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

    @MainActor
    func testLiveCatalogInstallsAndMountsVerifiedNetworkNapplet() throws {
        try XCTSkipUnless(
            Self.liveCatalogTestIsEnabled,
            "The live relay-backed catalog journey is opt-in. Set "
                + "NMP_RUN_LIVE_CATALOG_UI_TEST=1 or create "
                + Self.liveCatalogOptInMarker
        )

        let app = XCUIApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launch()
        app.activate()

        let addNapplet = app.descendants(matching: .any)["add-napplet"]
        XCTAssertTrue(addNapplet.waitForExistence(timeout: 10))
        addNapplet.click()

        let liveScope = app.descendants(matching: .any)[
            "catalog-feed-evidence"
        ]
        XCTAssertTrue(
            liveScope.waitForExistence(timeout: 30),
            "The sheet must identify the permanent feed as a bounded live NMP window"
        )
        // The scope is still projected in full, one deliberate move away in
        // the "Where these came from" evidence. What the footer says on the
        // plain path is the only part that changes what a person does next:
        // whether this list is everything. Asserting the old ambient
        // "Live NMP catalog window" string here would be asserting the defect
        // ADR 0008 removed.
        let feedEvidence = app.descendants(matching: .any)[
            "Where these came from"
        ].firstMatch
        XCTAssertTrue(
            feedEvidence.waitForExistence(timeout: 10),
            "The feed must offer its source evidence"
        )
        feedEvidence.click()
        XCTAssertTrue(
            app.staticTexts.containing(
                NSPredicate(format: "value CONTAINS %@", "live NMP window")
            ).firstMatch.waitForExistence(timeout: 10),
            "Opening the evidence must name the bounded live window verbatim"
        )

        // Keep this a real network journey while selecting a known current
        // public candidate whose signed blob is reachable. The search is a
        // local filter over the permanent bounded window, never a new relay
        // query or a fixture substitution.
        let search = app.textFields["Search napplets"]
        XCTAssertTrue(search.waitForExistence(timeout: 5))
        search.click()
        search.typeText("STL Preview")
        // Submitting the field runs the filter; the separate Search button is
        // gone, because a search field that needs a button beside it has not
        // finished being designed.
        search.typeText("\r")

        let catalogEntries = app.buttons.matching(identifier: "catalog-entry")
        XCTAssertTrue(
            catalogEntries.firstMatch.waitForExistence(timeout: 60),
            "The production NMP catalog should project a bounded network result"
        )
        // The permanent expandable window may deliver an initial small page
        // before its next replacement adds more public candidates. Give the
        // subscription one event-driven opportunity to expose the next rows.
        _ = catalogEntries
            .element(boundBy: Self.maximumLiveReviewAttempts - 1)
            .waitForExistence(timeout: 30)

        let attempts = min(
            catalogEntries.count,
            Self.maximumLiveReviewAttempts
        )
        XCTAssertGreaterThan(
            attempts,
            0,
            "The permanent feed must expose at least one network napplet"
        )

        var installedExactBuild = false
        for index in 0 ..< attempts {
            let entry = catalogEntries.element(boundBy: index)
            guard entry.waitForExistence(timeout: 2), entry.isHittable else {
                continue
            }
            entry.click()

            let installExactBuild = app.buttons[
                "catalog-install-exact-build"
            ]
            guard installExactBuild.waitForExistence(timeout: 20) else {
                continue
            }
            guard installExactBuild.isEnabled else {
                dismissCatalogReview(in: app)
                continue
            }

            // The control still installs exactly the hash under review — that
            // is enforced by `CatalogInstallConfirmation`, not by its wording.
            // What this asserts is that the review offers one install action
            // and that it is the enabled, default one.
            XCTAssertTrue(
                installExactBuild.label.contains("Add Napplet"),
                "The review must offer a single install action"
            )
            installExactBuild.click()

            if waitForNonexistence(
                of: installExactBuild,
                timeout: 20
            ) {
                installedExactBuild = true
                break
            }

            // A real source can disappear between review and acquisition.
            // Try another already-bounded feed entry without retrying this
            // consumed exact review.
            dismissCatalogReview(in: app)
        }

        XCTAssertTrue(
            installedExactBuild,
            "At least one bounded network candidate should complete exact verified installation"
        )

        let permissionConfirm = app.buttons["permission-confirm"]
        XCTAssertTrue(
            permissionConfirm.waitForExistence(timeout: 10),
            "The installed STL Preview build must enter exact permission review"
        )
        XCTAssertTrue(
            permissionConfirm.isHittable,
            "Permission review must be visibly presented after the catalog closes"
        )

        for domain in ["inc", "link", "resource", "theme"] {
            let decision = scrollPermissionDecisionIntoView(
                domain: domain,
                in: app,
                message: "The \(domain) decision must be reachable in the native review"
            )
            let allow = app.descendants(matching: .any)[
                "permission-\(domain)-allowExactBuild"
            ]
            XCTAssertTrue(
                openDecisionMenu(decision, revealing: allow, in: app),
                "The runtime must offer an exact-build grant for \(domain)"
            )
            allow.click()
        }

        XCTAssertTrue(permissionConfirm.isEnabled)
        permissionConfirm.click()
        XCTAssertTrue(
            waitForNonexistence(of: permissionConfirm, timeout: 20),
            "The exact permission batch must apply before launch"
        )

        XCTAssertTrue(
            app.groups["bundled-napplet"].waitForExistence(timeout: 30),
            "The verified public artifact must create a trusted napplet surface"
        )
        XCTAssertTrue(
            app.staticTexts["Waiting for an STL to preview..."]
                .waitForExistence(timeout: 30),
            "The signed public napplet's own DOM must mount and pass its NAP-INC readiness check"
        )
        XCTAssertTrue(
            app.groups["napplet-canvas"].exists,
            "The mounted public napplet must remain inside the native canvas"
        )
        XCTAssertFalse(
            app.descendants(matching: .any)
                .matching(
                    NSPredicate(
                        format: "label BEGINSWITH %@ OR value BEGINSWITH %@",
                        "Refused:",
                        "Refused:"
                    )
                )
                .firstMatch
                .exists,
            "The installed public build must not be reported as refused"
        )
    }

    private static var liveCatalogTestIsEnabled: Bool {
        ProcessInfo.processInfo.environment[
            "NMP_RUN_LIVE_CATALOG_UI_TEST"
        ] == "1"
            || FileManager.default.fileExists(
                atPath: liveCatalogOptInMarker
            )
    }

}
