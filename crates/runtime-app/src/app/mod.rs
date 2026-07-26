//! `RuntimeApp` composition root: shared kernel state plus the platform-facing
//! observation, snapshot, and event-replay surface.
//!
//! The kernel is the single writer for product policy and lifecycle. Concern
//! modules in this directory extend `impl RuntimeApp` against the shared
//! [`AppState`] owned here.

mod binding;
mod envelope;
mod facts;
mod install;
mod observe;
mod permissions;
mod push;
mod revisions;
mod session;
mod terminal;
mod workspace;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    thread::JoinHandle,
};

use nmp_native_nap_bridge::{
    ActivitySink, InjectionPlan, Provider, ProviderOperation, ProviderPushObserver,
    ProviderRegistry, ProviderSessionEnd, ProviderWriteProposal, SessionContext, SourceWindowId,
};
use nmp_native_providers::ShellProvider;
use nmp_native_runtime_core::{
    BindingRequest, Capability, GrantLedger, HostDataPlane, Principal, ResourceTracker, Session,
    SessionId, SessionState, WorkLease, WriteReceiptId,
};
use nmp_native_runtime_store::{
    InstalledBuild, PermissionDefaultPreference, RuntimeStore, WorkspaceRecord,
};
use nmp_native_surface::{Binding, BindingLimits};
use parking_lot::Mutex;
use thiserror::Error;
use tokio::sync::watch;

use self::install::installed_library_view;
use crate::{
    activity::ActivityFact,
    bounded::BoundedFacts,
    commands::{EventBatch, PlatformCommand, ProviderOperationId, SequencedPlatformEvent},
    limits::{AppLimits, ExecutableArtifact, KernelClock, OpenError, RuntimeAppConfig},
    receipt::{AppReceipt, NoopBridgeActivity},
    views::{AppErrorCode, AppErrorFact, AppSnapshot, AppTerminalReason, SectionRevisions},
};

#[derive(Debug)]
pub struct AppObserver {
    receiver: watch::Receiver<Arc<AppSnapshot>>,
}

impl AppObserver {
    pub fn latest(&self) -> Arc<AppSnapshot> {
        Arc::clone(&self.receiver.borrow())
    }

    pub async fn changed(&mut self) -> Result<Arc<AppSnapshot>, ObservationClosed> {
        self.receiver
            .changed()
            .await
            .map_err(|_| ObservationClosed)?;
        Ok(self.latest())
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("application observation is closed")]
pub struct ObservationClosed;

#[derive(Debug)]
pub struct RuntimeApp {
    limits: AppLimits,
    binding_limits: BindingLimits,
    resources: Arc<ResourceTracker>,
    grants: Arc<GrantLedger>,
    bridge: ProviderRegistry,
    shell_provider: Arc<ShellProvider>,
    mapped_routes: BTreeSet<(Capability, Arc<str>)>,
    store: Arc<RuntimeStore>,
    data_plane: Arc<dyn HostDataPlane>,
    clock: Arc<dyn KernelClock>,
    permission_default: PermissionDefaultPreference,
    state: Mutex<AppState>,
    snapshots: watch::Sender<Arc<AppSnapshot>>,
}

#[derive(Debug)]
pub(crate) struct AppState {
    next_session_id: u64,
    next_source_window_id: u64,
    next_operation_id: u64,
    next_event_sequence: u64,
    revision: u64,
    closed: bool,
    terminal_reason: Option<AppTerminalReason>,
    library_query: Arc<str>,
    installed: BTreeMap<Principal, InstalledBuild>,
    artifacts: BTreeMap<Principal, Arc<dyn ExecutableArtifact>>,
    sessions: BTreeMap<SessionId, SessionEntry>,
    operations: BTreeMap<ProviderOperationId, ActiveOperation>,
    bindings: BTreeMap<Arc<str>, BindingOwner>,
    receipts: BTreeMap<WriteReceiptId, Arc<AppReceipt>>,
    workspaces: BTreeMap<Arc<str>, WorkspaceRecord>,
    workspace_assignments: BTreeMap<Arc<str>, BTreeSet<Principal>>,
    activity: BoundedFacts<ActivityFact>,
    errors: BoundedFacts<AppErrorFact>,
    events: BoundedFacts<SequencedPlatformEvent>,
}

#[derive(Debug)]
pub(crate) struct SessionEntry {
    session: Arc<Session>,
    context: SessionContext,
    plan: InjectionPlan,
    source_window: SourceWindowId,
    push_observer: Option<ProviderPushObserver>,
    push_delivery: Option<ProviderPushDelivery>,
    /// Required domains no provider advertises, kept for the life of the
    /// session. Not an `Option`: every launch either has a shortfall or has an
    /// empty one, and there is no third state where the answer is unknown.
    pub(crate) unavailable_domains: BTreeSet<Capability>,
    ready: bool,
    last_provider_sequence: Option<u64>,
    delivered_push_count: u64,
    _artifact: Arc<dyn ExecutableArtifact>,
    _webview: WorkLease,
}

#[derive(Debug)]
pub(crate) struct ProviderPushDelivery {
    join: Option<JoinHandle<()>>,
}

#[derive(Debug)]
pub(crate) struct ActiveOperation {
    session: SessionId,
    principal: Principal,
    domain: Capability,
    handle: Option<ProviderOperation>,
    proposal: Option<ProviderWriteProposal>,
}

impl ActiveOperation {
    fn cancel(self, reason: Arc<str>) {
        if let Some(proposal) = self.proposal {
            proposal.refuse(reason);
        }
        if let Some(handle) = self.handle {
            handle.cancel();
        }
    }

