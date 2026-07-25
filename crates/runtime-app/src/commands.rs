//! Semantic platform inputs and the bounded event projection returned to
//! native shells.

use std::{collections::BTreeSet, sync::Arc};

use nmp_native_nap_bridge::{ProviderPushTermination, SourceWindowId};
use nmp_native_runtime_core::{
    ApprovedWrite, BindingRequest, BoundedJson, Capability, ExecutionProfile, GrantDecision,
    Principal, Sensitivity, SessionId, SessionSnapshot, WriteReceiptId,
};
use nmp_native_runtime_store::{
    InstalledBuild, UninstallCleanupPolicy, UninstallReport, WorkspaceRecord,
};

use crate::{
    limits::ExecutableArtifact,
    views::{AppErrorFact, PermissionDecision},
};

/// Commands are semantic platform inputs. No mapped-message command accepts a
/// principal, profile, grant, or account chosen by untrusted content.
#[derive(Debug)]
pub enum PlatformCommand {
    InstallVerified {
        build: InstalledBuild,
        artifact: Arc<dyn ExecutableArtifact>,
    },
    SetLibraryFilter {
        query: Arc<str>,
    },
    Uninstall {
        principal: Principal,
        cleanup: UninstallCleanupPolicy,
    },
    SetGrant {
        principal: Principal,
        capability: Capability,
        sensitivity: Sensitivity,
        decision: GrantDecision,
    },
    ApplyPermissionBatch {
        principal: Principal,
        decisions: Vec<PermissionDecision>,
    },
    Revoke {
        principal: Principal,
        capability: Capability,
    },
    Launch {
        principal: Principal,
        profile: ExecutionProfile,
        required_domains: BTreeSet<Capability>,
    },
    Stop {
        session: SessionId,
    },
    Suspend {
        session: SessionId,
    },
    Resume {
        session: SessionId,
    },
    Crash {
        session: SessionId,
        reason: Arc<str>,
    },
    MappedEnvelope {
        session: SessionId,
        bytes: Arc<[u8]>,
    },
    CompleteProviderOperation {
        operation: ProviderOperationId,
    },
    OpenBinding {
        request: BindingRequest,
    },
    CloseBinding {
        binding_id: Arc<str>,
    },
    ApproveWrite {
        write: ApprovedWrite,
    },
    DecideProviderWrite {
        operation: ProviderOperationId,
        approve: bool,
    },
    SaveWorkspace {
        workspace: WorkspaceRecord,
    },
    AssignWorkspaceBuild {
        workspace_id: Arc<str>,
        principal: Principal,
    },
    RemoveWorkspaceBuild {
        workspace_id: Arc<str>,
        principal: Principal,
    },
    RestoreWorkspaces,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderOperationId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlatformEvent {
    Installed {
        principal: Principal,
    },
    LibraryFilterChanged {
        query: Arc<str>,
    },
    Uninstalled {
        principal: Principal,
        cleanup: UninstallReport,
    },
    GrantChanged {
        principal: Principal,
        capability: Capability,
        decision: GrantDecision,
    },
    PermissionBatchApplied {
        principal: Principal,
        decisions: Vec<PermissionDecision>,
    },
    SessionChanged(SessionSnapshot),
    EnvelopeHandled {
        session: SessionId,
        operation: Option<ProviderOperationId>,
        response: Option<BoundedJson>,
    },
    EnvelopeIgnored {
        session: SessionId,
    },
    ProviderOperationFinished {
        operation: ProviderOperationId,
    },
    ProviderPush {
        session: SessionId,
        source_window: SourceWindowId,
        provider_sequence: u64,
        domain: Capability,
        envelope: BoundedJson,
    },
    ProviderPushLaneClosed {
        session: SessionId,
        source_window: SourceWindowId,
        termination: Option<ProviderPushTermination>,
    },
    BindingOpened {
        binding_id: Arc<str>,
        logical_source_id: Arc<str>,
    },
    BindingClosed {
        binding_id: Arc<str>,
    },
    WriteAccepted {
        receipt_id: WriteReceiptId,
        frozen_account: nmp_native_runtime_core::AccountRef,
    },
    WorkspaceSaved {
        workspace_id: Arc<str>,
    },
    WorkspaceRestored {
        workspace_id: Arc<str>,
    },
    WorkspaceAssignmentChanged {
        workspace_id: Arc<str>,
        principal: Principal,
        assigned: bool,
    },
    ReceiptReattached {
        receipt_id: WriteReceiptId,
    },
    ReceiptNotFound {
        receipt_id: WriteReceiptId,
    },
    Refused(AppErrorFact),
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequencedPlatformEvent {
    pub sequence: u64,
    pub event: PlatformEvent,
}

#[derive(Debug)]
pub struct EventBatch {
    pub oldest_available: u64,
    pub newest_available: u64,
    pub events: Vec<SequencedPlatformEvent>,
    pub cursor_was_stale: bool,
    /// Events evicted between the caller's cursor and `oldest_available`, i.e.
    /// `oldest_available - cursor - 1`. Zero when the cursor is still live.
    pub lost_before_batch: u64,
}
