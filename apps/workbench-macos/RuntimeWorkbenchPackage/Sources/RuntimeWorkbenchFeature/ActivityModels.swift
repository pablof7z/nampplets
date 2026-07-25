import Foundation

public enum ActivityLimits {
    public static let maximumFacts = 256
    public static let maximumDetailFieldsPerFact = 24
    public static let maximumFactUTF8Bytes = 32 * 1_024
    public static let maximumSnapshotUTF8Bytes = 2 * 1_024 * 1_024
    public static let maximumIdentifierUTF8Bytes = 512
    public static let maximumDisplayFieldUTF8Bytes = 4 * 1_024
    public static let maximumDetailKeyUTF8Bytes = 128
    public static let maximumDetailValueUTF8Bytes = 8 * 1_024
}

/// The immutable build identity used to scope every activity projection.
///
/// This is deliberately more specific than a napplet coordinate: two builds
/// from the same publisher and d-tag never share activity or diagnostics.
public struct ActivityExactBuildScope: Hashable, Sendable {
    public let manifestAuthor: String
    public let dTag: String
    public let aggregateHash: String

    public init?(
        manifestAuthor: String,
        dTag: String,
        aggregateHash: String
    ) {
        let fields = [manifestAuthor, dTag, aggregateHash]
        guard fields.allSatisfy({
            !$0.isEmpty
                && $0.utf8.count <= ActivityLimits.maximumIdentifierUTF8Bytes
        }) else {
            return nil
        }

        self.manifestAuthor = manifestAuthor
        self.dTag = dTag
        self.aggregateHash = aggregateHash
    }
}

public enum ActivitySeverity: String, CaseIterable, Hashable, Sendable {
    case debug
    case information
    case warning
    case error

    public var title: String {
        switch self {
        case .debug: "Debug"
        case .information: "Information"
        case .warning: "Warning"
        case .error: "Error"
        }
    }
}

public enum ActivityCategory: String, CaseIterable, Hashable, Sendable {
    case provider
    case session
    case resource
    case receipt
    case recovery

    public var title: String {
        switch self {
        case .provider: "Providers"
        case .session: "Sessions"
        case .resource: "Resources"
        case .receipt: "Receipts"
        case .recovery: "Recovery"
        }
    }
}

/// A semantic row type supplied by the runtime-owned activity projection.
///
/// Native code does not infer success, refusal, recovery, or receipt state from
/// strings. It renders the classification already decided by the runtime.
public enum ActivityFactKind: String, CaseIterable, Hashable, Sendable {
    case providerCall
    case providerRefusal
    case activeSession
    case activeBinding
    case activeResource
    case pendingReceipt
    case crash
    case recovery

    public var title: String {
        switch self {
        case .providerCall: "Provider call"
        case .providerRefusal: "Provider refusal"
        case .activeSession: "Active session"
        case .activeBinding: "Active binding"
        case .activeResource: "Active resource"
        case .pendingReceipt: "Pending receipt"
        case .crash: "Crash"
        case .recovery: "Recovery"
        }
    }
}

/// The visibility the runtime decided for one detail value.
///
/// Native code does not classify values. Whether something is secret is a
/// security decision the runtime makes where it produces the fact and where
/// it knows what the value is; keyword matching on the text both over-matches
/// and under-matches. A value the runtime classified as secret reaches this
/// layer with no bytes, so `.redacted` carries nothing to reveal.
public enum ActivityDetailValue: Equatable, Sendable {
    /// A value the runtime classified as safe to display verbatim.
    case visible(String)
    /// A value the runtime classified as secret and withheld.
    case redacted

    /// The text to render. There is no unclassified path to a display string.
    public var displayText: String {
        switch self {
        case let .visible(text): text
        case .redacted: ActivityDetailField.redactedPlaceholder
        }
    }

    var measuredUTF8ByteCount: Int {
        switch self {
        case let .visible(text): text.utf8.count
        case .redacted: 0
        }
    }
}

public struct ActivityDetailField: Identifiable, Equatable, Sendable {
    /// Shown in place of a value the runtime withheld.
    public static let redactedPlaceholder = "[REDACTED]"

    public let key: String
    public let value: ActivityDetailValue

    public var id: String {
        key
    }

    /// The text to render for this field.
    public var displayValue: String {
        value.displayText
    }

    /// Whether the runtime classified this value as secret.
    public var isRedacted: Bool {
        value == .redacted
    }

    public init?(key: String, value: ActivityDetailValue) {
        guard !key.isEmpty,
              key.utf8.count <= ActivityLimits.maximumDetailKeyUTF8Bytes,
              value.measuredUTF8ByteCount
                <= ActivityLimits.maximumDetailValueUTF8Bytes
        else {
            return nil
        }

        self.key = key
        self.value = value
    }
}