    fn complete(self) {
        drop(self.proposal);
        if let Some(handle) = self.handle {
            handle.complete();
        }
    }
}

#[derive(Debug)]
pub(crate) struct BindingOwner {
    request: BindingRequest,
    binding: Arc<Binding>,
}

impl RuntimeApp {
    pub fn open(config: RuntimeAppConfig) -> Result<Arc<Self>, OpenError> {
        let limits = config.limits.validate()?;
        let installed = config
            .store
            .installed_builds()?
            .into_iter()
            .map(|build| (build.principal.clone(), build))
            .collect::<BTreeMap<_, _>>();
        if installed.len() > limits.maximum_installed_artifacts {
            return Err(OpenError::InstalledLibraryCapacity {
                actual: installed.len(),
                maximum: limits.maximum_installed_artifacts,
            });
        }
        let resources = Arc::new(ResourceTracker::new(config.resource_limits)?);
        let grants = Arc::new(GrantLedger::new(
            config.grant_limits,
            Arc::clone(&resources),
        )?);
        let activity_sink: Arc<dyn ActivitySink> = Arc::new(NoopBridgeActivity);
        let mut bridge = ProviderRegistry::new(
            config.bridge_limits,
            Arc::clone(&resources),
            Arc::clone(&grants),
            activity_sink,
        )?;
        let shell_provider = config.shell_provider;
        let mut mapped_routes = shell_provider
            .descriptor()
            .actions
            .iter()
            .cloned()
            .map(|action| (shell_provider.descriptor().domain.clone(), action))
            .collect::<BTreeSet<_>>();
        let registered_shell: Arc<dyn Provider> = shell_provider.clone();
        bridge.register(registered_shell)?;
        for provider in config.providers {
            mapped_routes.extend(
                provider
                    .descriptor()
                    .actions
                    .iter()
                    .cloned()
                    .map(|action| (provider.descriptor().domain.clone(), action)),
            );
            bridge.register(provider)?;
        }
        let initial = Arc::new(AppSnapshot {
            revision: 0,
            revisions: SectionRevisions::default(),
            closed: false,
            terminal_reason: None,
            library: installed_library_view(&installed, &BTreeMap::new(), &BTreeMap::new(), ""),
            sessions: Vec::new(),
            session_domains: Vec::new(),
            provider_push_lanes: Vec::new(),
            bindings: Vec::new(),
            pending_writes: Vec::new(),
            receipts: Vec::new(),
            workspaces: Vec::new(),
            resources: resources.census(),
            recent_activity: Vec::new(),
            dropped_activity: 0,
            recent_errors: Vec::new(),
            dropped_errors: 0,
        });
        let (snapshots, _) = watch::channel(initial);
        Ok(Arc::new(Self {
            limits,
            binding_limits: config.binding_limits,
            resources,
            grants,
            bridge,
            shell_provider,
            mapped_routes,
            store: config.store,
            data_plane: config.data_plane,
            clock: config.clock,
            permission_default: config.permission_default,
            state: Mutex::new(AppState {
                next_session_id: 0,
                next_source_window_id: 0,
                next_operation_id: 0,
                next_event_sequence: 0,
                revision: 0,
                closed: false,
                terminal_reason: None,
                library_query: Arc::from(""),
                installed,
                artifacts: BTreeMap::new(),
                sessions: BTreeMap::new(),
                operations: BTreeMap::new(),
                bindings: BTreeMap::new(),
                receipts: BTreeMap::new(),
                workspaces: BTreeMap::new(),
                workspace_assignments: BTreeMap::new(),
                activity: BoundedFacts::with_capacity(limits.maximum_activity_facts),
                errors: BoundedFacts::with_capacity(limits.maximum_error_facts),
                events: BoundedFacts::with_capacity(limits.maximum_platform_events),
            }),
            snapshots,
        }))
    }

