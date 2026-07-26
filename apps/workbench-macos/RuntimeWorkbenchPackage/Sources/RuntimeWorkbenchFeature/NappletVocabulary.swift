import Foundation

/// A plain-language rendering of one capability domain.
///
/// `sentence` completes "This napplet will be able to …" and is what an
/// ordinary person reads. `explanation` answers "what does that actually
/// mean?" for someone who taps to ask, still without protocol vocabulary.
public struct NappletCapabilityPhrase: Equatable, Sendable {
    public let sentence: String
    public let explanation: String
    public let symbol: String
    /// False when this build has no plain language for the domain. Callers
    /// must say so rather than inventing a phrase or hiding the capability.
    public let isRecognised: Bool

    fileprivate init(
        _ sentence: String,
        _ explanation: String,
        symbol: String,
        isRecognised: Bool = true
    ) {
        self.sentence = sentence
        self.explanation = explanation
        self.symbol = symbol
        self.isRecognised = isRecognised
    }
}

/// The shell's plain-language phrasebook for runtime capability domains.
///
/// Domains are the ones enumerated in `docs/provider-matrix.md`. This type
/// performs rendering only: it never decides whether a capability is granted,
/// required, sensitive, or available. Those remain Rust's, projected through
/// `PermissionCapabilityReview`.
///
/// See `docs/adr/0008-verdicts-on-the-path.md`.
public enum NappletVocabulary {
    /// - Parameter domain: the raw runtime domain, e.g. `outbox`.
    /// - Parameter fallbackTitle: the runtime's own projected title for the
    ///   capability, used only to enrich the unrecognised case.
    public static func phrase(
        forDomain domain: String,
        fallbackTitle: String? = nil
    ) -> NappletCapabilityPhrase {
        if let known = known[domain] {
            return known
        }
        return unrecognised(domain: domain, fallbackTitle: fallbackTitle)
    }

    public static func isRecognised(domain: String) -> Bool {
        known[domain] != nil
    }

    /// Honest degradation, and a deliberate, narrow exception to the rule that
    /// `.plain` renders no raw domain: when the shell has no phrase, the
    /// domain token itself is the most honest thing it can say. Inventing a
    /// friendly sentence would be a lie and hiding the capability would be
    /// worse. Unknown is a verdict too -- and it is the one an ordinary person
    /// most needs stated plainly.
    private static func unrecognised(
        domain: String,
        fallbackTitle: String?
    ) -> NappletCapabilityPhrase {
        let named = fallbackTitle.flatMap { title in
            title.isEmpty || title == domain ? nil : title
        }
        return NappletCapabilityPhrase(
            named.map { "Use \($0)" } ?? "Use a feature this app doesn't recognise",
            "This version of Napplets has no description for “\(domain)”. "
                + "It was added to the runtime after this app was built, so "
                + "the app cannot tell you what it does. Allow it only if you "
                + "trust the publisher.",
            symbol: "questionmark.circle",
            isRecognised: false
        )
    }

    private static let known: [String: NappletCapabilityPhrase] = [
        "identity": NappletCapabilityPhrase(
            "See which account you're using",
            "It can read your public name and picture. It never sees your "
                + "secret key.",
            symbol: "person"
        ),
        "keys": NappletCapabilityPhrase(
            "Sign things on your behalf",
            "It can ask for your approval to sign data as you. Your secret "
                + "key stays in this app and is never handed over.",
            symbol: "signature"
        ),
        "outbox": NappletCapabilityPhrase(
            "Post publicly as you",
            "Anything it publishes appears under your name, exactly as if you "
                + "had posted it yourself.",
            symbol: "paperplane"
        ),
        "dm": NappletCapabilityPhrase(
            "Read and send your private messages",
            "It can see the private messages you exchange and send new ones "
                + "as you.",
            symbol: "lock.bubble"
        ),
        "relay": NappletCapabilityPhrase(
            "Send and receive data over the network",
            "It can talk to the servers you're connected to. It never gets "
                + "direct internet access of its own.",
            symbol: "antenna.radiowaves.left.and.right"
        ),
        "storage": NappletCapabilityPhrase(
            "Save data on this device",
            "Its own private storage, kept separate from every other napplet "
                + "and removed when you uninstall it.",
            symbol: "internaldrive"
        ),
        "config": NappletCapabilityPhrase(
            "Remember its own settings",
            "Small preferences that belong to this napplet, so it doesn't "
                + "start from scratch each time.",
            symbol: "slider.horizontal.3"
        ),
        "media": NappletCapabilityPhrase(
            "Use your camera or microphone",
            "It has to ask your Mac for permission first, and you'll see the "
                + "usual recording indicator whenever it does.",
            symbol: "camera"
        ),
        "upload": NappletCapabilityPhrase(
            "Upload files you choose",
            "Only files you pick yourself. It cannot browse your disk.",
            symbol: "arrow.up.doc"
        ),
        "notify": NappletCapabilityPhrase(
            "Send you notifications",
            "The same notifications any app sends. You can turn them off in "
                + "System Settings at any time.",
            symbol: "bell"
        ),
        "theme": NappletCapabilityPhrase(
            "Match your appearance settings",
            "It follows your light or dark mode and accent colour so it "
                + "doesn't look out of place.",
            symbol: "paintpalette"
        ),
        "link": NappletCapabilityPhrase(
            "Open links in your browser",
            "You'll always see where a link goes before it opens.",
            symbol: "arrow.up.right.square"
        ),
        "intent": NappletCapabilityPhrase(
            "Hand things off to your other apps",
            "For example, sending an address to Maps. The other app decides "
                + "what to do with it.",
            symbol: "square.and.arrow.up"
        ),
        "resource": NappletCapabilityPhrase(
            "Load the files it needs to run",
            "Its own images, fonts and scripts, each one checked against the "
                + "publisher's signature before it loads.",
            symbol: "shippingbox"
        ),
        "lists": NappletCapabilityPhrase(
            "Read and edit your lists",
            "Things like who you follow, who you've muted, and any lists "
                + "you've made.",
            symbol: "list.bullet"
        ),
        "count": NappletCapabilityPhrase(
            "Look up totals from the network",
            "How many replies or reactions something has, without fetching "
                + "every one of them.",
            symbol: "number"
        ),
        "inc": NappletCapabilityPhrase(
            "Ask this app to do things for it",
            "It can request an action from Napplets itself — opening a "
                + "window, say. This app decides whether to carry it out.",
            symbol: "arrow.triangle.turn.up.right.diamond"
        ),
        "common": NappletCapabilityPhrase(
            "Use shared features of this app",
            "Ordinary building blocks that most napplets rely on.",
            symbol: "square.grid.2x2"
        ),
        "cvm": NappletCapabilityPhrase(
            "Use an outside computing service",
            "Some of its work happens on a machine that isn't yours. Whatever "
                + "it sends there leaves this device.",
            symbol: "server.rack"
        ),
        "ble": NappletCapabilityPhrase(
            "Connect to nearby Bluetooth devices",
            "It can find and talk to Bluetooth hardware around you.",
            symbol: "dot.radiowaves.right"
        ),
        "webrtc": NappletCapabilityPhrase(
            "Start live audio and video calls",
            "It can open a direct connection to other people for real-time "
                + "audio and video.",
            symbol: "video"
        ),
        "serial": NappletCapabilityPhrase(
            "Talk to a device plugged into this Mac",
            "Hardware connected over a serial port, such as a development "
                + "board or a signing device.",
            symbol: "cable.connector"
        ),
    ]
}
