import Foundation

/// How the shell renders who made something, and whether anything about it is
/// worth interrupting a person for.
///
/// See `docs/adr/0008-verdicts-on-the-path.md`.
public enum NappletIdentityPresentation {
    /// What a person is shown in place of a publisher.
    ///
    /// A public key is never a name. When the publisher has not given one,
    /// saying so is both the honest answer and the more useful trust signal:
    /// "this publisher hasn't told you who they are" is a fact an ordinary
    /// person can act on, and sixty-four hexadecimal characters is not.
    public static func publisherName(
        displayName: String?,
        publicKey: String
    ) -> String {
        guard
            let displayName,
            !displayName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
            displayName != publicKey
        else {
            return "Unnamed publisher"
        }
        return displayName
    }

    /// True when `publisherName` fell back, so a surface can add one short
    /// clause of context without having to re-derive the condition.
    public static func publisherIsUnnamed(
        displayName: String?,
        publicKey: String
    ) -> Bool {
        publisherName(displayName: displayName, publicKey: publicKey)
            == "Unnamed publisher"
    }

    /// An abbreviated key for the technical tier only.
    ///
    /// Never call this to soften a `.plain` surface. A shortened key is still
    /// a key: it is unreadable, unmemorable, and invites a person to believe
    /// they have verified something by looking at it.
    public static func shortKey(_ key: String) -> String {
        guard key.count > 16 else {
            return key
        }
        return "\(key.prefix(8))…\(key.suffix(6))"
    }
}

extension String {
    /// Joins a runtime-supplied clause onto a sentence the shell wrote.
    ///
    /// Rust's reason strings are fragments and start lowercase, so appending
    /// one straight after a full stop produced "…works here. no provider
    /// metadata is registered…". This capitalises the first letter and gives
    /// it a full stop, without touching the rest of the runtime's wording.
    func appendingSentence(_ clause: String) -> String {
        let trimmed = clause.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let first = trimmed.first else {
            return self
        }
        let capitalised = first.uppercased() + trimmed.dropFirst()
        let terminated = ".!?".contains(capitalised.last!)
            ? capitalised
            : capitalised + "."
        return "\(self) \(terminated)"
    }
}

/// Whether the shell needs to say anything at all about trust.
///
/// The default is silence. Verification is the application's job, and an
/// application that has done its job does not congratulate itself for it --
/// a green seal on every surface is not reassurance, it is noise that makes
/// the one screen that genuinely needs attention indistinguishable from the
/// twenty that do not.
///
/// Modelling the settled case as "render nothing" makes that structural
/// rather than a habit each view has to remember.
public enum NappletTrustVerdict: Equatable, Sendable {
    /// Everything checked out. Surfaces render nothing.
    case settled
    /// Worth reading before continuing, but not disqualifying.
    case caution(String)
    /// The action cannot proceed, and this is why.
    case blocked(String)

    public var message: String? {
        switch self {
        case .settled: nil
        case let .caution(message), let .blocked(message): message
        }
    }

    public var isSettled: Bool {
        self == .settled
    }

    public var symbol: String {
        switch self {
        case .settled: "checkmark"
        case .caution: "exclamationmark.triangle"
        case .blocked: "hand.raised"
        }
    }
}