    /// Fire-and-observe command boundary. Operation success and failure are
    /// projected through [`PlatformEvent`] and [`AppSnapshot`], never returned
    /// to the native renderer as product control flow.
    pub fn dispatch(self: &Arc<Self>, command: PlatformCommand) {
        let now = self.clock.now_millis();
        let mut state = self.state.lock();
        let mut delivery_joins = Vec::new();
        if state.terminal_reason.is_some() {
            return;
        }
        if !self.preflight_revision_capacity(&mut state) {
            return;
        }
        if state.closed && !matches!(command, PlatformCommand::Close) {
            self.refuse(
                &mut state,
                AppErrorCode::Closed,
                None,
                None,
                "runtime is closed",
                now,
            );
            self.publish(&mut state);
            return;
        }

        match command {
            PlatformCommand::InstallVerified { build, artifact } => {
                self.install_verified(&mut state, build, artifact, now);
            }
            PlatformCommand::SetLibraryFilter { query } => {
                self.set_library_filter(&mut state, query, now);
            }
            PlatformCommand::Uninstall { principal, cleanup } => {
                delivery_joins.extend(self.uninstall(&mut state, principal, cleanup, now));
            }
            PlatformCommand::SetGrant {
                principal,
                capability,
                sensitivity,
                decision,
            } => self.set_grant(
                &mut state,
                principal,
                capability,
                sensitivity,
                decision,
                now,
            ),
            PlatformCommand::ApplyPermissionChanges(request) => {
                let _ = self.apply_permission_changes_locked(&mut state, request, now);
            }
            PlatformCommand::Revoke {
                principal,
                capability,
            } => self.revoke(&mut state, principal, capability, now),
            PlatformCommand::Launch {
                principal,
                profile,
                required_domains,
            } => self.launch(&mut state, principal, profile, required_domains, now),
            PlatformCommand::Stop { session } => {
                if let Some(join) =
                    self.end_session(&mut state, session, SessionState::Stopped, None, now)
                {
                    delivery_joins.push(join);
                }
            }
            PlatformCommand::Suspend { session } => {
                self.transition_session(&mut state, session, SessionState::Suspended, now);
            }
            PlatformCommand::Resume { session } => {
                self.transition_session(&mut state, session, SessionState::Running, now);
            }
            PlatformCommand::Crash { session, reason } => {
                if let Some(join) = self.end_session(
                    &mut state,
                    session,
                    SessionState::Crashed,
                    Some(reason),
                    now,
                ) {
                    delivery_joins.push(join);
                }
            }
            PlatformCommand::MappedEnvelope { session, bytes } => {
                if let Some(join) = self.dispatch_envelope(&mut state, session, &bytes, now) {
                    delivery_joins.push(join);
                }
            }
            PlatformCommand::CompleteProviderOperation { operation } => {
                self.complete_operation(&mut state, operation, now);
            }
            PlatformCommand::OpenBinding { request } => {
                self.open_binding(&mut state, request, now);
            }
            PlatformCommand::CloseBinding { binding_id } => {
                self.close_binding(&mut state, &binding_id, now);
            }
            PlatformCommand::ApproveWrite { write } => {
                self.approve_write(&mut state, write, now);
            }
            PlatformCommand::DecideProviderWrite { operation, approve } => {
                self.decide_provider_write(&mut state, operation, approve, now);
            }
            PlatformCommand::SaveWorkspace { workspace } => {
                self.save_workspace(&mut state, workspace, now);
            }
            PlatformCommand::AssignWorkspaceBuild {
                workspace_id,
                principal,
            } => self.assign_workspace_build(&mut state, workspace_id, principal, true, now),
            PlatformCommand::RemoveWorkspaceBuild {
                workspace_id,
                principal,
            } => self.assign_workspace_build(&mut state, workspace_id, principal, false, now),
            PlatformCommand::RestoreWorkspaces => self.restore_workspaces(&mut state, now),
            PlatformCommand::Close => delivery_joins.extend(self.close(&mut state, now)),
        }
        drop(state);
        for join in delivery_joins {
            let _ = join.join();
        }
        let mut state = self.state.lock();
        self.publish(&mut state);
    }

