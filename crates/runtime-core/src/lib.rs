//! Native runtime ownership primitives.
//!
//! This crate intentionally contains no Nostr protocol or NMP types.  NMP
//! integration happens through [`HostDataPlane`], which projects bounded
//! host-owned state and durable-write facts into the runtime.

mod cancellation;
mod grants;
mod host_data;
mod identity;
mod lifecycle;
mod lists;
mod principal;
mod resources;

pub use cancellation::{Cancellation, Cancelled};
pub use grants::{
    Capability, CapabilityRequest, CapabilityRequirement, GrantBatchError, GrantDecision,
    GrantError, GrantLedger, GrantLimits, Sensitivity,
};
pub use host_data::{
    AcceptedWrite, AccountRef, ApprovedWrite, BindingEventSink, BindingRequest, BindingSinkError,
    BoundedJson, BoundedJsonError, HostBindingHandle, HostBindingSnapshot, HostDataError,
    HostDataPlane, ReceiptEventSink, ReceiptObservation, ReceiptReattachment, ReceiptSinkError,
    ReceiptSnapshot, WriteReceiptId,
};
pub use identity::{
    PublicIdentity, PublicIdentityChangeSink, PublicIdentityDataPlane, PublicIdentityError,
    PublicIdentityObservation, PublicIdentityQuery, PublicIdentityRead, PublicIdentityReadLimits,
    PublicIdentitySubscription,
};
pub use lifecycle::{
    ExecutionProfile, Session, SessionError, SessionId, SessionSnapshot, SessionState,
};
pub use lists::{
    ListEntry, ListItemTag, ListReadLimits, ListSelector, ListSnapshot, ListsDataError,
    ListsDataPlane,
};
pub use principal::{Principal, PrincipalError};
pub use resources::{
    ResourceCensus, ResourceClass, ResourceLimits, ResourceRefusal, ResourceTracker, WorkLease,
};
