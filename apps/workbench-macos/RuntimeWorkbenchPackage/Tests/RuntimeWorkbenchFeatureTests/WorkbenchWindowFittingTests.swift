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

/// The permission sheet's action row is the reason this matters: it is a fixed
/// footer at the bottom of the sheet, so whatever the window's bottom edge
/// cannot reach, "Not Now" cannot either.
@Test func thePermissionSheetActionRowClearsTheDockOnceTheWindowFits() {
    let fitted = WorkbenchWindowFitting.fitted(
        desiredWindow,
        into: CIDisplay.visibleFrame
    )
    // The sheet is inset below the window's title bar and toolbar; the
    // observed inset was 52pt (window top 25 -> sheet top 77).
    let toolbarInset: CGFloat = 52
    let sheetHeight = min(700, fitted.height - toolbarInset)

    // `PermissionReviewSheet` refuses to render shorter than this, so a fit
    // that violated it would trade one defect for another.
    #expect(sheetHeight >= 520)

    let windowTop = CIDisplay.screenHeight - fitted.maxY
    let sheetBottomFromTop = windowTop + toolbarInset + sheetHeight
    #expect(sheetBottomFromTop <= CIDisplay.dockTopFromTop)
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
