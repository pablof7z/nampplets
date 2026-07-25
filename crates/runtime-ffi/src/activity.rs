//! ABI projection of runtime-owned activity facts.
//!
//! The runtime decided each detail's visibility where it produced the fact.
//! This module only re-shapes that decision for the boundary; it never
//! inspects key or value text, and native callers must not either.
//!
//! [`RuntimeActivityDetailValue`] has no variant carrying secret bytes, so a
//! detail the runtime classified as secret crosses the ABI as the fact that a
//! secret exists and nothing more. Native code renders the marking it is
//! given rather than guessing from strings.

use nmp_native_runtime_app::{ActivityDetail, ActivityDetailValue, ActivityFact};

/// One activity detail value whose visibility the runtime already decided.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeActivityDetailValue {
    /// The runtime classified this value as safe to display verbatim.
    Visible { text: String },
    /// The runtime classified this value as secret; no bytes are carried.
    Redacted,
}

/// One classified key/value pair belonging to an activity fact.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeActivityDetail {
    pub key: String,
    pub value: RuntimeActivityDetailValue,
}

/// One bounded activity fact attributed to an exact build.
#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeActivitySnapshot {
    pub author: String,
    pub d_tag: String,
    pub aggregate_hash: String,
    pub category: String,
    pub operation: String,
    pub outcome: String,
    pub occurred_at_millis: u64,
    /// Details the runtime produced, each already classified.
    pub details: Vec<RuntimeActivityDetail>,
    /// Details the runtime dropped to stay within its per-fact bound.
    pub dropped_detail_count: u32,
}

fn detail(detail: &ActivityDetail) -> RuntimeActivityDetail {
    RuntimeActivityDetail {
        key: detail.key().to_owned(),
        value: match detail.value() {
            ActivityDetailValue::Visible(text) => RuntimeActivityDetailValue::Visible {
                text: text.as_ref().to_owned(),
            },
            ActivityDetailValue::Redacted => RuntimeActivityDetailValue::Redacted,
        },
    }
}

pub(crate) fn activity_snapshot(fact: &ActivityFact) -> RuntimeActivitySnapshot {
    RuntimeActivitySnapshot {
        author: fact.principal.manifest_author().to_owned(),
        d_tag: fact.principal.d_tag().to_owned(),
        aggregate_hash: fact.principal.aggregate_hash().to_owned(),
        category: fact.category.to_string(),
        operation: fact.operation.to_string(),
        outcome: fact.outcome.to_string(),
        occurred_at_millis: fact.occurred_at_millis,
        details: fact.details().iter().map(detail).collect(),
        dropped_detail_count: fact.dropped_detail_count(),
    }
}

#[cfg(test)]
mod tests {
    use nmp_native_runtime_app::ActivitySensitivity;
    use nmp_native_runtime_core::Principal;

    use super::*;

    #[test]
    fn a_secret_detail_crosses_the_abi_without_its_bytes() {
        let fact = ActivityFact::new(
            Principal::new("a".repeat(64), "activity", "b".repeat(64)).expect("principal"),
            "write",
            "accept",
            "durable-obligation",
            vec![
                ActivityDetail::classified(
                    "approved-draft",
                    "nsec1thismustneverappear",
                    ActivitySensitivity::Secret,
                ),
                ActivityDetail::visible("token-relay", "wss://relay.example"),
            ],
            9,
        );

        let projected = activity_snapshot(&fact);

        assert_eq!(
            projected.details[0].value,
            RuntimeActivityDetailValue::Redacted
        );
        assert_eq!(
            projected.details[1].value,
            RuntimeActivityDetailValue::Visible {
                text: "wss://relay.example".to_owned()
            }
        );
        assert!(!format!("{projected:?}").contains("nsec1thismustneverappear"));
        assert_eq!(projected.dropped_detail_count, 0);
    }
}
