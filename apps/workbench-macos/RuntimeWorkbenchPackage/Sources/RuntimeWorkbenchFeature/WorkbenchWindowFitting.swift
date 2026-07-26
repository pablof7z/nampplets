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
}
