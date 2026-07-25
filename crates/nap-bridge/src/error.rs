use std::sync::Arc;

use nmp_native_runtime_core::{Capability, ResourceRefusal, SessionId};
use thiserror::Error;

use crate::{ProviderCall, ProviderError, SourceWindowId};

#[derive(Debug)]
pub enum DispatchOutcome {
    IgnoredUnknown,
    Handled(ProviderCall),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeCensus {
    pub sessions: usize,
    pub dispatched: u64,
    pub ignored_unknown: u64,
    pub refusals: u64,
    pub throttles: u64,
}

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("bridge limits must be finite and non-zero")]
    InvalidLimits,
    #[error("provider {domain} has an invalid bounded action inventory")]
    InvalidProvider { domain: Capability },
    #[error("provider {domain} is unavailable on this platform: {reason}")]
    ProviderUnavailable {
        domain: Capability,
        reason: Arc<str>,
    },
    #[error("provider {domain} is already registered")]
    DuplicateProvider { domain: Capability },
    #[error("provider capacity {capacity} is full")]
    ProviderCapacity { capacity: usize },
    #[error("required domains are unavailable: {missing:?}")]
    MissingRequiredDomains { missing: Vec<Capability> },
    #[error("session capacity {capacity} is full")]
    SessionCapacity { capacity: usize },
    #[error("mapped session {session:?} is not open")]
    UnknownSession { session: SessionId },
    #[error("mapped session {session:?} does not match its fixed principal and profile")]
    SessionIdentityMismatch { session: SessionId },
    #[error("source window {source_window:?} is not mapped to session {session:?}")]
    SourceWindowMismatch {
        session: SessionId,
        source_window: SourceWindowId,
    },
    #[error("envelope is {actual} bytes; the maximum is {maximum}")]
    EnvelopeTooLarge { actual: usize, maximum: usize },
    #[error("malformed envelope: {reason}")]
    MalformedEnvelope { reason: String },
    #[error("message rate exceeded for session {session:?}")]
    MessageRateExceeded { session: SessionId },
    #[error("capability {domain} was not injected into this fixed session profile")]
    CapabilityDenied { domain: Capability },
    #[error("the injection plan belongs to a different exact-build principal")]
    PlanPrincipalMismatch,
    #[error("capability {domain} requires an explicit bounded user decision")]
    GrantDecisionRequired { domain: Capability },
    #[error("provider response exceeded the bounded response limit")]
    ResponseTooLarge,
    #[error(transparent)]
    ResourceRefused(#[from] ResourceRefusal),
    #[error(transparent)]
    Provider(#[from] ProviderError),
}
