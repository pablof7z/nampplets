//! UniFFI projection for the native napplet runtime.
//!
//! This crate is the only unsafe/native ABI boundary in the runtime
//! workspace.  It exports sealed verified-artifact handles and one
//! Rust-owned controller; native callers cannot construct principals or
//! smuggle session authority through napplet envelopes.

use nmp_native_runtime_core::CapabilityRequirement;

mod catalog;
mod config;
mod controller;
mod diagnostics;
mod native_capabilities;
mod permission_types;
mod projection;
mod snapshot_types;
mod support;
mod types;
mod workspace;

#[cfg(test)]
mod tests;

pub use catalog::{
    RuntimeCatalogCancellationResult, RuntimeCatalogCapability, RuntimeCatalogConfirmation,
    RuntimeCatalogConfirmationResult, RuntimeCatalogEntry, RuntimeCatalogFailure,
    RuntimeCatalogFeedSnapshot, RuntimeCatalogLookupState, RuntimeCatalogPage,
    RuntimeCatalogPageResult, RuntimeCatalogProvenance, RuntimeCatalogReview,
    RuntimeCatalogReviewResult, RuntimeCatalogShortfall, RuntimeCatalogSource,
    RuntimeCatalogSourceAccess, RuntimeCatalogSourceState, RuntimeCatalogWindowState,
};
pub use config::{RuntimeConfig, RuntimeOpenError, RuntimePermissionMode};
pub use controller::RuntimeController;
pub use diagnostics::{
    RuntimeRelayAccess, RuntimeRelayCoverage, RuntimeRelayDiagnostics,
    RuntimeRelayDiagnosticsObservation, RuntimeRelayDiagnosticsObservationStart,
    RuntimeRelayDiagnosticsObserver, RuntimeRelayDiagnosticsSnapshot, RuntimeRelayKindCount,
    RuntimeRelayLane, RuntimeRelayLaneCount, RuntimeRelaySubscription,
};
pub use native_capabilities::{
    ArtifactFetchRequest, ArtifactFetchResponse, ArtifactSource, NativeAppearanceSnapshot,
    NativeAppearanceSource, NativeIncActionEnd, NativeIncActionEnqueueResult,
    NativeIncActionExecutor, NativeIncActionRequest, NativeSettingsExecutor,
    NativeSettingsOpenResult, NativeSettingsRequest,
};
pub use permission_types::{
    RuntimeExecutionProfile, RuntimeGrantDecision, RuntimePermissionBatchUpdate,
    RuntimePermissionCapabilitySnapshot, RuntimePermissionDecisionBatch,
    RuntimePermissionDecisionOption, RuntimePermissionDecisionSelection,
    RuntimePermissionExistingDecision, RuntimePermissionPlatformAvailability,
    RuntimePermissionRequirement, RuntimePermissionReviewResult, RuntimePermissionReviewSnapshot,
    RuntimePermissionSensitivity, RuntimeSensitivity,
};
pub use snapshot_types::{
    ObservationStart, RuntimeActivitySnapshot, RuntimeBindingSnapshot, RuntimeErrorSnapshot,
    RuntimeEvent, RuntimeExactBuildCoordinate, RuntimeInstalledBuildAvailability,
    RuntimeInstalledBuildSnapshot, RuntimeInstalledLibrarySnapshot, RuntimeObservation,
    RuntimeObservationFrame, RuntimeObserver, RuntimePendingWriteSnapshot, RuntimeReceiptSnapshot,
    RuntimeSessionSnapshot, RuntimeSnapshot, RuntimeWorkspaceAxis, RuntimeWorkspaceDefinition,
    RuntimeWorkspaceRenderer, RuntimeWorkspaceRestore, RuntimeWorkspaceRole, RuntimeWorkspaceSlot,
    RuntimeWorkspaceUpdate,
};
pub use types::{
    ArtifactCoordinate, ArtifactExecutionMode, ArtifactVerification, NativeConfigCommit,
    RuntimeAccountFailure, RuntimeAccountHandle, RuntimeAccountKind, RuntimeAccountSnapshot,
    RuntimeAccountUpdate, RuntimeProviderUpdate, RuntimeRefusal, VerifiedArtifact, VerifiedRead,
};

pub(crate) const DEFAULT_MAXIMUM_CONFIG_STRING_BYTES: u64 = 16 * 1_024;
pub(crate) const DEFAULT_MAXIMUM_CONFIG_ITEMS: u64 = 64;
pub(crate) const DEFAULT_MAXIMUM_MANIFEST_BYTES: u64 = 256 * 1_024;
pub(crate) const DEFAULT_MAXIMUM_ARTIFACT_READ_BYTES: u64 = 8 * 1_024 * 1_024;
pub(crate) const DEFAULT_MAXIMUM_OBSERVERS: u64 = 8;
pub(crate) const DEFAULT_MAXIMUM_BOUNDARY_EVENTS: u64 = 256;
pub(crate) const WORKSPACE_SCHEMA_VERSION: u16 = 1;
pub(crate) const MAXIMUM_WORKSPACE_SLOTS: usize = 16;
pub(crate) const MAXIMUM_WORKSPACE_JSON_BYTES: usize = 512 * 1_024;
pub(crate) const MAXIMUM_WORKSPACE_FIELD_BYTES: usize = 64 * 1_024;
pub(crate) const MAXIMUM_WORKSPACE_RECEIPTS: usize = 256;
pub(crate) const MAXIMUM_WORKSPACE_POINT_SIZE: u16 = 4_096;
pub(crate) const MAXIMUM_PERMISSION_DECISIONS: usize = 64;
pub(crate) const GOOD_MORNING_AUTHOR: &str =
    "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5";
pub(crate) const GOOD_MORNING_D_TAG: &str = "good-morning";
pub(crate) const GOOD_MORNING_AGGREGATE_HASH: &str =
    "828a6df02afd56782ea20f805084acce65c53f7c37554948c1e0a64aa5a2b0a8";
pub(crate) const GOOD_MORNING_CAPABILITY_PROFILE: &[(&str, CapabilityRequirement)] = &[
    ("identity", CapabilityRequirement::Required),
    ("inc", CapabilityRequirement::Required),
    ("outbox", CapabilityRequirement::Required),
    ("resource", CapabilityRequirement::Optional),
    ("theme", CapabilityRequirement::Optional),
    ("link", CapabilityRequirement::Optional),
];

uniffi::setup_scaffolding!();
