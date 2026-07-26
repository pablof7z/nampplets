import Foundation
import NMPNativeRuntimeApple

/// A bounded, native-only projection of one NAP-INC action.
///
/// The napplet payload is not treated as navigation authority. It is decoded
/// only for the small schemas accepted by the pinned provider, and the
/// Workbench still scopes presentation to the exact installed build that
/// emitted the action.
/// The decoding below is unchanged: it is the security boundary, and it still
/// fails closed on anything the pinned provider does not accept. What changed
/// is the split between `summary`, which a person reads, and `evidence`, which
/// carries the identifiers. This surface rendered "event <64 hex> · kind 1"
/// into the inspector -- the literal complaint that a raw event id is not
/// something to show a human. See `docs/adr/0008-verdicts-on-the-path.md`.
struct NativeActionNotice: Identifiable, Equatable, Sendable {
    let id: UUID
    let kind: NativeWorkbenchActionKind
    let title: String
    /// Plain language. Never contains an identifier.
    let summary: String
    /// The identifiers, for the technical tier only.
    let evidence: [NappletField]

    init(
        id: UUID = UUID(),
        kind: NativeWorkbenchActionKind,
        title: String,
        summary: String,
        evidence: [NappletField] = []
    ) {
        self.id = id
        self.kind = kind
        self.title = title
        self.summary = summary
        self.evidence = evidence
    }

    static func decode(
        _ action: NativeWorkbenchAction
    ) -> NativeActionNotice? {
        guard let object = try? JSONSerialization.jsonObject(
            with: Data(action.payloadJSON.utf8),
            options: [.fragmentsAllowed]
        ) as? [String: Any]
        else {
            return nil
        }

        switch action.kind {
        case .noteOpen:
            guard
                let target = object["target"] as? [String: Any],
                target["type"] as? String == "event",
                let eventID = boundedHex(target["id"], length: 64)
            else {
                return nil
            }
            var evidence = [NappletField("Event id", eventID)]
            if let kind = boundedInteger(target["kind"]) {
                evidence.append(NappletField("Kind", "\(kind)"))
            }
            if let author = boundedHex(target["pubkey"], length: 64) {
                evidence.append(NappletField("Author key", author))
            }
            return NativeActionNotice(
                kind: action.kind,
                title: "Open a post",
                summary: "This napplet wants to show you a post.",
                evidence: evidence
            )
        case .profileOpen:
            guard let pubkey = boundedHex(object["pubkey"], length: 64) else {
                return nil
            }
            return NativeActionNotice(
                kind: action.kind,
                title: "Open a profile",
                summary: "This napplet wants to show you someone's profile.",
                evidence: [NappletField("Profile key", pubkey)]
            )
        case .composeOpen:
            guard let replyTo = object["replyTo"] as? [String: Any] else {
                return NativeActionNotice(
                    kind: action.kind,
                    title: "Write something",
                    summary: "This napplet wants you to write a post. "
                        + "Napplets can't do that here yet."
                )
            }
            let evidence = boundedHex(replyTo["id"], length: 64)
                .map { [NappletField("Replying to event", $0)] } ?? []
            return NativeActionNotice(
                kind: action.kind,
                title: "Write a reply",
                summary: "This napplet wants you to reply to a post. "
                    + "Napplets can't do that here yet.",
                evidence: evidence
            )
        }
    }

    private static func boundedHex(
        _ value: Any?,
        length: Int
    ) -> String? {
        guard let value = value as? String,
              value.utf8.count == length,
              value.unicodeScalars.allSatisfy(
                  CharacterSet(charactersIn: "0123456789abcdefABCDEF").contains
              )
        else {
            return nil
        }
        return value
    }

    private static func boundedInteger(_ value: Any?) -> Int? {
        guard let number = value as? NSNumber,
              number.doubleValue.rounded() == number.doubleValue,
              number.intValue >= 0,
              number.intValue <= 65_535
        else {
            return nil
        }
        return number.intValue
    }
}
