//! Rust-owned application composition root for the native napplet runtime.
//!
//! The kernel is the single writer for product policy and lifecycle. Platform
//! shells submit commands and render bounded snapshots/events. NMP remains the
//! sole owner of canonical Nostr state and durable write obligations behind
//! [`HostDataPlane`].

mod activity;
mod app;
mod bounded;
mod commands;
mod limits;
mod receipt;
mod views;

pub use activity::{
    ActivityDetail, ActivityDetailValue, ActivityFact, ActivitySensitivity,
    MAXIMUM_ACTIVITY_DETAILS,
};
pub use app::{AppObserver, ObservationClosed, RuntimeApp};
pub use bounded::BoundedFacts;
pub use commands::{
    EventBatch, PlatformCommand, PlatformEvent, ProviderOperationId, SequencedPlatformEvent,
};
pub use limits::{AppLimits, ExecutableArtifact, KernelClock, OpenError, RuntimeAppConfig};
pub use receipt::{AppReceipt, ReceiptObserver};
pub use views::{
    AppErrorCode, AppErrorFact, AppSnapshot, BindingView, InstalledBuildAvailability,
    InstalledBuildView, InstalledLibraryView, PermissionCapabilityView, PermissionDecision,
    PermissionDecisionOption, PermissionPlatformAvailability, PermissionReviewError,
    PermissionReviewView, ProviderPushLaneView, ProviderWriteProposalView, ReceiptDeliveryState,
    ReceiptView, SessionDomainView, WorkspaceView,
};
