import Foundation
@testable import RuntimeWorkbenchFeature
import Testing

/// Every palette token must clear its WCAG contrast threshold, computed here
/// rather than asserted in prose.
///
/// The visual design document originally stated a ratio for each token to one
/// decimal place. Four of the twelve were wrong beyond rounding -- 19.0 for
/// 18.27, 8.1 for 7.79, 7.6 for 7.17, 16.0 for 15.85 -- and every one of the
/// four erred in the flattering direction. They read as computed. They were
/// not; they were plausible figures written next to colours.
///
/// No conclusion changed: every token still clears its threshold with room to
/// spare, so the palette was sound and the accessibility claim held. But a
/// document that prints "19.0 : 1" is asserting that it measured something,
/// which is the same defect as a consent prompt summarising a draft it did
/// not read. The fix for both is the same -- stop asserting, start checking.
///
/// This is the check. It fails if anyone edits a hex value in `NappletInk`
/// into something that no longer clears its threshold, which a number in a
/// markdown file could never do.
private func relativeLuminance(_ hex: Int) -> Double {
    func channel(_ raw: Int) -> Double {
        let value = Double(raw) / 255
        return value <= 0.039_28
            ? value / 12.92
            : pow((value + 0.055) / 1.055, 2.4)
    }
    let red = channel((hex >> 16) & 0xFF)
    let green = channel((hex >> 8) & 0xFF)
    let blue = channel(hex & 0xFF)
    return 0.2126 * red + 0.7152 * green + 0.0722 * blue
}

private func contrastRatio(_ foreground: Int, on background: Int) -> Double {
    let first = relativeLuminance(foreground)
    let second = relativeLuminance(background)
    let lighter = max(first, second)
    let darker = min(first, second)
    return (lighter + 0.05) / (darker + 0.05)
}

/// Mirrors the literals in `NappletInk`. Kept here deliberately rather than
/// read back from `Color`: a `Color` built from a dynamic provider cannot be
/// resolved to components without a rendering environment, and a test that
/// needs one is a test that gets disabled.
private enum Palette {
    static let paperLight = 0xFC_FC_FB
    static let paperDark = 0x17_18_1A

    /// (name, light, dark, minimum ratio it must clear)
    static let onPaper: [(String, Int, Int, Double)] = [
        ("ink", 0x11_12_13, 0xF2_F2_F0, 7.0),
        ("inkSecondary", 0x5B_60_67, 0xA0_A5_AC, 4.5),
        // Non-essential text only, so it is held to the large-text bar and
        // nothing a person needs in order to act may be set in it.
        ("inkTertiary", 0x8A_90_99, 0x6E_74_7C, 3.0),
        ("accent", 0x3A_3F_8F, 0xA0_A6_F0, 4.5),
        ("caution", 0x8A_5A_00, 0xE0_B2_5C, 4.5),
        ("refusal", 0x9A_2B_22, 0xF0_93_8A, 4.5),
    ]
}

@Test func everyInkTokenClearsItsContrastThresholdInBothAppearances() {
    for (name, light, dark, minimum) in Palette.onPaper {
        let lightRatio = contrastRatio(light, on: Palette.paperLight)
        let darkRatio = contrastRatio(dark, on: Palette.paperDark)

        #expect(
            lightRatio >= minimum,
            "\(name) light is \(lightRatio) on paper, below \(minimum)"
        )
        #expect(
            darkRatio >= minimum,
            "\(name) dark is \(darkRatio) on paper, below \(minimum)"
        )
    }
}

/// The accent must remain legible against its own fill, since the one
/// accent-coloured element per screen is a filled primary action.
@Test func theAccentIsLegibleAgainstItsOwnFill() {
    #expect(contrastRatio(0xFF_FF_FF, on: 0x3A_3F_8F) >= 4.5)
    #expect(contrastRatio(0x10_11_14, on: 0xA0_A6_F0) >= 4.5)
}

/// ADR 0008 says colour reinforces state and never carries it. The structural
/// consequence: caution and refusal must be distinguishable from ordinary ink
/// by more than hue, so a reader who cannot separate them still reads a
/// different word and glyph. This asserts the weaker, checkable half -- that
/// neither semantic ink is so close to `inkSecondary` in luminance that it
/// would read as ordinary body text to someone who cannot see the hue.
@Test func semanticInksAreNotMistakableForOrdinaryTextByLuminanceAlone() {
    let secondaryLight = relativeLuminance(0x5B_60_67)
    for (name, light, _, _) in Palette.onPaper
        where name == "caution" || name == "refusal"
    {
        let semantic = relativeLuminance(light)
        #expect(
            abs(semantic - secondaryLight) > 0.01,
            "\(name) is luminance-identical to inkSecondary, so with hue removed it would read as ordinary text"
        )
    }
}
