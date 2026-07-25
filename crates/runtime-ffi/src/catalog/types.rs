//! Screen-shaped catalog records exchanged across the UniFFI boundary.

use std::sync::Arc;

use nmp_native_artifact::VerifiedArtifactHandle;
use thiserror::Error;

use super::install_eligibility::RuntimeCatalogInstallEligibility;
use crate::{RuntimePermissionRequirement, VerifiedArtifact};

/// One candidate from the current bounded NMP window.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogEntry {
    pub event_id: String,
    pub coordinate: Option<String>,
    pub manifest_author: String,
    pub kind: u16,
    pub created_at: u64,
    pub d_tag: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub aggregate_hash: Option<String>,
    pub observed_sources: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeCatalogSourceAccess {
    Public,
    Nip42 { public_key: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeCatalogSourceState {
    Requesting,
    Connecting,
    Disconnected,
    AwaitingAuth,
    AuthDenied,
    Error,
}

/// Source-scoped evidence. It never implies global completeness.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogSource {
    pub relay: String,
    pub access: RuntimeCatalogSourceAccess,
    pub reconciled_through: Option<u64>,
    pub state: RuntimeCatalogSourceState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeCatalogShortfall {
    NoPlannedSource,
    NoResolvedDemand,
    LocalLimit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeCatalogWindowState {
    Idle,
    Requesting,
    Returned { added: u64 },
    AtBound { maximum: u64 },
    Unknown,
}

/// A finite page for one screen.
///
/// `has_more` means matching rows were omitted by the 100-row screen
/// projection. It does not claim that NMP, a relay, or the network is complete
/// when false.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogPage {
    pub entries: Vec<RuntimeCatalogEntry>,
    pub query_was_local_filter: bool,
    pub locally_filtered_rows: u64,
    pub projection_limited_rows: u64,
    pub refused_rows: u64,
    pub has_more: bool,
    pub window: RuntimeCatalogWindowState,
    pub sources: Vec<RuntimeCatalogSource>,
    pub shortfalls: Vec<RuntimeCatalogShortfall>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeCatalogLookupState {
    Observed { rows: u64 },
    Shortfall { reason: String },
    Selected { event_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogProvenance {
    pub source: String,
    pub state: RuntimeCatalogLookupState,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogCapability {
    pub domain: String,
    pub requirement: RuntimePermissionRequirement,
}

/// An opaque exact review frozen from one verified signed manifest event.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogReview {
    pub token: String,
    pub event_id: String,
    pub coordinate: String,
    pub manifest_author: String,
    pub d_tag: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub aggregate_hash: String,
    pub capabilities: Vec<RuntimeCatalogCapability>,
    pub blob_sources: Vec<String>,
    pub provenance: Vec<RuntimeCatalogProvenance>,
    /// Rust's exact-install decision. Native renders it; it never re-derives
    /// eligibility from `d_tag` or any other raw field above.
    pub install_eligibility: RuntimeCatalogInstallEligibility,
}

/// Screen record paired with the verified handle returned by confirmation.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogConfirmation {
    pub event_id: String,
    pub coordinate: String,
    pub manifest_author: String,
    pub d_tag: Option<String>,
    pub title: Option<String>,
    pub aggregate_hash: String,
    pub capabilities: Vec<RuntimeCatalogCapability>,
    pub provenance: Vec<RuntimeCatalogProvenance>,
}

#[derive(Clone, Debug)]
pub struct RuntimeCatalogConfirmedArtifact {
    pub(super) handle: VerifiedArtifactHandle,
    pub confirmation: RuntimeCatalogConfirmation,
}

impl RuntimeCatalogConfirmedArtifact {
    pub fn into_handle(self) -> VerifiedArtifactHandle {
        self.handle
    }
}

/// Typed, state-shaped refusal for every catalog boundary operation.
///
/// The controller returns these inside records instead of throwing across the
/// FFI boundary, keeping refusal and cancellation observable native state.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogFailure {
    pub code: String,
    pub detail: String,
    pub provenance: Vec<RuntimeCatalogProvenance>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeCatalogPageResult {
    pub page: Option<RuntimeCatalogPage>,
    pub failure: Option<RuntimeCatalogFailure>,
}

/// Latest replacement from the profile's single permanent NMP catalog feed.
#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeCatalogFeedSnapshot {
    pub revision: u64,
    pub result: RuntimeCatalogPageResult,
    pub closed: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeCatalogReviewResult {
    pub review: Option<RuntimeCatalogReview>,
    pub failure: Option<RuntimeCatalogFailure>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeCatalogConfirmationResult {
    pub confirmation: Option<RuntimeCatalogConfirmation>,
    pub artifact: Option<Arc<VerifiedArtifact>>,
    pub failure: Option<RuntimeCatalogFailure>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeCatalogCancellationResult {
    pub cancelled: bool,
    pub failure: Option<RuntimeCatalogFailure>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum RuntimeCatalogError {
    #[error("catalog configuration is invalid: {reason}")]
    InvalidConfiguration { reason: String },
    #[error("catalog operation capacity is full at {maximum}")]
    Busy { maximum: u64 },
    #[error("catalog operation exceeded its {milliseconds}ms deadline")]
    Deadline { milliseconds: u64 },
    #[error("catalog worker could not start or ended unexpectedly: {reason}")]
    WorkerUnavailable { reason: String },
    #[error("catalog query was refused: {reason}")]
    Browse { reason: String },
    #[error("manifest coordinate is invalid: {reason}")]
    InvalidCoordinate { reason: String },
    #[error("no manifest was selected from the scoped sources")]
    NotFound {
        provenance: Vec<RuntimeCatalogProvenance>,
    },
    #[error("catalog review capacity is full at {maximum}")]
    ReviewCapacity { maximum: u64 },
    #[error("catalog review token is stale")]
    StaleReview,
    #[error("catalog operation was cancelled")]
    Cancelled,
    #[error("catalog resolution failed: {reason}")]
    Resolve { reason: String },
}
