//! Bounded read-only projections rendered by native shells.

use std::sync::Arc;

use nmp_native_nap_bridge::SourceWindowId;
use nmp_native_runtime_core::{
    BoundedJson, Capability, CapabilityRequirement, GrantDecision, Principal, ReceiptSnapshot,
    ResourceCensus, Sensitivity, SessionId, SessionSnapshot, WriteReceiptId,
};
use nmp_native_runtime_store::InstalledBuild;
use thiserror::Error;

use crate::activity::ActivityFact;
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
    /// Domains this build's own signed content requires that no provider on
    /// this runtime advertises. Non-empty means the session is running without
    /// something it declared it needs.
    ///
    /// Carried as a set rather than a message. The shortfall used to exist
    /// only as a comma-joined string inside one activity fact, which no
    /// consumer could act on and every consumer could miss.
    pub unavailable_domains: Vec<Capability>,
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
pub enum PermissionDecisionController {
    User,
    HostPolicy { reason: Arc<str> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionCapabilityView {
    pub capability: Capability,
    pub requirement: CapabilityRequirement,
    pub sensitivity: Option<Sensitivity>,
    pub dependencies: Vec<Capability>,
    pub platform_availability: PermissionPlatformAvailability,
    pub controller: PermissionDecisionController,
    pub current_decision: GrantDecision,
    /// True when the decision in force already allows this capability without
    /// prompting. This is the runtime's own classification of "granted"; it is
    /// not a list of decision names for a caller to match against.
    pub is_granted: bool,
    pub requested_decision: Option<GrantDecision>,
    /// The decision the runtime recommends when the user accepts this
    /// capability without choosing a scope: the broadest currently valid
    /// affirmative decision, `Denied` when nothing affirmative is valid on
    /// this platform, and `None` when host policy manages the capability and
    /// the user has no decision to make.
    pub recommended_decision: Option<GrantDecision>,
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
    pub revision: Arc<str>,
    pub title: Arc<str>,
    pub capabilities: Vec<PermissionCapabilityView>,
    pub read_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionDecision {
    pub capability: Capability,
    pub decision: GrantDecision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionChangeRefusalCode {
    Closed,
    NotInstalled,
    StaleReview,
    EmptyChanges,
    DuplicateCapability,
    UnknownCapability,
    ManagedCapability,
    InvalidDecision,
    DecisionUnavailable,
    DependencyDenied,
    Grant,
    Store,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionChangeRefusal {
    pub code: PermissionChangeRefusalCode,
    pub detail: Arc<str>,
    pub current_review: Option<Box<PermissionReviewView>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionChangeSuccess {
    pub changed: bool,
    pub review: PermissionReviewView,
}

pub type PermissionChangeResult = Result<PermissionChangeSuccess, PermissionChangeRefusal>;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PermissionReviewError {
    #[error("permission target is not an installed exact build")]
    NotInstalled,
    #[error("persistent grant state could not be read: {detail}")]
    Store { detail: Arc<str> },
}

/// Monotonic per-section revisions for the producer snapshot.
///
/// An unchanged revision between two published snapshots proves that the
/// section's content is unchanged. An advance means consumers must re-read the
/// section; consumers must not require every advance to represent a difference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SectionRevisions {
    pub library: u64,
    /// Sessions and their immutable launch-time domains form one section.
    pub sessions: u64,
    /// Live provider-push delivery state, intentionally separate from sessions.
    pub provider_push_lanes: u64,
    pub bindings: u64,
    pub pending_writes: u64,
    pub receipts: u64,
    pub workspaces: u64,
    pub resources: u64,
    pub activity: u64,
    pub errors: u64,
    /// The platform event concern's revision is its existing replay cursor.
    pub newest_event_sequence: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotSection {
    FusedSnapshot,
    Library,
    Sessions,
    ProviderPushLanes,
    Bindings,
    PendingWrites,
    Receipts,
    Workspaces,
    Resources,
    Activity,
    Errors,
    NewestEventSequence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppTerminalReason {
    SectionRevisionExhausted { section: SnapshotSection },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppSnapshot {
    pub revision: u64,
    pub revisions: SectionRevisions,
    pub closed: bool,
    /// Rust-owned lifecycle evidence, always read outside section gating.
    pub terminal_reason: Option<AppTerminalReason>,
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
