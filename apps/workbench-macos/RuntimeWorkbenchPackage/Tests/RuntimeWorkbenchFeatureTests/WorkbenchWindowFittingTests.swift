import CoreGraphics
import Testing

@testable import RuntimeWorkbenchFeature

/// The display the Apple job actually runs on, measured from the accessibility
/// dump and screen recording of run 30206983443:
///
/// * screen `1024x768` (menu bar reports `{{0,0},{1024,24}}`)
/// * Dock top edge at `y=696` in top-left coordinates -- at `x=548`, the
///   centre of the "Not Now" button, the pixels from `y=720` to `y=727` are a
///   yellow Dock icon, not the button
///
/// so the usable strip below the menu bar is `768 - 24 - 72 = 672` points tall.
/// In AppKit's bottom-left coordinates that is `{{0, 72}, {1024, 672}}`.
private enum CIDisplay {
    static let visibleFrame = CGRect(x: 0, y: 72, width: 1024, height: 672)
    /// Top-left y of the Dock's top edge, for readability in assertions.
    static let dockTopFromTop: CGFloat = 696
    static let screenHeight: CGFloat = 768

    /// Converts an AppKit bottom-left frame to the top-left y of its bottom
    /// edge, which is the coordinate space the XCUITest failure reports in.
    static func bottomEdgeFromTop(of frame: CGRect) -> CGFloat {
        screenHeight - frame.minY
    }
}

/// What the window actually asks for.
private let desiredWindow = CGRect(x: 0, y: 0, width: 1180, height: 780)

/// The window AppKit actually produced on that runner, read straight off the
/// accessibility dump: `{{0, 25}, {1050, 712}}` in top-left coordinates.
private let observedUnfittedWindow = CGRect(
    x: 0,
    y: CIDisplay.screenHeight - 737,
    width: 1050,
    height: 712
)

@Test func theWorkbenchWindowIsLiftedOffTheDockOnAShortDisplay() {
    // Negative control. If this assertion ever stops holding, the rest of this
    // file is measuring nothing: it says the frame CI actually observed really
    // does run under the Dock, so the property below can distinguish the fix
    // from the defect.
    #expect(
        CIDisplay.bottomEdgeFromTop(of: observedUnfittedWindow)
            > CIDisplay.dockTopFromTop
    )
    #expect(!CIDisplay.visibleFrame.contains(observedUnfittedWindow))

    let fitted = WorkbenchWindowFitting.fitted(
        desiredWindow,
        into: CIDisplay.visibleFrame
    )

    // The whole point: the window's bottom edge must clear the Dock. Before
    // this fix the observed window was {{0,25},{1050,712}} in top-left
    // coordinates -- a bottom edge at y=737, i.e. 41pt underneath a Dock whose
    // top edge is at y=696.
    #expect(
        CIDisplay.bottomEdgeFromTop(of: fitted) <= CIDisplay.dockTopFromTop
    )
    #expect(CIDisplay.visibleFrame.contains(fitted))
    // It must also stop overhanging the right edge; the observed window was
    // 1050 wide on a 1024-wide screen.
    #expect(fitted.width <= CIDisplay.visibleFrame.width)
}

private let chromeInset: CGFloat = 52
private let sheetMinimum: CGFloat = 520

/// Bottom edge, in top-left coordinates, of a sheet hung from a parent whose
/// top edge is at `parentTopFromTop`. A macOS sheet is its own window anchored
/// below the parent's chrome, which is the whole reason this arithmetic matters.
private func sheetBottomFromTop(
    parentTopFromTop: CGFloat,
    sheetHeight: CGFloat
) -> CGFloat {
    parentTopFromTop + chromeInset + sheetHeight
}

/// The finding that cost the first attempt, pinned so it cannot be forgotten:
/// fitting the parent window is necessary and **not** sufficient.
///
/// AppKit capped the sheet at `672` -- exactly `visibleFrame.height` -- and hung
/// it below the parent's chrome. Moving the parent's top edge from `25` to `24`
/// therefore moves the sheet's bottom from `749` to `748`. One point. That is
/// precisely why the observed button frame was byte-identical after the first
/// fix, and why the sheet needs a ceiling of its own.
@Test func fittingTheParentWindowAloneDoesNotLiftTheSheetOffTheDock() {
    let unhelped = sheetBottomFromTop(parentTopFromTop: 25, sheetHeight: 672)
    #expect(unhelped == 749)

    let fitted = WorkbenchWindowFitting.fitted(
        desiredWindow,
        into: CIDisplay.visibleFrame
    )
    let parentTop = CIDisplay.screenHeight - fitted.maxY
    #expect(parentTop == 24)

    // The parent now fits perfectly...
    #expect(CIDisplay.visibleFrame.contains(fitted))
    // ...and the sheet is still under the Dock, by all but one point.
    let stillWrong = sheetBottomFromTop(
        parentTopFromTop: parentTop,
        sheetHeight: 672
    )
    #expect(stillWrong == 748)
    #expect(stillWrong > CIDisplay.dockTopFromTop)
}

