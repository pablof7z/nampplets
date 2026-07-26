import Foundation
import NMPNativeRuntime

/// The visibility the Rust runtime decided for one activity detail value.
///
/// Native code never classifies a value itself. Substring matching on keys or
/// values both over-matches and under-matches, and the runtime already knows
/// what each value is; a value it classified as secret arrives carrying no
/// bytes at all, so there is nothing here to leak.
public enum NativeRuntimeActivityDetailValue: Hashable, Sendable {
    /// The runtime classified this value as safe to display verbatim.
    case visible(String)
    /// The runtime classified this value as secret and withheld its bytes.
    case redacted

    fileprivate init(_ value: RuntimeActivityDetailValue) {
        switch value {
        case let .visible(text):
            self = .visible(text)
        case .redacted:
            self = .redacted
        }
    }
}

/// One runtime-classified key/value pair belonging to an activity fact.
public struct NativeRuntimeActivityDetail: Hashable, Sendable {
    public let key: String
    public let value: NativeRuntimeActivityDetailValue

    public init(key: String, value: NativeRuntimeActivityDetailValue) {
        self.key = key
        self.value = value
    }

    fileprivate init(_ detail: RuntimeActivityDetail) {
        key = detail.key
        value = NativeRuntimeActivityDetailValue(detail.value)
    }
}

/// A persisted, runtime-owned activity fact. Native code receives the
/// classification strings verbatim and does not become an activity store.
public struct NativeRuntimeActivityRecord: Sendable {
    public let scope: NativeRuntimeActivityScope
    public let category: String
    public let operation: String
    public let outcome: String
    public let occurredAtMillis: UInt64
    /// Details the runtime produced, each already classified by the runtime.
    public let details: [NativeRuntimeActivityDetail]
    /// Details the runtime dropped to stay within its own per-fact bound.
    ///
    /// Carried faithfully to here and no further, and the reason is a blocked
    /// dependency rather than an oversight. It belongs on the fact row it
    /// describes, and the Workbench has no fact rows:
    /// `RuntimeWorkbenchActivitySource.snapshot(from:for:)` returns
    /// `facts: []` unconditionally, because the FFI has no typed activity
    /// severity or kind for Swift to render one from. `ActivitySnapshot`
    /// reports the whole set as `omittedFactCount` instead.
    ///
    /// The single unblock condition is Rust supplying typed facts, at which
    /// point `ActivityFact` needs a `droppedDetailCount` of its own and the
    /// row states it. Until then there is nowhere to put this that would not
    /// be attaching a per-fact number to something that is not a fact.
    public let droppedDetailCount: UInt32

    init(_ record: RuntimeActivitySnapshot) {
        scope = NativeRuntimeActivityScope(
            manifestAuthor: record.author,
            dTag: record.dTag,
            aggregateHash: record.aggregateHash
        )
        category = record.category
        operation = record.operation
        outcome = record.outcome
        occurredAtMillis = record.occurredAtMillis
        details = record.details.map(NativeRuntimeActivityDetail.init)
        droppedDetailCount = record.droppedDetailCount
    }
}
