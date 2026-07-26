import SwiftUI

#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// The palette.
///
/// Colour is permitted in exactly three places: the accent, on one element per
/// screen; the caution and refusal inks, only when Rust projected a
/// `NappletTrustVerdict` that warrants them; and a person's own profile
/// picture, which is their colour and not ours. Everything else -- every
/// state, category, status, tier and count -- is carried by words, size,
/// position and structure.
///
/// The ground is a warm off-white rather than `#FFFFFF`, and names are set in
/// a serif. Those two moves are the difference between "restrained" and
/// "unstyled": grey type on device white is what default SwiftUI already is.
///
/// See `docs/design/napplet-browser-visual.md` and
/// `docs/adr/0008-verdicts-on-the-path.md`.
public enum NappletInk {
    // Ink ramp.
    public static let ink = dynamic(light: 0x111213, dark: 0xF2F2F0)
    public static let inkSecondary = dynamic(light: 0x5B6067, dark: 0xA0A5AC)
    /// Non-essential text only. It does not meet body contrast by design, so
    /// nothing a person needs in order to act may be set in it.
    public static let inkTertiary = dynamic(light: 0x8A9099, dark: 0x6E747C)

    // Ground.
    public static let paper = dynamic(light: 0xFCFCFB, dark: 0x17181A)
    public static let paperRaised = dynamic(light: 0xFFFFFF, dark: 0x1F2124)
    public static let fillQuiet = dynamic(light: 0xF2F1EE, dark: 0x212326)
    public static let fillSelected = dynamic(light: 0xE8E7E3, dark: 0x2A2D31)
    public static let rule = dynamic(light: 0xE4E3E0, dark: 0x2E3134)

    /// Indigo by elimination, not by preference: green is the hue being
    /// retired with the seal, amber is caution, red is refusal, and system
    /// blue on macOS is indistinguishable from having made no choice. Indigo
    /// is the only hue left carrying no semantic, and desaturated it reads as
    /// ink from a good pen -- the right association for a product about
    /// signatures.
    public static let accent = dynamic(light: 0x3A3F8F, dark: 0xA0A6F0)
    public static let onAccent = dynamic(light: 0xFFFFFF, dark: 0x101114)

    // Semantic. Rare by construction: reachable only from a projected verdict.
    public static let caution = dynamic(light: 0x8A5A00, dark: 0xE0B25C)
    public static let refusal = dynamic(light: 0x9A2B22, dark: 0xF0938A)

    static func ground(for verdict: NappletTrustVerdict) -> Color {
        switch verdict {
        case .settled: .clear
        case .caution: caution.opacity(0.10)
        case .blocked: refusal.opacity(0.10)
        }
    }

    static func tint(for verdict: NappletTrustVerdict) -> Color {
        switch verdict {
        case .settled: .clear
        case .caution: caution
        case .blocked: refusal
        }
    }

    private static func dynamic(light: Int, dark: Int) -> Color {
        #if os(macOS)
        Color(
            nsColor: NSColor(name: nil) { appearance in
                appearance.bestMatch(
                    from: [.darkAqua, .aqua]
                ) == .darkAqua
                    ? NSColor(rgb: dark)
                    : NSColor(rgb: light)
            }
        )
        #else
        Color(
            uiColor: UIColor { traits in
                traits.userInterfaceStyle == .dark
                    ? UIColor(rgb: dark)
                    : UIColor(rgb: light)
            }
        )
        #endif
    }
}

#if os(macOS)
private extension NSColor {
    convenience init(rgb: Int) {
        self.init(
            srgbRed: Double((rgb >> 16) & 0xFF) / 255,
            green: Double((rgb >> 8) & 0xFF) / 255,
            blue: Double(rgb & 0xFF) / 255,
            alpha: 1
        )
    }
}
#else
private extension UIColor {
    convenience init(rgb: Int) {
        self.init(
            red: Double((rgb >> 16) & 0xFF) / 255,
            green: Double((rgb >> 8) & 0xFF) / 255,
            blue: Double(rgb & 0xFF) / 255,
            alpha: 1
        )
    }
}
#endif
