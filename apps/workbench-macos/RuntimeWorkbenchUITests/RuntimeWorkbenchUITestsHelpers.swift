import Foundation
import XCTest

#if canImport(AppKit)
    import AppKit
#endif

extension RuntimeWorkbenchUITests {
    /// The identifier the Workbench app builds under — and, because it is
    /// hard-coded rather than derived from the checkout, the *same*
    /// identifier in every worktree on the machine
    /// (`apps/workbench-macos/Config/Shared.xcconfig`,
    /// `PRODUCT_BUNDLE_IDENTIFIER`). See issue #147.
    static let workbenchBundleIdentifier = "io.f7z.nmp.runtime-workbench"

    /// Logs whether the app under test was still alive at the moment a
    /// failure was recorded.
    ///
    /// `Failed to get matching snapshots: Lost connection to the
    /// application (pid …)` has two candidate explanations that produce an
    /// identical message and, in both cases, no crash report — so nobody
    /// has been able to tell them apart from the logs alone:
    ///
    /// * **Termination.** Every worktree builds the same
    ///   `PRODUCT_BUNDLE_IDENTIFIER`, and `XCUIApplication.launch()`
    ///   terminates any already-running instance of that bundle id, so
    ///   concurrent UI runs from different worktrees kill each other's app
    ///   (issue #147). Clean termination, no crash report, death at an
    ///   arbitrary point in the victim's timeline.
    /// * **Connection loss.** The AX / `testmanagerd` channel drops under
    ///   load while the app process itself keeps running.
    ///
    /// The discriminator is simply whether the process is still there:
    /// **app gone ⇒ termination; app alive ⇒ connection loss.**
    ///
    /// Two things make the answer trustworthy. First, this runs from
    /// `record(_:)`, at failure time, not from `tearDown` — see the
    /// override for why that matters. Second, it checks the *specific* pid
    /// named in the failure message wherever one is present, not just "is
    /// something with this bundle id running": under the termination
    /// hypothesis the killer's own app is running under that same bundle
    /// id, so a bundle-id-only lookup would report "alive" for the very
    /// case it is supposed to detect. The bundle-id census is logged
    /// alongside as corroboration — a surviving peer instance with a
    /// different pid is the termination signature.
    ///
    /// Costs nothing on a green run: `record(_:)` only fires on failure.
    func logAppLivenessDiagnostic(for issue: XCTIssue) {
        let description = issue.compactDescription
        var fields = [
            String(
                format: "elapsed=%.2fs",
                Date().timeIntervalSince(testStartedAt)
            ),
            "bundleID=\(Self.workbenchBundleIdentifier)",
        ]

        if let pid = Self.applicationPID(inFailureDescription: description) {
            let alive = Self.processExists(pid)
            fields.append("reportedPID=\(pid)")
            fields.append("reportedPIDAlive=\(alive)")
            fields.append(
                "verdict="
                    + (alive
                        ? "app-alive-so-connection-loss"
                        : "app-gone-so-termination")
            )
        } else {
            fields.append("reportedPID=none")
            fields.append("verdict=no-pid-in-failure-message")
        }

        fields.append(
            "runningInstancePIDs=\(Self.runningWorkbenchProcessIdentifiers())"
        )

        NSLog(
            "app-liveness-at-failure: %@ | issue=%@",
            fields.joined(separator: " "),
            description
        )
    }

    /// The pid XCTest names in `Lost connection to the application (pid N)`,
    /// when the failure carries one.
    static func applicationPID(
        inFailureDescription description: String
    ) -> pid_t? {
        guard
            let range = description.range(
                of: #"pid\s+\d+"#,
                options: [.regularExpression, .caseInsensitive]
            )
        else {
            return nil
        }
        return pid_t(description[range].drop { !$0.isNumber })
    }

    /// Whether `pid` still names a live process. `EPERM` counts as alive:
    /// the process exists, this one simply may not signal it.
    static func processExists(_ pid: pid_t) -> Bool {
        kill(pid, 0) == 0 || errno == EPERM
    }

