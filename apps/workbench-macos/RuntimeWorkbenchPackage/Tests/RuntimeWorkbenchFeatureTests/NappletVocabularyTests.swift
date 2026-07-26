@testable import RuntimeWorkbenchFeature
import Testing

/// The domains enumerated in `docs/provider-matrix.md`. If a domain is added
/// there and not here, `everyProviderMatrixDomainHasPlainLanguage` fails --
/// which is the point: shipping a capability with no plain rendering means
/// shipping a consent prompt an ordinary person cannot read.
private let providerMatrixDomains = [
    "relay", "identity", "storage", "inc", "theme", "keys", "media", "notify",
    "config", "resource", "cvm", "outbox", "upload", "intent", "ble",
    "webrtc", "link", "count", "lists", "serial", "common", "dm",
]

@Test func everyProviderMatrixDomainHasPlainLanguage() {
    for domain in providerMatrixDomains {
        let phrase = NappletVocabulary.phrase(forDomain: domain)
        #expect(
            phrase.isRecognised,
            "\(domain) is in the provider matrix but has no plain phrase"
        )
        #expect(!phrase.sentence.isEmpty)
        #expect(!phrase.explanation.isEmpty)
    }
}

/// Every string on the plain path is checked against the vocabulary ADR 0008
/// forbids. These are the words that made the old sheets unreadable.
@Test func plainLanguageNeverUsesRuntimeVocabulary() {
    let forbidden = [
        "principal", "manifest", "aggregate", "projection", "coordinate",
        "facade", "exact build", "dtag", "npub", "nsec", "nip-", "relay url",
        "revision", "event id",
    ]
    for domain in providerMatrixDomains {
        let phrase = NappletVocabulary.phrase(forDomain: domain)
        let text = "\(phrase.sentence) \(phrase.explanation)".lowercased()
        for term in forbidden {
            #expect(
                !text.contains(term),
                "\(domain) leaks “\(term)” onto the plain path: \(text)"
            )
        }
    }
}

/// Honest degradation. A capability this build has no language for is named
/// as unrecognised and its raw token shown -- never given an invented
/// friendly phrase, and never quietly dropped from a consent prompt.
@Test func anUnknownDomainIsNamedRatherThanInventedOrHidden() {
    let phrase = NappletVocabulary.phrase(forDomain: "quantum-teleport")

    #expect(!phrase.isRecognised)
    #expect(!phrase.sentence.isEmpty)
    #expect(phrase.explanation.contains("quantum-teleport"))
    #expect(phrase.explanation.lowercased().contains("trust the publisher"))
    #expect(!NappletVocabulary.isRecognised(domain: "quantum-teleport"))
}

/// The runtime's own title is preferable to "a feature this app doesn't
/// recognise" when it actually says something, but it must not be mistaken
/// for a recognised phrase.
@Test func anUnknownDomainPrefersTheRuntimesOwnTitleWhenItHasOne() {
    let named = NappletVocabulary.phrase(
        forDomain: "hologram",
        fallbackTitle: "Holographic Display"
    )
    #expect(named.sentence == "Use Holographic Display")
    #expect(!named.isRecognised)

    // A title that merely repeats the domain adds nothing.
    let echoed = NappletVocabulary.phrase(
        forDomain: "hologram",
        fallbackTitle: "hologram"
    )
    #expect(echoed.sentence == "Use a feature this app doesn't recognise")
}

@Test func aPublicKeyIsNeverPresentedAsAName() {
    let key = String(repeating: "a", count: 64)

    #expect(
        NappletIdentityPresentation.publisherName(
            displayName: nil,
            publicKey: key
        ) == "Unnamed publisher"
    )
    #expect(
        NappletIdentityPresentation.publisherName(
            displayName: "  ",
            publicKey: key
        ) == "Unnamed publisher"
    )
    // A projection that echoes the key into the display name is still a key.
    #expect(
        NappletIdentityPresentation.publisherName(
            displayName: key,
            publicKey: key
        ) == "Unnamed publisher"
    )
    #expect(
        NappletIdentityPresentation.publisherName(
            displayName: "Alice",
            publicKey: key
        ) == "Alice"
    )
    #expect(
        !NappletIdentityPresentation.publisherIsUnnamed(
            displayName: "Alice",
            publicKey: key
        )
    )
}

@Test func aSettledVerdictHasNothingToSay() {
    // `NappletNotice` renders nothing when `message` is nil, which is what
    // keeps the app quiet when everything is fine.
    #expect(NappletTrustVerdict.settled.message == nil)
    #expect(NappletTrustVerdict.settled.isSettled)
    #expect(NappletTrustVerdict.caution("careful").message == "careful")
    #expect(!NappletTrustVerdict.blocked("no").isSettled)
}