/// The permission sheet's action row is the reason this matters: it is a fixed
/// footer at the bottom of the sheet, so whatever the sheet's bottom edge
/// cannot reach, "Not Now" cannot either.
@Test func thePermissionSheetActionRowClearsTheDockOnceTheSheetIsBounded() {
    let fitted = WorkbenchWindowFitting.fitted(
        desiredWindow,
        into: CIDisplay.visibleFrame
    )
    let sheetHeight = WorkbenchWindowFitting.maxSheetHeight(
        visibleHeight: CIDisplay.visibleFrame.height,
        chromeInset: chromeInset,
        minimum: sheetMinimum
    )
    #expect(sheetHeight == 620)

    // `PermissionReviewSheet` refuses to render shorter than this, so a ceiling
    // that violated it would trade one defect for another. 620 > 520, so the
    // sheet gives up only slack it never needed.
    #expect(sheetHeight >= sheetMinimum)

    let parentTop = CIDisplay.screenHeight - fitted.maxY
    #expect(
        sheetBottomFromTop(parentTopFromTop: parentTop, sheetHeight: sheetHeight)
            <= CIDisplay.dockTopFromTop
    )
}

/// The ceiling must never be the thing that makes the sheet unusable.
@Test func theSheetCeilingNeverFallsBelowItsDeclaredMinimum() {
    // A display so short that honouring the Dock outright is impossible.
    let ceiling = WorkbenchWindowFitting.maxSheetHeight(
        visibleHeight: 300,
        chromeInset: chromeInset,
        minimum: sheetMinimum
    )
    #expect(ceiling == sheetMinimum)
    // And an unknown screen (height 0) must not collapse the sheet to nothing.
    #expect(
        WorkbenchWindowFitting.maxSheetHeight(
            visibleHeight: 0,
            chromeInset: chromeInset,
            minimum: sheetMinimum
        ) == sheetMinimum
    )
}

/// On a roomy display the ceiling must sit above the sheet's ideal height, so
/// nothing about the sheet changes on the machines it already worked on.
@Test func theSheetCeilingIsInertOnADisplayWithRoom() {
    let ceiling = WorkbenchWindowFitting.maxSheetHeight(
        visibleHeight: 2100,
        chromeInset: chromeInset,
        minimum: sheetMinimum
    )
    #expect(ceiling > 700)
}

/// Every developer machine this app has been looked at on is large enough,
/// which is exactly why the defect survived. The fix must not move those
/// windows at all.
@Test func aDisplayLargeEnoughForTheIdealSizeIsLeftAlone() {
    let roomy = CGRect(x: 0, y: 0, width: 3456, height: 2100)
    #expect(WorkbenchWindowFitting.fits(desiredWindow, in: roomy))
    #expect(
        WorkbenchWindowFitting.fitted(desiredWindow, into: roomy)
            == desiredWindow
    )
}

@Test func aWindowHangingOffAnEdgeIsMovedBackRatherThanShrunk() {
    let visible = CGRect(x: 0, y: 72, width: 1024, height: 672)
    let offRight = CGRect(x: 900, y: 400, width: 400, height: 300)

    let fitted = WorkbenchWindowFitting.fitted(offRight, into: visible)

    #expect(fitted.size == offRight.size)
    #expect(visible.contains(fitted))
}

@Test func aDegenerateVisibleFrameLeavesTheWindowUntouched() {
    // `NSScreen.visibleFrame` should never be empty, but a zero rect must not
    // collapse the window to nothing if it ever is.
    #expect(
        WorkbenchWindowFitting.fitted(desiredWindow, into: .zero)
            == desiredWindow
    )
}

/// The root cause, pinned. SwiftUI promotes a root view's `minWidth`/
/// `minHeight` to the window's minimum size, and the old values were
/// `1050 x 660`. On CI's 1024x768 display that is a window at least 1050 wide
/// and `660 + 52 = 712` tall -- exactly the frame measured -- which no
/// `setFrame` can shrink. A minimum the display cannot satisfy outranks every
/// other placement fix, which is why the first attempt moved nothing.
@Test func theWindowMinimumSizeMustFitTheSmallestSupportedDisplay() {
    let chrome = chromeInset
    let minWindowHeight = WorkbenchContentSizing.minimumHeight + chrome

    #expect(WorkbenchContentSizing.minimumWidth <= CIDisplay.visibleFrame.width)
    #expect(minWindowHeight <= CIDisplay.visibleFrame.height)

    // And the old values must NOT have fit -- the negative control that shows
    // this assertion discriminates rather than merely agreeing with the code.
    #expect(1_050 > CIDisplay.visibleFrame.width)
    #expect(660 + chrome > CIDisplay.visibleFrame.height)
}

/// A window at the minimum content size must be placeable entirely within the
/// reachable strip, or the Dock overlap simply returns.
@Test func aWindowAtTheMinimumContentSizeFitsEntirelyOnTheCIDisplay() {
    let smallest = CGRect(
        x: 0,
        y: 0,
        width: WorkbenchContentSizing.minimumWidth,
        height: WorkbenchContentSizing.minimumHeight + chromeInset
    )
    let fitted = WorkbenchWindowFitting.fitted(
        smallest,
        into: CIDisplay.visibleFrame
    )
    #expect(fitted.size == smallest.size)
    #expect(CIDisplay.visibleFrame.contains(fitted))
    #expect(CIDisplay.bottomEdgeFromTop(of: fitted) <= CIDisplay.dockTopFromTop)
}
