import Foundation

public struct NappletArtifact: Sendable {
    public let title: String
    let reader: any VerifiedArtifactByteReader
    let runtimeSession: (any TrustedNappletRuntimeSession)?
    let negotiatedDomains: [String]

    init(
        title: String,
        reader: any VerifiedArtifactByteReader,
        runtimeSession: (any TrustedNappletRuntimeSession)? = nil,
        negotiatedDomains: [String] = ["shell"]
    ) {
        self.title = title
        self.reader = reader
        self.runtimeSession = runtimeSession
        self.negotiatedDomains = negotiatedDomains
    }

    init(title: String, html: String) {
        self = Self.internalM1Canary(title: title, html: html)
    }

    var html: String {
        guard let index = try? reader.readSealed(logicalPath: "/index.html") else {
            return ""
        }
        return String(data: index.bytes, encoding: .utf8) ?? ""
    }

    /// Internal M1 canary construction. This is deliberately not public API:
    /// production executable bytes must arrive through the Rust-owned verified
    /// artifact handle adapter.
    static func internalM1Canary(title: String, html: String) -> Self {
        NappletArtifact(
            title: title,
            reader: InMemoryVerifiedArtifactReader(files: [
                SealedArtifactBytes(
                    logicalPath: "/index.html",
                    sha256: String(repeating: "0", count: 64),
                    bytes: Data(html.utf8)
                )
            ])
        )
    }

    static func bundledCompatibilityFixture() -> Self? {
        guard let url = TrustedShellResources.fixtureURL,
              let html = try? String(contentsOf: url, encoding: .utf8)
        else {
            return nil
        }
        return NappletArtifact.internalM1Canary(
            title: "Workbench Welcome",
            html: html
        )
    }
}

public enum TrustedNappletActivity: Sendable, Equatable {
    case loading
    case mounted
    case request(type: String)
    case refused(reason: String)
    case crashed
}
