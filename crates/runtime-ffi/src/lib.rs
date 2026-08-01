//! UniFFI projection for the native napplet runtime.
//!
//! This crate is the only unsafe/native ABI boundary in the runtime
//! workspace.  It exports sealed verified-artifact handles and one
//! Rust-owned controller; native callers cannot construct principals or
//! smuggle session authority through napplet envelopes.

mod activity;
mod catalog;
mod catalog_coordinate;
mod config;
mod controller;
mod diagnostics;
mod intent_dispatch;
mod native_capabilities;
mod permission_types;
mod profile_preferences;
mod projection;
mod receipt_projection;
mod relay_lane;
mod slots;
mod snapshot_integrity;
mod snapshot_types;
mod support;
mod types;
mod workspace;

#[cfg(test)]
mod tests;

pub use activity::{RuntimeActivityDetail, RuntimeActivityDetailValue, RuntimeActivitySnapshot};
pub use catalog::{
    RuntimeCatalogCancellationResult, RuntimeCatalogCapability, RuntimeCatalogConfirmation,
    RuntimeCatalogConfirmationResult, RuntimeCatalogEntry, RuntimeCatalogFailure,
    RuntimeCatalogFeedSnapshot, RuntimeCatalogInstallEligibility, RuntimeCatalogLookupState,
    RuntimeCatalogPage, RuntimeCatalogPageResult, RuntimeCatalogProvenance, RuntimeCatalogReview,
    RuntimeCatalogReviewResult, RuntimeCatalogShortfall, RuntimeCatalogSource,
    RuntimeCatalogSourceAccess, RuntimeCatalogSourceState, RuntimeCatalogWindowState,
};
pub use config::{RuntimeConfig, RuntimeOpenError, RuntimePermissionDefault};
pub use controller::RuntimeController;
pub use diagnostics::{
    RuntimeRelayAccess, RuntimeRelayCoverage, RuntimeRelayDiagnostics,
    RuntimeRelayDiagnosticsObservation, RuntimeRelayDiagnosticsObservationStart,
    RuntimeRelayDiagnosticsObserver, RuntimeRelayDiagnosticsSnapshot, RuntimeRelayKindCount,
    RuntimeRelayLane, RuntimeRelayLaneCount, RuntimeRelaySubscription,
};
pub use intent_dispatch::{NativeIntentActivationExecutor, NativeIntentActivationRequest};
pub use native_capabilities::{
    ArtifactFetchRequest, ArtifactFetchResponse, ArtifactSource, NativeAppearanceSnapshot,
    NativeAppearanceSource, NativeIncActionEnd, NativeIncActionEnqueueResult,
    NativeIncActionExecutor, NativeIncActionRequest, NativeSettingsExecutor,
    NativeSettingsOpenResult, NativeSettingsRequest,
};
pub use permission_types::{
    RuntimeExecutionProfile, RuntimeGrantDecision, RuntimePermissionBatchUpdate,
    RuntimePermissionCapabilitySnapshot, RuntimePermissionChangeRefusal,
    RuntimePermissionChangeRefusalCode, RuntimePermissionDecisionBatch,
    RuntimePermissionDecisionController, RuntimePermissionDecisionOption,
    RuntimePermissionDecisionSelection, RuntimePermissionExistingDecision,
    RuntimePermissionPlatformAvailability, RuntimePermissionRequirement,
    RuntimePermissionReviewResult, RuntimePermissionReviewSnapshot, RuntimePermissionSensitivity,
    RuntimeSensitivity,
};
pub use profile_preferences::{
    RuntimeProfilePreferences, RuntimeProfilePreferencesUpdate, RuntimeStorageResetResult,
    RuntimeStorageSnapshot,
};
pub use receipt_projection::project_receipt;
pub use slots::{
    RuntimeReceiptsSlotObservation, RuntimeReceiptsSlotObservationStart,
    RuntimeReceiptsSlotObserver, RuntimeReceiptsSlotProjection, RuntimeReceiptsSlotSnapshot,
};
pub use snapshot_types::{
    ObservationStart, RuntimeBindingSnapshot, RuntimeErrorSnapshot, RuntimeEvent,
    RuntimeExactBuildCoordinate, RuntimeInstalledBuildAvailability, RuntimeInstalledBuildSnapshot,
    RuntimeInstalledLibrarySnapshot, RuntimeObservation, RuntimeObservationFrame, RuntimeObserver,
    RuntimePendingWriteSnapshot, RuntimeReceiptObservationLifecycle, RuntimeReceiptOutcome,
    RuntimeReceiptSnapshot, RuntimeSessionSnapshot, RuntimeSnapshot, RuntimeSnapshotProjection,
    RuntimeWorkspaceAxis, RuntimeWorkspaceDefinition, RuntimeWorkspaceRenderer,
    RuntimeWorkspaceRestore, RuntimeWorkspaceRole, RuntimeWorkspaceSlot, RuntimeWorkspaceUpdate,
};
pub use types::{
    ArtifactCoordinate, ArtifactExecutionMode, ArtifactVerification, NativeConfigCommit,
    RuntimeAccountFailure, RuntimeAccountHandle, RuntimeAccountKind, RuntimeAccountSnapshot,
    RuntimeAccountUpdate, RuntimeProviderUpdate, RuntimeRefusal, VerifiedArtifact, VerifiedRead,
};

pub(crate) const DEFAULT_MAXIMUM_CONFIG_STRING_BYTES: u64 = 16 * 1_024;
pub(crate) const DEFAULT_MAXIMUM_CONFIG_ITEMS: u64 = 64;
pub(crate) const DEFAULT_MAXIMUM_MANIFEST_BYTES: u64 = 256 * 1_024;
pub(crate) const MAXIMUM_INSTALLED_MANIFEST_METADATA_BYTES: usize = 512 * 1_024;
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
uniffi::setup_scaffolding!();
