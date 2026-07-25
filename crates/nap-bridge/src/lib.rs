//! Validated NAP envelope dispatch and provider ownership.
//!
//! Source-window identity is established by the platform trust boundary. This
//! crate accepts only the already-mapped [`SessionContext`] and never reads a
//! principal or session identifier from an untrusted payload.

mod activity;
mod call;
mod envelope;
mod error;
mod outbound;
mod provider;
mod registry;

pub use activity::{ActivityOutcome, ActivitySink, MemoryActivitySink, ProviderActivity};
pub use call::{ProviderCall, ProviderOperation, ProviderWriteCompletion, ProviderWriteProposal};
pub use envelope::{BridgeLimits, Envelope, SessionContext};
pub use error::{BridgeCensus, BridgeError, DispatchOutcome};
pub use outbound::{
    ProviderPush, ProviderPushBatch, ProviderPushError, ProviderPushLimits, ProviderPushObserver,
    ProviderPushSender, ProviderPushTermination, SourceWindowId,
};
pub use provider::{
    Provider, ProviderDescriptor, ProviderError, ProviderPlatformAvailability, ProviderRequest,
    ProviderSession, ProviderSessionContext, ProviderSessionEnd,
};
pub use registry::{InjectionPlan, ProviderRegistry};

#[cfg(test)]
mod tests;
