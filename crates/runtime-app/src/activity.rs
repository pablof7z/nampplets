//! Runtime-owned activity facts and the sensitivity classification carried
//! with them.
//!
//! Whether a value is secret is decided here, where the fact is produced and
//! where the runtime knows what the value actually is. It is never re-derived
//! downstream by inspecting key or value text: substring matching both
//! over-matches (redacting a harmless relay URL because it spells "token")
//! and under-matches (rendering a credential nobody thought to enumerate).
//!
//! The classification is structural rather than advisory.
//! [`ActivityDetailValue`] has no variant that carries secret bytes, so a
//! value classified [`ActivitySensitivity::Secret`] is dropped at production:
//! it never reaches the store, the ABI, or a renderer, and no later layer can
//! be persuaded to reveal it.

use std::sync::Arc;

use nmp_native_runtime_core::Principal;

/// The most detail values one activity fact may carry.
///
/// Details beyond this bound are dropped and counted, never silently lost.
pub const MAXIMUM_ACTIVITY_DETAILS: usize = 24;

/// Whether a produced value may be shown, decided by the producer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivitySensitivity {
    /// The runtime knows this value carries nothing secret.
    Public,
    /// The runtime knows this value is secret-bearing.
    Secret,
}

/// The classified value of one activity detail.
///
/// There is deliberately no variant holding secret bytes. Redaction is a
/// value that was never constructed, not a value that was hidden.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivityDetailValue {
    /// A value the producer classified as safe to display verbatim.
    Visible(Arc<str>),
    /// A value the producer classified as secret; its bytes were discarded.
    Redacted,
}

/// One classified key/value pair attached to an activity fact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityDetail {
    key: Arc<str>,
    value: ActivityDetailValue,
}

impl ActivityDetail {
    /// Record a value the producer knows is safe to display.
    pub fn visible(key: &str, value: &str) -> Self {
        Self {
            key: Arc::from(key),
            value: ActivityDetailValue::Visible(Arc::from(value)),
        }
    }

    /// Record the presence of a secret-bearing value without its bytes.
    ///
    /// The value is not a parameter: a caller that cannot name the secret
    /// cannot leak it.
    pub fn secret(key: &str) -> Self {
        Self {
            key: Arc::from(key),
            value: ActivityDetailValue::Redacted,
        }
    }

    /// Record a value the producer has in hand together with its
    /// classification. Secret values are discarded here.
    pub fn classified(key: &str, value: &str, sensitivity: ActivitySensitivity) -> Self {
        match sensitivity {
            ActivitySensitivity::Public => Self::visible(key, value),
            ActivitySensitivity::Secret => Self::secret(key),
        }
    }

    pub fn key(&self) -> &str {
        self.key.as_ref()
    }

    pub fn value(&self) -> &ActivityDetailValue {
        &self.value
    }

    /// Whether the producer classified this value as secret.
    pub fn is_redacted(&self) -> bool {
        matches!(self.value, ActivityDetailValue::Redacted)
    }
}

/// One bounded activity fact owned by the runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityFact {
    pub principal: Principal,
    pub category: Arc<str>,
    pub operation: Arc<str>,
    pub outcome: Arc<str>,
    pub occurred_at_millis: u64,
    details: Vec<ActivityDetail>,
    dropped_detail_count: u32,
}

impl ActivityFact {
    /// Build one fact, enforcing the detail bound at the production point.
    pub fn new(
        principal: Principal,
        category: &str,
        operation: &str,
        outcome: &str,
        mut details: Vec<ActivityDetail>,
        occurred_at_millis: u64,
    ) -> Self {
        let dropped_detail_count =
            u32::try_from(details.len().saturating_sub(MAXIMUM_ACTIVITY_DETAILS))
                .unwrap_or(u32::MAX);
        details.truncate(MAXIMUM_ACTIVITY_DETAILS);
        Self {
            principal,
            category: Arc::from(category),
            operation: Arc::from(operation),
            outcome: Arc::from(outcome),
            occurred_at_millis,
            details,
            dropped_detail_count,
        }
    }

    /// The classified details, in production order.
    pub fn details(&self) -> &[ActivityDetail] {
        &self.details
    }

    /// Details dropped by the per-fact bound.
    pub fn dropped_detail_count(&self) -> u32 {
        self.dropped_detail_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal() -> Principal {
        Principal::new("a".repeat(64), "activity", "b".repeat(64)).expect("principal")
    }

    #[test]
    fn a_secret_detail_retains_no_value_bytes() {
        let detail = ActivityDetail::classified(
            "signing-key",
            "nsec1thismustneverappear",
            ActivitySensitivity::Secret,
        );

        assert!(detail.is_redacted());
        assert_eq!(detail.value(), &ActivityDetailValue::Redacted);
        assert!(!format!("{detail:?}").contains("nsec1thismustneverappear"));
    }

    #[test]
    fn a_public_detail_is_shown_even_when_its_name_reads_like_a_secret() {
        let detail = ActivityDetail::classified(
            "token-relay",
            "wss://relay.example",
            ActivitySensitivity::Public,
        );

        assert!(!detail.is_redacted());
        assert_eq!(
            detail.value(),
            &ActivityDetailValue::Visible(Arc::from("wss://relay.example"))
        );
    }

    #[test]
    fn details_beyond_the_bound_are_counted_rather_than_dropped_silently() {
        let details = (0..MAXIMUM_ACTIVITY_DETAILS + 3)
            .map(|index| ActivityDetail::visible(&format!("key-{index}"), "value"))
            .collect();
        let fact = ActivityFact::new(principal(), "write", "accept", "durable", details, 7);

        assert_eq!(fact.details().len(), MAXIMUM_ACTIVITY_DETAILS);
        assert_eq!(fact.dropped_detail_count(), 3);
    }

    #[test]
    fn a_fact_without_details_is_still_a_fact() {
        let fact = ActivityFact::new(principal(), "session", "launch", "running", Vec::new(), 1);

        assert!(fact.details().is_empty());
        assert_eq!(fact.dropped_detail_count(), 0);
    }
}
