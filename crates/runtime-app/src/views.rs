//! Bounded read-only projections rendered by native shells.

use std::sync::Arc;

use nmp_native_nap_bridge::SourceWindowId;
use nmp_native_runtime_core::{
    BoundedJson, Capability, CapabilityRequirement, GrantDecision, Principal, ReceiptSnapshot,
    ResourceCensus, Sensitivity, SessionId, SessionSnapshot, WriteReceiptId,
};
use nmp_native_runtime_store::InstalledBuild;
use thiserror::Error;

use crate::commands::ProviderOperationId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppErrorFact {
    pub code: AppErrorCode,
    pub principal: Option<Principal>,
    pub session: Option<SessionId>,
    pub detail: Arc<str>,
    pub occurred_at_millis: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AppErrorCode {
    Capacity,
    NotInstalled,
    OfflineBytesUnavailable,
    UnsupportedManifestIdentity,
    ArtifactIdentityMismatch,
    MissingIndex,
    UnknownSession,
    SessionIdentityMismatch,
    InvalidLifecycle,
    Grant,
    Bridge,
    Binding,
    HostData,
    Store,
    Receipt,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityFact {
    pub principal: Principal,
    pub category: Arc<str>,
    pub operation: Arc<str>,
    pub outcome: Arc<str>,
    pub occurred_at_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingView {
    pub id: Arc<str>,
    pub schema: Arc<str>,
    pub logical_source_id: Option<Arc<str>>,
    pub revision: Option<u64>,
}

/// Fixed provider domains injected for one mapped session. This is the same
/// immutable negotiation plan used to build `shell.init`; native platforms
/// must not infer or widen it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionDomainView {
    pub session: SessionId,
    pub domains: Vec<Capability>,
}

/// Bounded provider-to-component delivery state for one exact mapped source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderPushLaneView {
    pub session: SessionId,
    pub source_window: SourceWindowId,
    pub ready: bool,
    pub last_provider_sequence: Option<u64>,
    pub delivered_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptDeliveryState {
    Observing,
    NotFound,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptView {
    pub receipt_id: WriteReceiptId,
    pub delivery: ReceiptDeliveryState,
    pub latest: Option<ReceiptSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderWriteProposalView {
    pub operation: ProviderOperationId,
    pub approval_id: Arc<str>,
    pub principal: Principal,
    pub session: SessionId,
    pub account: nmp_native_runtime_core::AccountRef,
    pub draft: BoundedJson,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceView {
    pub id: Arc<str>,
    pub definition: BoundedJson,
    pub retained_receipts: Vec<WriteReceiptId>,
    pub assigned_builds: Vec<Principal>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstalledBuildAvailability {
    /// Verified metadata survived restart, but no live immutable artifact
    /// handle currently proves that the sealed bytes are available offline.
    MetadataOnly,
    /// The runtime holds a verifier-produced immutable handle for this exact
    /// aggregate and can launch without resolving mutable network state.
    SealedExactBytesReady,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledBuildView {
    pub build: InstalledBuild,
    pub availability: InstalledBuildAvailability,
    pub active_sessions: Vec<SessionId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledLibraryView {
    pub query: Arc<str>,
    pub total_installed: usize,
    pub builds: Vec<InstalledBuildView>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionPlatformAvailability {
    Available,
    Unknown { reason: Arc<str> },
    Unavailable { reason: Arc<str> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionCapabilityView {
    pub capability: Capability,
    pub requirement: CapabilityRequirement,
    pub sensitivity: Option<Sensitivity>,
    pub dependencies: Vec<Capability>,
    pub platform_availability: PermissionPlatformAvailability,
    pub current_decision: GrantDecision,
    pub requested_decision: Option<GrantDecision>,
    pub decision_options: Vec<PermissionDecisionOption>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionDecisionOption {
    pub decision: GrantDecision,
    pub valid: bool,
    pub invalid_reason: Option<Arc<str>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionReviewView {
    pub principal: Principal,
    pub title: Arc<str>,
    pub capabilities: Vec<PermissionCapabilityView>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionDecision {
    pub capability: Capability,
    pub decision: GrantDecision,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PermissionReviewError {
    #[error("permission target is not an installed exact build")]
    NotInstalled,
    #[error("persistent grant state could not be read: {detail}")]
    Store { detail: Arc<str> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppSnapshot {
    pub revision: u64,
    pub closed: bool,
    pub library: InstalledLibraryView,
    pub sessions: Vec<SessionSnapshot>,
    pub session_domains: Vec<SessionDomainView>,
    pub provider_push_lanes: Vec<ProviderPushLaneView>,
    pub bindings: Vec<BindingView>,
    pub pending_writes: Vec<ProviderWriteProposalView>,
    pub receipts: Vec<ReceiptView>,
    pub workspaces: Vec<WorkspaceView>,
    pub resources: ResourceCensus,
    /// Bounded tails. Each `dropped_*` is the cumulative number of facts the
    /// ring evicted to stay bounded — those facts are gone, not merely left
    /// out of this view — so zero means the list beside it is complete.
    pub recent_activity: Vec<ActivityFact>,
    pub dropped_activity: u64,
    pub recent_errors: Vec<AppErrorFact>,
    pub dropped_errors: u64,
}