public struct ActivityFact: Identifiable, Equatable, Sendable {
    public let id: String
    public let scope: ActivityExactBuildScope
    public let ordinal: UInt64
    public let severity: ActivitySeverity
    public let category: ActivityCategory
    public let kind: ActivityFactKind
    public let title: String
    public let summary: String
    public let evidenceSummary: String?
    public let detailFields: [ActivityDetailField]

    public init?(
        id: String,
        scope: ActivityExactBuildScope,
        ordinal: UInt64,
        severity: ActivitySeverity,
        category: ActivityCategory,
        kind: ActivityFactKind,
        title: String,
        summary: String,
        evidenceSummary: String? = nil,
        detailFields: [ActivityDetailField] = []
    ) {
        let displayFields = [id, title, summary, evidenceSummary ?? ""]
        guard !id.isEmpty,
              !title.isEmpty,
              detailFields.count
                <= ActivityLimits.maximumDetailFieldsPerFact,
              Set(detailFields.map(\.key)).count == detailFields.count,
              displayFields.allSatisfy({
                  $0.utf8.count <= ActivityLimits.maximumDisplayFieldUTF8Bytes
              })
        else {
            return nil
        }

        let byteCount = displayFields.reduce(0) { $0 + $1.utf8.count }
            + detailFields.reduce(0) {
                $0 + $1.key.utf8.count + $1.value.measuredUTF8ByteCount
            }
        guard byteCount <= ActivityLimits.maximumFactUTF8Bytes else {
            return nil
        }

        // The runtime owns every string here. It produced these display
        // fields, so it is also where a secret would have been withheld;
        // re-scanning them for secret-looking substrings would only add a
        // second, weaker opinion.
        self.id = id
        self.scope = scope
        self.ordinal = ordinal
        self.severity = severity
        self.category = category
        self.kind = kind
        self.title = title
        self.summary = summary
        self.evidenceSummary = evidenceSummary
        self.detailFields = detailFields
    }

    fileprivate var activityUTF8ByteCount: Int {
        [id, title, summary, evidenceSummary ?? ""].reduce(0) {
            $0 + $1.utf8.count
        } + detailFields.reduce(0) {
            $0 + $1.key.utf8.count + $1.value.measuredUTF8ByteCount
        }
    }
}

/// Counts are supplied by the runtime projection and never derived from the
/// bounded recent-fact window.
public struct ActivityInventorySummary: Equatable, Sendable {
    public static let maximumActiveSessions = 32
    public static let maximumActiveBindings = 256
    public static let maximumActiveResources = 1_024
    public static let maximumPendingReceipts = 512

    public let activeSessions: Int
    public let activeBindings: Int
    public let activeResources: Int
    public let pendingReceipts: Int

    public init?(
        activeSessions: Int,
        activeBindings: Int,
        activeResources: Int,
        pendingReceipts: Int
    ) {
        guard (0...Self.maximumActiveSessions).contains(activeSessions),
              (0...Self.maximumActiveBindings).contains(activeBindings),
              (0...Self.maximumActiveResources).contains(activeResources),
              (0...Self.maximumPendingReceipts).contains(pendingReceipts)
        else {
            return nil
        }

        self.activeSessions = activeSessions
        self.activeBindings = activeBindings
        self.activeResources = activeResources
        self.pendingReceipts = pendingReceipts
    }

    public static let empty = ActivityInventorySummary(
        activeSessions: 0,
        activeBindings: 0,
        activeResources: 0,
        pendingReceipts: 0
    )!
}

/// A bounded, screen-shaped replacement projection.
public struct ActivitySnapshot: Equatable, Sendable {
    public let scope: ActivityExactBuildScope
    public let revision: UInt64
    public let inventory: ActivityInventorySummary
    public let facts: [ActivityFact]
    public let omittedFactCount: UInt64

    public init?(
        scope: ActivityExactBuildScope,
        revision: UInt64,
        inventory: ActivityInventorySummary,
        facts: [ActivityFact],
        omittedFactCount: UInt64 = 0
    ) {
        guard facts.count <= ActivityLimits.maximumFacts,
              facts.allSatisfy({ $0.scope == scope }),
              Set(facts.map(\.id)).count == facts.count,
              facts.reduce(0, {
                  $0 + $1.activityUTF8ByteCount
              }) <= ActivityLimits.maximumSnapshotUTF8Bytes
        else {
            return nil
        }

        self.scope = scope
        self.revision = revision
        self.inventory = inventory
        self.facts = facts
        self.omittedFactCount = omittedFactCount
    }
}

public struct ActivityUpdateGap: Equatable, Sendable {
    public let expectedPredecessorRevision: UInt64
    public let receivedPredecessorRevision: UInt64
    public let receivedRevision: UInt64

    public init(
        expectedPredecessorRevision: UInt64,
        receivedPredecessorRevision: UInt64,
        receivedRevision: UInt64
    ) {
        self.expectedPredecessorRevision = expectedPredecessorRevision
        self.receivedPredecessorRevision = receivedPredecessorRevision
        self.receivedRevision = receivedRevision
    }
}
