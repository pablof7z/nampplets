import NMPNativeRuntimeApple
import Testing
@testable import RuntimeWorkbenchFeature

struct NativeActionPresentationTests {
    private let author = String(repeating: "a", count: 64)
    private let aggregate = String(repeating: "b", count: 64)

    @Test
    func notePayloadProjectsOnlyTheBoundedEventTarget() {
        let eventID = String(repeating: "c", count: 64)
        let action = NativeWorkbenchAction(
            manifestAuthor: author,
            dTag: "good-morning",
            aggregateHash: aggregate,
            sessionID: 4,
            sourceWindowID: 8,
            kind: .noteOpen,
            payloadJSON: "{\"target\":{\"type\":\"event\",\"id\":\"\(eventID)\",\"kind\":1,\"pubkey\":\"\(author)\"},\"extra\":{\"secret\":\"ignored\"}}"
        )

        let notice = NativeActionNotice.decode(action)

        #expect(notice?.title == "Open a post")
        // The bounded target is still projected in full -- it moved to the
        // technical tier rather than being dropped. Nothing the napplet sent
        // outside the accepted schema (`extra.secret`) appears anywhere.
        let evidence = notice?.evidence ?? []
        #expect(evidence.contains { $0.value == eventID })
        #expect(evidence.contains { $0.label == "Kind" && $0.value == "1" })
        #expect(evidence.contains { $0.value == author })
        #expect(!evidence.contains { $0.value.contains("ignored") })

        // An identifier on the plain path is the defect this surface had.
        #expect(notice?.summary.contains(eventID) == false)
        #expect(notice?.summary.contains(author) == false)
    }

    @Test
    func malformedProfilePayloadFailsClosed() {
        let action = NativeWorkbenchAction(
            manifestAuthor: author,
            dTag: "good-morning",
            aggregateHash: aggregate,
            sessionID: 4,
            sourceWindowID: 8,
            kind: .profileOpen,
            payloadJSON: #"{"pubkey":"not-a-pubkey"}"#
        )

        #expect(NativeActionNotice.decode(action) == nil)
    }

    @Test
    func composePayloadIsDisplayedWithoutCreatingAComposer() {
        let eventID = String(repeating: "d", count: 64)
        let action = NativeWorkbenchAction(
            manifestAuthor: author,
            dTag: "good-morning",
            aggregateHash: aggregate,
            sessionID: 4,
            sourceWindowID: 8,
            kind: .composeOpen,
            payloadJSON: "{\"intent\":\"reply\",\"replyTo\":{\"id\":\"\(eventID)\"}}"
        )

        let notice = NativeActionNotice.decode(action)

        #expect(notice?.title == "Write a reply")
        // Still says plainly that no composer exists, without pretending one
        // is coming and without naming the event on the plain path.
        #expect(notice?.summary.contains("can't do that here yet") == true)
        #expect(notice?.summary.contains(eventID) == false)
        #expect(notice?.evidence.contains { $0.value == eventID } == true)
    }
}