    /// Every process currently registered under the Workbench bundle id,
    /// including instances launched from other worktrees.
    static func runningWorkbenchProcessIdentifiers() -> [pid_t] {
        #if canImport(AppKit)
            return NSRunningApplication.runningApplications(
                withBundleIdentifier: workbenchBundleIdentifier
            ).map(\.processIdentifier)
        #else
            return []
        #endif
    }

    @MainActor
    func dismissCatalogReview(in app: XCUIApplication) {
        let cancel = app.buttons.matching(identifier: "Cancel").firstMatch
        guard cancel.waitForExistence(timeout: 2), cancel.isHittable else {
            return
        }
        cancel.click()
        _ = waitForNonexistence(
            of: app.buttons[
                "catalog-install-exact-build"
            ],
            timeout: 5
        )
    }

    @MainActor
    func waitForNonexistence(
        of element: XCUIElement,
        timeout: TimeInterval
    ) -> Bool {
        let expectation = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"),
            object: element
        )
        return XCTWaiter.wait(
            for: [expectation],
            timeout: timeout
        ) == .completed
    }

    /// Reveals the `permission-decision-<domain>` control in the native
    /// permission review sheet and returns it.
    ///
    /// The permission list is rendered inside a `ScrollView` whose sheet
    /// window is only given an `idealHeight`, not a fixed one — CI's
    /// virtual display can size it down toward its `minHeight` and reveal
    /// far less of the list than a local run does. Two earlier attempts at
    /// simulating swipe gestures from this side of the fence
    /// (`scrollToHittable` below) proved unable to keep up with that: a
    /// single-direction swipe loop can overshoot a target that sits near
    /// the end of the list right off the top of the scroll view's visible
    /// bounds, with no safe way to correct without reintroducing a flicker
    /// bug a prior fix hit when it tried swiping both directions.
    ///
    /// `PermissionReviewSheet` now exposes a deterministic, UI-test-only
    /// hook instead: a `permission-scroll-to-<domain>` button that lives
    /// outside the `ScrollView` (so it is always hittable, regardless of
    /// scroll position) and calls `ScrollViewProxy.scrollTo` directly,
    /// which is exact and cannot overshoot. Prefer that hook when it
    /// exists; fall back to the swipe-based heuristic only for launches
    /// that do not set `NMP_WORKBENCH_UI_TEST_SCENARIO` (and therefore
    /// never render the hook), such as the opt-in live catalog journey.
    @MainActor
    func scrollPermissionDecisionIntoView(
        domain: String,
        in app: XCUIApplication,
        message: String? = nil
    ) -> XCUIElement {
        let decision = app.descendants(matching: .any)[
            "permission-decision-\(domain)"
        ]
        XCTAssertTrue(
            decision.waitForExistence(timeout: 10),
            message ?? "The \(domain) decision must exist in the native review"
        )

        let anchor = app.buttons["permission-scroll-to-\(domain)"]
        if anchor.waitForExistence(timeout: 2) {
            let frameBeforeClick = decision.frame
            anchor.click()
            let settled = waitForStableFrame(decision, timeout: 5)
            if !settled {
                NSLog(
                    "scrollPermissionDecisionIntoView: \(domain) did not "
                        + "settle after clicking the deterministic-scroll "
                        + "anchor. anchorExists=\(anchor.exists) "
                        + "anchorHittable=\(anchor.isHittable) "
                        + "decisionExists=\(decision.exists) "
                        + "decisionHittable=\(decision.isHittable) "
                        + "frameBeforeClick=\(frameBeforeClick) "
                        + "frameAfterClick=\(decision.frame)"
                )
            }
            XCTAssertTrue(
                settled,
                message
                    ?? "The \(domain) decision must settle into view after "
                    + "the deterministic scroll"
            )
        } else {
            XCTAssertTrue(
                scrollToHittable(decision, in: app),
                message ?? "The \(domain) decision must be reachable in the native review"
            )
        }
        return decision
    }

    @MainActor
    func scrollToHittable(
        _ element: XCUIElement,
        in app: XCUIApplication
    ) -> Bool {
        guard !element.isHittable else {
            return true
        }
        // Scope the scroll view lookup to the window that actually contains
        // `element`. A previously dismissed sheet (e.g. the account
        // registration window) can still report its own scroll view to an
        // app-wide, unscoped `app.scrollViews.firstMatch` query for a brief
        // window while it finishes tearing down, even after the specific
        // control we waited on has already left the accessibility tree.
        // Swiping that stale scroll view has no effect on the still-open
        // review sheet and would silently spin without ever revealing
        // `element`, so anchor the search to element's own window instead of
        // relying on window ordering.
        let scope = containingWindow(of: element, in: app) ?? app
        let scrollView = scope.scrollViews.firstMatch
        guard scrollView.waitForExistence(timeout: 2) else {
            return false
        }

        // The permission sheet's window is only given `idealHeight: 720`;
        // its floor is `minHeight: 560`. A fixed swipe count tuned against
        // one developer's local display — where the sheet renders near its
        // ideal size and most of the capability list is visible without
        // scrolling — does not generalize: CI runs headless against its own
        // virtual display, which can size the sheet down toward its minimum
        // height and reveal far less of the list per swipe, so a domain
        // that never needed scrolling locally can need several swipes in
        // CI. Loop with a generous, geometry-independent attempt budget
        // instead of a fixed one tuned to a single screen.
        //
        // Swiping is intentionally one-directional (always up, the
        // direction that reveals later rows). An earlier version of this
        // loop tried to "correct" an overshoot by swiping back down
        // whenever the target briefly scrolled out of the visible area
        // above the scroll view. In practice that made things worse: for a
        // row sitting exactly at the end of the scrollable content (e.g.
        // the last capability in the list), alternating swipe directions
        // could make the row's accessibility element flicker in and out of
        // existence entirely and, eventually, made the scroll view itself
        // stop resolving in the accessibility snapshot
        // ("Failed to get matching snapshot ... ScrollView"), reproduced
        // locally. A single scroll direction does not have that failure
        // mode.
        //
        // "Revealed" requires the target's full frame inside the scroll
        // view's visible bounds (see `isFullyRevealed`), not merely
        // `isHittable`: XCUITest can mark a row hittable a frame or two
        // before it has fully crossed the scroll view's clip boundary,
        // which was enough for the old check to stop scrolling but not
        // enough for a subsequent click to reliably open its popup menu —
        // exactly the residual flakiness a previous pass through this test
        // flagged for the last row in the list.
        //
        // Progress is tracked by the target's vertical offset from the
        // scroll view's center. Once swiping stops moving that offset
        // (the scroll view has reached the end of its content — the
        // saturation point), further swipes cannot help, so stop and
        // report a diagnostic instead of silently spinning to the attempt
        // ceiling.
        let maxAttempts = 20
        var consecutiveStalls = 0
        var previousOffset: CGFloat?

        for attempt in 0 ..< maxAttempts {
            scrollView.swipeUp()
            usleep(250_000)

            if isFullyRevealed(element, in: scrollView),
                waitForStableFrame(element, timeout: 2)
            {
                return true
            }

            let scrollFrame = scrollView.frame
            let elementFrame = element.frame
            let offset = abs(elementFrame.midY - scrollFrame.midY)
            if let previousOffset, abs(offset - previousOffset) < 1 {
                consecutiveStalls += 1
            } else {
                consecutiveStalls = 0
            }
            previousOffset = offset

            if consecutiveStalls >= 3 {
                NSLog(
                    "scrollToHittable: giving up on "
                        + "\(element.identifier) after \(attempt + 1) "
                        + "attempt(s) — the scroll view stopped moving it "
                        + "any further (reached the end of its content) "
                        + "without fully revealing it. isHittable="
                        + "\(element.isHittable) scrollView=\(scrollFrame) "
                        + "element=\(elementFrame)"
                )
                return false
            }
        }

        NSLog(
            "scrollToHittable: exhausted \(maxAttempts) attempts revealing "
                + "\(element.identifier). isHittable=\(element.isHittable) "
                + "Last scrollView=\(scrollView.frame) "
                + "element=\(element.frame)"
        )
        return false
    }

    /// Whether `element`'s full frame sits inside `scrollView`'s visible
    /// bounds, not merely at its edge. XCUITest can mark a row `isHittable`
    /// a frame or two before it has fully crossed the scroll view's clip
    /// boundary — enough to stop scrolling but not enough for a subsequent
    /// click to land reliably on it.
    @MainActor
    func isFullyRevealed(
        _ element: XCUIElement,
        in scrollView: XCUIElement
    ) -> Bool {
        guard element.exists, element.isHittable else {
            return false
        }
        let margin: CGFloat = 2
        let visibleBounds = scrollView.frame.insetBy(dx: 0, dy: margin)
        return visibleBounds.contains(element.frame)
    }

    /// Clicks `decision` to open its native popup menu and waits for `allow`
    /// to appear inside it, retrying the click a bounded number of times.
    ///
    /// A click delivered immediately after `scrollToHittable` scrolls a row
    /// into view can land on a control whose popup-menu tracking session
    /// never stabilizes in the accessibility tree — the row was hittable,
    /// but only just, right at the edge of the scroll view's clip bounds.
    /// The first click is silently swallowed rather than opening a menu.
    /// Retrying the click (instead of only waiting longer for a menu that
    /// was never opened) recovers deterministically without weakening what
    /// this is actually asserting: that the runtime offers this decision.
    ///
    /// The retry alone only covers the swallowed-click mode. A menu that
    /// merely opened *slowly* — under a machine sharing its single desktop
    /// session with concurrent builds, an NSMenu tracking session can take
    /// longer than the old 2s budget to publish its items into the
    /// accessibility tree — is still an **open** menu, and a second click on
    /// the same popup control toggles it shut. That turned one slow menu
    /// into three failed attempts, which is a decision-menu failure of
    /// exactly the shape issue #137 reports.
    ///
    /// Both halves of the fix are here because neither is sufficient alone:
    ///
    /// * The re-click is now guarded on no menu being open. That is the
    ///   direct fix, but it can only see a half-open menu that has already
    ///   reached the accessibility tree as a `menus` element.
    /// * The final attempt therefore also waits the suite's standard 10s, so
    ///   a menu still entirely invisible to that guard gets a budget long
    ///   enough to finish opening, with no further click behind it to close
    ///   it again.
    ///
    /// The first attempt keeps the short 2s budget: a swallowed click must
    /// still be retried promptly, and that is the common case.
    @MainActor
    func openDecisionMenu(
        _ decision: XCUIElement,
        revealing allow: XCUIElement,
        in app: XCUIApplication
    ) -> Bool {
        let attempts = 3
        for attempt in 0 ..< attempts {
            if attempt == 0 || !app.menus.firstMatch.exists {
                decision.click()
            }
            let isFinalAttempt = attempt == attempts - 1
            if allow.waitForExistence(timeout: isFinalAttempt ? 10 : 2) {
                return true
            }
        }
        return false
    }

    /// `swipeUp()` requests a fast (flinged) scroll, which hands off to
    /// AppKit's own momentum/deceleration animation. That animation runs on
    /// the window server, not the app's run loop, so XCUITest's automatic
    /// "wait for app to idle" step completes before the scrolled content has
    /// actually come to rest. Interacting with a menu-style control while its
    /// row is still drifting underneath the pointer can open a popup menu
    /// that never stabilizes in the accessibility tree before it is
    /// dismissed by the continuing scroll. Waiting for the element's frame
    /// to be identical across two consecutive samples confirms the scroll
    /// has actually settled before the caller clicks it.
    @MainActor
    func waitForStableFrame(
        _ element: XCUIElement,
        timeout: TimeInterval
    ) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        var previousFrame: CGRect?
        while Date() < deadline {
            guard element.isHittable else {
                previousFrame = nil
                usleep(100_000)
                continue
            }
            let currentFrame = element.frame
            if let previousFrame, previousFrame == currentFrame {
                return true
            }
            previousFrame = currentFrame
            usleep(150_000)
        }
        return false
    }

    @MainActor
    func containingWindow(
        of element: XCUIElement,
        in app: XCUIApplication
    ) -> XCUIElement? {
        let identifier = element.identifier
        guard !identifier.isEmpty else {
            return nil
        }
        return app.windows.allElementsBoundByIndex.first { window in
            window.descendants(matching: .any)[identifier].exists
        }
    }

    /// Every presence wait in this flow uses the suite's standard 10s
    /// allowance, deliberately and uniformly.
    ///
    /// Each step here waits on something that only materializes after a
    /// window-server transition — an NSMenu popup, a sheet presentation, a
    /// sheet dismissal — none of which are driven by the app's own run loop,
    /// so XCTest's automatic "wait for app to idle" step cannot cover them.
    /// When a second process steals frontmost (concurrent `xcodebuild` runs
    /// on the same machine are the common case), those transitions are
    /// exactly what stalls, and the popup-menu step stalls first. Three of
    /// these waits were previously written as bare `timeout: 2` asserts,
    /// which read as "this should be instant" rather than as a tuned budget;
    /// the menu wait below then failed repeatedly under concurrent runs while
    /// passing in isolation. A longer allowance costs nothing when the
    /// element appears on time, so there is no reason for any step in this
    /// sequence to be the short one.
    @MainActor
    func registerAndActivateDeterministicAccount(
        in app: XCUIApplication
    ) {
        let accountSwitcher = app.descendants(matching: .any)[
            "account-switcher"
        ]
        XCTAssertTrue(
            accountSwitcher.waitForExistence(timeout: 10),
            "The account switcher must be the first toolbar control"
        )
        accountSwitcher.click()

        let addSigner = app.menuItems["Signer-backed Account…"]
        XCTAssertTrue(
            addSigner.waitForExistence(timeout: 10),
            "The account switcher menu must offer signer-backed registration"
        )
        addSigner.click()

        let secretField = app.secureTextFields["Secret key"]
        XCTAssertTrue(secretField.waitForExistence(timeout: 10))
        secretField.click()
        secretField.typeText(Self.uiTestSigningSecret)

        let register = app.buttons["Register Local Account"]
        XCTAssertTrue(
            register.waitForExistence(timeout: 10),
            "The registration sheet must offer a register control"
        )
        XCTAssertTrue(register.isEnabled)
        register.click()

        let activate = app.buttons[
            "Activate \(Self.uiTestSigningPublicKey)"
        ]
        XCTAssertTrue(
            activate.waitForExistence(timeout: 10),
            "Registration must project the deterministic signer without activating it"
        )
        XCTAssertTrue(
            scrollToHittable(activate, in: app),
            "The newly registered signer must be reachable in the account sheet"
        )
        activate.click()

        let activePublicKey = app.staticTexts[
            "Active account hexadecimal public key"
        ]
        XCTAssertTrue(
            activePublicKey.waitForExistence(timeout: 10),
            "Activation must project the selected public account identity"
        )
        XCTAssertEqual(
            activePublicKey.value as? String,
            Self.uiTestSigningPublicKey
        )

        let done = app.buttons["Done"]
        XCTAssertTrue(
            done.waitForExistence(timeout: 10),
            "The account sheet must offer a dismissal control"
        )
        done.click()
        XCTAssertTrue(
            waitForNonexistence(of: activePublicKey, timeout: 10),
            "The account sheet must close before permission review resumes"
        )
    }
}