    pub fn observe(&self) -> AppObserver {
        AppObserver {
            receiver: self.snapshots.subscribe(),
        }
    }

    pub fn snapshot(&self) -> Arc<AppSnapshot> {
        Arc::clone(&self.snapshots.borrow())
    }

    pub fn binding(&self, binding_id: &str) -> Option<Arc<Binding>> {
        self.state
            .lock()
            .bindings
            .get(binding_id)
            .map(|owner| Arc::clone(&owner.binding))
    }

    pub fn receipt(&self, receipt_id: &WriteReceiptId) -> Option<Arc<AppReceipt>> {
        self.state.lock().receipts.get(receipt_id).cloned()
    }

    /// Finite event replay. A stale cursor is observable, carries the exact
    /// number of events lost before the batch (`oldest_available - cursor - 1`),
    /// and the caller must resynchronize from the current bounded snapshot.
    pub fn events_after(&self, sequence: u64) -> EventBatch {
        let state = self.state.lock();
        let oldest_available = state
            .events
            .front()
            .map_or(state.next_event_sequence, |item| item.sequence);
        let newest_available = state
            .events
            .back()
            .map_or(state.next_event_sequence, |item| item.sequence);
        let cursor_was_stale = sequence.saturating_add(1) < oldest_available;
        let events = if cursor_was_stale {
            Vec::new()
        } else {
            state
                .events
                .iter()
                .filter(|item| item.sequence > sequence)
                .cloned()
                .collect()
        };
        EventBatch {
            oldest_available,
            newest_available,
            events,
            cursor_was_stale,
            lost_before_batch: oldest_available.saturating_sub(sequence).saturating_sub(1),
        }
    }
}

impl Drop for RuntimeApp {
    fn drop(&mut self) {
        let state = self.state.get_mut();
        state.operations.clear();
        let mut delivery_joins = Vec::new();
        for (session_id, mut entry) in std::mem::take(&mut state.sessions) {
            self.shell_provider.close_session(session_id);
            self.bridge
                .close_session_with_reason(session_id, ProviderSessionEnd::RuntimeClosed);
            entry.session.stop();
            if let Some(join) = entry
                .push_delivery
                .take()
                .and_then(|mut delivery| delivery.join.take())
            {
                delivery_joins.push(join);
            }
        }
        for join in delivery_joins {
            let _ = join.join();
        }
        for (_, owner) in std::mem::take(&mut state.bindings) {
            owner.binding.close();
        }
        for (_, receipt) in std::mem::take(&mut state.receipts) {
            receipt.stop_delivery();
        }
        state.artifacts.clear();
        state.closed = true;
    }
}
