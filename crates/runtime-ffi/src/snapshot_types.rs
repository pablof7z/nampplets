//! Bounded runtime snapshot, event, and observation records.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tokio::sync::watch;

use crate::activity::RuntimeActivitySnapshot;
use crate::{
    RuntimeCatalogFeedSnapshot, RuntimeExecutionProfile, RuntimeRefusal, support::bump_signal,
};

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeSessionSnapshot {
    pub id: u64,
    pub author: String,
    pub d_tag: String,
    pub aggregate_hash: String,
    pub profile: RuntimeExecutionProfile,
    pub state: String,
    /// Exact kernel-negotiated domain set used by both native injection and
    /// the NAP-SHELL `shell.init` response.
    pub domains: Vec<String>,
}

/// Exact installed-build identity. Every library action remains bound to all
/// three coordinate fields; native callers cannot target a publisher/dTag
/// pair without naming the verified aggregate.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeExactBuildCoordinate {
    pub manifest_author: String,
    pub d_tag: String,
    pub aggregate_hash: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeInstalledBuildAvailability {
    /// Verified installation metadata survived, but this process does not
    /// currently retain a verifier-produced immutable artifact handle.
    MetadataOnly,
    /// This process retains the immutable exact-build handle required for an
    /// offline launch.
    SealedExactBytesReady,
}

/// Bounded, screen-shaped installed-build projection. Manifest metadata is
/// opaque verified JSON; native presentation must not reinterpret it as
/// runtime authority.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeInstalledBuildSnapshot {
    pub coordinate: RuntimeExactBuildCoordinate,
    pub title: String,
    pub manifest_metadata_json: String,
    pub availability: RuntimeInstalledBuildAvailability,
    pub active_session_ids: Vec<u64>,
    pub assigned_workspace_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeInstalledLibrarySnapshot {
    pub query: String,
    pub total_installed: u64,
    pub builds: Vec<RuntimeInstalledBuildSnapshot>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeBindingSnapshot {
    pub id: String,
    pub schema: String,
    pub logical_source_id: Option<String>,
    pub revision: Option<u64>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeReceiptSnapshot {
    pub receipt_id: String,
    pub delivery: String,
    pub latest_state_json: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimePendingWriteSnapshot {
    pub operation_id: u64,
    pub approval_id: String,
    pub author: String,
    pub d_tag: String,
    pub aggregate_hash: String,
    pub session_id: u64,
    pub account: String,
    pub draft_json: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeWorkspaceAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeWorkspaceRole {
    Feed,
    Detail,
    Profile,
    Thread,
    Composer,
    MediaPlayer,
    ToolWindow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeWorkspaceRenderer {
    Native,
    LegacyNapplet,
    Surface,
    Unavailable,
}

/// One coarse native workspace slot. Dynamic binding and navigation values
/// remain bounded JSON objects because their schemas belong to the selected
/// handler, while identity, role, renderer, visibility, and layout constraints
/// are typed at this boundary.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeWorkspaceSlot {
    pub slot_id: String,
    pub role: RuntimeWorkspaceRole,
    pub renderer: RuntimeWorkspaceRenderer,
    pub handler_id: String,
    pub manifest_author: Option<String>,
    pub d_tag: Option<String>,
    pub aggregate_hash: Option<String>,
    pub binding_parameters_json: String,
    pub navigation_json: String,
    pub visible: bool,
    pub order: u16,
    pub size_points: u16,
    pub minimum_points: u16,
    pub maximum_points: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeWorkspaceDefinition {
    pub schema_version: u16,
    pub workspace_id: String,
    pub axis: RuntimeWorkspaceAxis,
    pub slots: Vec<RuntimeWorkspaceSlot>,
    pub focused_slot_id: Option<String>,
    pub activity_drawer_visible: bool,
    pub preferences_json: String,
    pub retained_receipt_ids: Vec<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeWorkspaceUpdate {
    pub accepted: bool,
    pub workspace: Option<RuntimeWorkspaceDefinition>,
    pub refusal: Option<RuntimeRefusal>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeWorkspaceRestore {
    pub accepted: bool,
    pub workspaces: Vec<RuntimeWorkspaceDefinition>,
    pub refusal: Option<RuntimeRefusal>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeErrorSnapshot {
    pub code: String,
    pub author: Option<String>,
    pub d_tag: Option<String>,
    pub aggregate_hash: Option<String>,
    pub session_id: Option<u64>,
    pub detail: String,
    pub occurred_at_millis: u64,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeSnapshot {
    pub revision: u64,
    pub closed: bool,
    pub installed_library: RuntimeInstalledLibrarySnapshot,
    pub sessions: Vec<RuntimeSessionSnapshot>,
    pub bindings: Vec<RuntimeBindingSnapshot>,
    pub pending_writes: Vec<RuntimePendingWriteSnapshot>,
    pub receipts: Vec<RuntimeReceiptSnapshot>,
    pub workspaces: Vec<RuntimeWorkspaceDefinition>,
    pub recent_activity: Vec<RuntimeActivitySnapshot>,
    pub dropped_activity: u64,
    pub recent_errors: Vec<RuntimeErrorSnapshot>,
    pub dropped_errors: u64,
    pub boundary_refusals: Vec<RuntimeRefusal>,
    pub dropped_boundary_refusals: u64,
    pub active_resources: u64,
    pub resource_high_watermark: u64,
    pub resource_refusal_count: u64,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeEvent {
    pub sequence: u64,
    pub kind: String,
    pub detail: String,
    pub session_id: Option<u64>,
    pub response_json: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeObservationFrame {
    pub snapshot: RuntimeSnapshot,
    pub catalog: RuntimeCatalogFeedSnapshot,
    pub events: Vec<RuntimeEvent>,
    pub oldest_available_event: u64,
    pub newest_available_event: u64,
    pub event_cursor_was_stale: bool,
    pub lost_before_batch: u64,
}

#[uniffi::export(callback_interface)]
pub trait RuntimeObserver: Send + Sync {
    fn update(&self, frame: RuntimeObservationFrame);
}

#[derive(Debug, uniffi::Object)]
pub struct RuntimeObservation {
    pub(crate) stopped: Arc<AtomicBool>,
    pub(crate) signal: watch::Sender<u64>,
}

#[uniffi::export]
impl RuntimeObservation {
    pub fn stop(&self) {
        if !self.stopped.swap(true, Ordering::AcqRel) {
            bump_signal(&self.signal);
        }
    }
}

impl Drop for RuntimeObservation {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ObservationStart {
    pub observation: Option<Arc<RuntimeObservation>>,
    pub refusal: Option<RuntimeRefusal>,
}
