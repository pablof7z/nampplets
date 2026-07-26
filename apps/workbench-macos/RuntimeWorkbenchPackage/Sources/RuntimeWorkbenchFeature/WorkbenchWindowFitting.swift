import CoreGraphics

/// Keeps the Workbench window inside the part of the screen a person can
/// actually reach.
///
/// The window asks for 1180x780. On a display shorter than that -- CI's
/// virtual Mac is 1024x768, and small laptops are not far off -- AppKit will
/// happily place a window that extends underneath the Dock. Nothing clips or
/// warns; the bottom of the app is simply covered by another process's window.
///
/// That is not cosmetic. The permission review's action row sits at the bottom
/// of its sheet, so on a short display "Not Now" and the confirm button render
/// beneath the Dock. They are drawn, they report real frames, and they cannot
/// be clicked -- a consent dialog the user cannot decline. The Workbench's own
/// status bar lands there too.
///
/// `NSScreen.visibleFrame` is exactly the rectangle that excludes the menu bar
/// and the Dock, so fitting to it is the whole fix. This is a no-op on any
/// display large enough for the ideal size, which is every developer machine
/// the app has been looked at on -- which is precisely why it went unnoticed.
public enum WorkbenchWindowFitting {
    /// The largest version of `desired` that fits entirely within
    /// `visibleFrame`, moved into range if it would otherwise hang off an edge.
    ///
    /// Both rectangles use AppKit's bottom-left origin, so clamping the origin
    /// up to `visibleFrame.minY` is what lifts the window off the Dock.
    public static func fitted(
        _ desired: CGRect,
        into visibleFrame: CGRect
    ) -> CGRect {
        guard visibleFrame.width > 0, visibleFrame.height > 0 else {
            return desired
        }
        let width = min(desired.width, visibleFrame.width)
        let height = min(desired.height, visibleFrame.height)
        // `max(_:_:)` on the lower bound first: when the window is exactly as
        // large as the visible frame, both clamps agree and the origin lands
        // on the visible frame's own origin.
        let x = min(
            max(desired.minX, visibleFrame.minX),
            visibleFrame.maxX - width
        )
        let y = min(
            max(desired.minY, visibleFrame.minY),
            visibleFrame.maxY - height
        )
        return CGRect(x: x, y: y, width: width, height: height)
    }

    /// Whether `frame` already sits entirely inside `visibleFrame`, so callers
    /// can skip a redundant (and visible) window move on normal displays.
    public static func fits(_ frame: CGRect, in visibleFrame: CGRect) -> Bool {
        visibleFrame.contains(frame)
    }

    /// The tallest a sheet may be before its footer runs under the Dock.
    ///
    /// Fitting the parent window is necessary but **not** sufficient, which the
    /// measured CI geometry shows outright. A macOS sheet is its own window: it
    /// is anchored just below the parent's title bar, but AppKit caps its height
    /// against the screen rather than against the parent's content area. On the
    /// 1024x768 runner the sheet came out `672` tall -- exactly
    /// `visibleFrame.height` -- hung from a parent whose top edge was at `25`:
    ///
    ///     25 (parent top) + 52 (chrome) + 672 (sheet) = 749
    ///
    /// which is 53pt below the Dock's top edge at `696`. Fitting the parent
    /// moves its top edge from `25` to `24`, so the sheet's bottom moves to
    /// `748` -- one single point. That is why the button frame did not budge.
    ///
    /// The sheet has to give up the height instead. It already scrolls, and its
    /// own minimum is `520`, so there is real slack between what it asks for and
    /// what it needs; this hands back only the excess and never goes below
    /// `minimum`.
    public static func maxSheetHeight(
        visibleHeight: CGFloat,
        chromeInset: CGFloat,
        minimum: CGFloat
    ) -> CGFloat {
        guard visibleHeight > 0 else { return minimum }
        return max(minimum, visibleHeight - chromeInset)
    }
}

#if canImport(AppKit)
    import AppKit

    /// Resolves the sheet's height ceiling against the screen it will appear on.
    ///
    /// Kept beside the pure geometry rather than inside the view so the numbers
    /// stay testable; only this lookup needs AppKit.
    public enum PermissionReviewSheetGeometry {
        /// Title bar plus toolbar above the sheet's top edge, measured from the
        /// CI accessibility dump: parent top `25` -> sheet top `77`.
        public static let chromeInset: CGFloat = 52
        /// Matches the `minHeight` the sheet declares.
        public static let minimumHeight: CGFloat = 520

        public static var maxHeight: CGFloat {
            WorkbenchWindowFitting.maxSheetHeight(
                visibleHeight: NSScreen.main?.visibleFrame.height ?? 0,
                chromeInset: chromeInset,
                minimum: minimumHeight
            )
        }
    }
#endif

/// The Workbench's own content sizing.
///
/// The ideal numbers are the ones the app was designed around and are what a
/// roomy display still gets. The minimums exist so the window can be made to
/// fit a small screen at all -- they are a floor on usability, not a target.
public enum WorkbenchContentSizing {
    public static let idealWidth: CGFloat = 1_050
    public static let idealHeight: CGFloat = 660
    /// Chosen so the window fits within a 1024pt-wide display, the narrowest
    /// Mac screen the app is expected to run on and the one CI uses.
    public static let minimumWidth: CGFloat = 820
    /// Leaves room below a 1024x768 screen's menu bar and Dock: the usable
    /// strip there is 672pt, less 52pt of window chrome.
    public static let minimumHeight: CGFloat = 560
}
