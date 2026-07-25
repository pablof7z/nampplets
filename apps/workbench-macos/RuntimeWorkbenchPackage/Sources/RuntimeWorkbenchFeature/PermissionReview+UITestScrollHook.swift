import Foundation
import SwiftUI

extension PermissionReviewSheet {
    /// Whether the process is running under an XCUITest scenario launch.
    /// UI tests set `NMP_WORKBENCH_UI_TEST_SCENARIO` (see
    /// `RuntimeWorkbenchApp.swift` and `ContentView.swift`, which gate their
    /// own fixture-loading behind the same variable) before the app is
    /// launched, so its mere presence — regardless of which scenario string
    /// it carries — is a reliable, test-only signal. It is never set for a
    /// user-facing launch.
    var isUITestScrollHookEnabled: Bool {
        ProcessInfo.processInfo.environment["NMP_WORKBENCH_UI_TEST_SCENARIO"]
            != nil
    }

    /// A UI-test-only row of near-invisible buttons, one per capability
    /// domain, that jump the matching capability card straight to the
    /// center of the scroll view via `ScrollViewProxy.scrollTo`.
    ///
    /// This exists because swipe-gesture-based scrolling from the XCUITest
    /// side (`scrollToHittable` in `RuntimeWorkbenchUITests.swift`) proved
    /// fundamentally unreliable against CI's virtual display: the sheet's
    /// window can size down toward its `minHeight`, changing how much of
    /// the list a single swipe reveals, and a swipe that overshoots the
    /// target can push it entirely outside the scroll view's bounds with
    /// no reliable way to correct without reintroducing a flicker bug a
    /// prior fix already hit with bidirectional swiping.
    ///
    /// `proxy.scrollTo` is exact and immediate: it needs no swipe-distance
    /// tuning, cannot overshoot, and (called outside a `withAnimation`
    /// block, as here) is not subject to AppKit's momentum/deceleration
    /// animation running out from under XCUITest's "wait for idle" step.
    /// The row lives outside the `ScrollView` so it is always present and
    /// hittable regardless of scroll position, and it is gated behind the
    /// same `NMP_WORKBENCH_UI_TEST_SCENARIO` launch-environment signal
    /// already used to gate fixture loading, so it never exists in a
    /// user-facing build. It only moves a row into view — the UI test
    /// still performs the real click on the real, fully revealed control
    /// to prove the actual interaction works.
    func scrollAnchorRow(proxy: ScrollViewProxy) -> some View {
        HStack(spacing: 0) {
            ForEach(model.review.capabilities) { capability in
                Button {
                    proxy.scrollTo(capability.domain, anchor: .center)
                } label: {
                    // A fully transparent `Color.clear` fill (and, before
                    // this, wrapping the whole row in `.opacity(0.01)`)
                    // left this button visually present in the
                    // accessibility tree but not reliably receiving
                    // AppKit's mouse hit-testing: CI saw
                    // `permission-scroll-to-<domain>` report existing and
                    // accept `.click()` without `proxy.scrollTo` ever
                    // having any observable effect (the target row's
                    // frame never changed). `.contentShape` pins the
                    // hit-testing region explicitly regardless of the
                    // fill's alpha, and a non-zero (if minuscule) alpha
                    // avoids relying on an exact-zero value some AppKit
                    // layers treat as "not interactive."
                    Color.white.opacity(0.001)
                        .frame(width: 8, height: 8)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier(
                    "permission-scroll-to-\(capability.domain)"
                )
                .accessibilityLabel(
                    "Scroll \(capability.domain) into view"
                )
            }
        }
        .frame(height: 8)
    }
}
