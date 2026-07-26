import SwiftUI

/// How much of the runtime's own vocabulary a surface is allowed to render.
///
/// See `docs/adr/0008-verdicts-on-the-path.md`. A view never decides its own
/// tier from context; it reads `\.nappletDisclosure` from the environment. That
/// keeps "what may an ordinary person see" a single reviewable decision rather
/// than one judgement call per view.
public enum NappletDisclosure: String, CaseIterable, Hashable, Sendable {
    /// The default everywhere, including at the root of every sheet.
    ///
    /// A `.plain` surface must not render an aggregate hash, an event id, a
    /// public key in any encoding, a `dTag`, a manifest coordinate, a raw
    /// capability domain, a relay URL, a revision number, a NIP number, or a
    /// session identifier -- and must not use the words *principal*,
    /// *manifest*, *aggregate*, *projection*, *coordinate*, *facade*, or
    /// *exact build*. Accessibility text is bound by the same rule: a sheet
    /// that reads a hash to VoiceOver has only hidden the defect from sighted
    /// users.
    case plain

    /// Everything the runtime projected, verbatim.
    ///
    /// Reached only by a deliberate move. Truncating, prettifying, or
    /// summarising evidence here is a defect: this tier exists precisely so
    /// that `.plain` may be confident.
    case technical

    public var isTechnical: Bool {
        self == .technical
    }
}

private struct NappletDisclosureKey: EnvironmentKey {
    static let defaultValue = NappletDisclosure.plain
}

public extension EnvironmentValues {
    var nappletDisclosure: NappletDisclosure {
        get { self[NappletDisclosureKey.self] }
        set { self[NappletDisclosureKey.self] = newValue }
    }
}

public extension View {
    /// Raises (or lowers) the disclosure tier for a subtree.
    func nappletDisclosure(_ disclosure: NappletDisclosure) -> some View {
        environment(\.nappletDisclosure, disclosure)
    }
}
