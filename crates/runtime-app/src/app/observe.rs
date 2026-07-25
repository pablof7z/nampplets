//! Snapshot publication plus the bounded activity, event, and refusal rings.

use std::sync::Arc;

use nmp_native_nap_bridge::BridgeError;
use nmp_native_runtime_core::{Principal, SessionError, SessionId};
use nmp_native_runtime_store::{ActivityRecord, StoreError};
use nmp_native_surface::BindingError;

use super::{AppState, RuntimeApp, install::installed_library_view};
use crate::activity::{ActivityDetail, ActivityFact};
use crate::{
    commands::{PlatformEvent, SequencedPlatformEvent},
    views::{
        AppErrorCode, AppErrorFact, AppSnapshot, BindingView, ProviderPushLaneView,
        ProviderWriteProposalView, SessionDomainView, WorkspaceView,
    },
};

impl RuntimeApp {
    pub(super) fn publish(&self, state: &mut AppState) {
        state.revision = state.revision.saturating_add(1);
        let snapshot = Arc::new(self.build_snapshot(state));
        self.snapshots.send_replace(snapshot);
    }

    pub(super) fn build_snapshot(&self, state: &AppState) -> AppSnapshot {
        AppSnapshot {
            revision: state.revision,
            closed: state.closed,
            library: installed_library_view(
                &state.installed,
                &state.artifacts,
                &state.sessions,
                &state.library_query,
            ),
            sessions: state
                .sessions
                .values()
                .map(|entry| entry.session.snapshot())
                .collect(),
            session_domains: state
                .sessions
                .iter()
                .map(|(session, entry)| SessionDomainView {
                    session: *session,
                    domains: entry.plan.domains().iter().cloned().collect(),
                })
                .collect(),
            provider_push_lanes: state
                .sessions
                .iter()
                .map(|(session, entry)| ProviderPushLaneView {
                    session: *session,
                    source_window: entry.source_window,
                    ready: entry.ready,
                    last_provider_sequence: entry.last_provider_sequence,
                    delivered_count: entry.delivered_push_count,
                })
                .collect(),
            bindings: state
                .bindings
                .iter()
                .map(|(id, owner)| BindingView {
                    id: Arc::clone(id),
                    schema: Arc::clone(&owner.request.schema),
                    logical_source_id: owner.binding.logical_source_id().map(Arc::from),
                    revision: owner.binding.latest().map(|snapshot| snapshot.revision),
                })
                .collect(),
            pending_writes: state
                .operations
                .iter()
                .filter_map(|(operation, active)| {
                    let proposal = active.proposal.as_ref()?;
                    let write = proposal.write.as_ref()?;
                    Some(ProviderWriteProposalView {
                        operation: *operation,
                        approval_id: Arc::clone(&write.approval_id),
                        principal: write.origin_principal.clone(),
                        session: write.origin_session,
                        account: write.account.clone(),
                        draft: write.draft.clone(),
                    })
                })
                .collect(),
            receipts: state
                .receipts
                .values()
                .filter_map(|receipt| receipt.view())
                .collect(),
            workspaces: state
                .workspaces
                .values()
                .map(|workspace| WorkspaceView {
                    id: Arc::clone(&workspace.id),
                    definition: workspace.definition.clone(),
                    retained_receipts: workspace.retained_receipts.clone(),
                    assigned_builds: state
                        .workspace_assignments
                        .get(&workspace.id)
                        .map(|assignments| assignments.iter().cloned().collect())
                        .unwrap_or_default(),
                })
                .collect(),
            resources: self.resources.census(),
            recent_activity: state.activity.iter().cloned().collect(),
            dropped_activity: state.activity.dropped(),
            recent_errors: state.errors.iter().cloned().collect(),
            dropped_errors: state.errors.dropped(),
        }
    }

    pub(super) fn push_event(&self, state: &mut AppState, event: PlatformEvent) {
        state.next_event_sequence = state.next_event_sequence.saturating_add(1);
        let sequence = state.next_event_sequence;
        state.events.push(
            self.limits.maximum_platform_events,
            SequencedPlatformEvent { sequence, event },
        );
    }

    pub(crate) fn record_activity(
        &self,
        state: &mut AppState,
        principal: &Principal,
        category: &str,
        operation: &str,
        outcome: &str,
        now: u64,
    ) {
        self.record_activity_with_details(
            state,
            principal,
            category,
            operation,
            outcome,
            Vec::new(),
            now,
        );
    }

    /// Record one fact together with details the producer already classified.
    ///
    /// Only the retained in-memory fact carries details. The durable
    /// `activity` table stores the three bounded strings it has always
    /// stored; widening that schema belongs to the store workstream, and a
    /// secret detail has no bytes to persist in any case.
    pub(crate) fn record_activity_with_details(
        &self,
        state: &mut AppState,
        principal: &Principal,
        category: &str,
        operation: &str,
        outcome: &str,
        details: Vec<ActivityDetail>,
        now: u64,
    ) {
        let fact = ActivityFact::new(
            principal.clone(),
            category,
            operation,
            outcome,
            details,
            now,
        );
        let persisted = ActivityRecord {
            principal: fact.principal.clone(),
            category: Arc::clone(&fact.category),
            operation: Arc::clone(&fact.operation),
            outcome: Arc::clone(&fact.outcome),
            occurred_at_millis: now,
        };
        state
            .activity
            .push(self.limits.maximum_activity_facts, fact);
        if let Err(error) = self.store.append_activity(&persisted) {
            self.record_error(
                state,
                AppErrorFact {
                    code: AppErrorCode::Store,
                    principal: Some(principal.clone()),
                    session: None,
                    detail: Arc::from(error.to_string()),
                    occurred_at_millis: now,
                },
            );
        }
    }

    pub(super) fn refuse(
        &self,
        state: &mut AppState,
        code: AppErrorCode,
        principal: Option<Principal>,
        session: Option<SessionId>,
        detail: impl Into<Arc<str>>,
        now: u64,
    ) {
        let fact = AppErrorFact {
            code,
            principal,
            session,
            detail: detail.into(),
            occurred_at_millis: now,
        };
        self.record_error(state, fact.clone());
        self.push_event(state, PlatformEvent::Refused(fact));
    }

    pub(super) fn record_error(&self, state: &mut AppState, fact: AppErrorFact) {
        state.errors.push(self.limits.maximum_error_facts, fact);
    }

    pub(super) fn refuse_store(
        &self,
        state: &mut AppState,
        principal: Option<Principal>,
        session: Option<SessionId>,
        error: StoreError,
        now: u64,
    ) {
        self.refuse(
            state,
            AppErrorCode::Store,
            principal,
            session,
            error.to_string(),
            now,
        );
    }

    pub(super) fn refuse_bridge(
        &self,
        state: &mut AppState,
        principal: Option<Principal>,
        session: Option<SessionId>,
        error: BridgeError,
        now: u64,
    ) {
        self.refuse(
            state,
            AppErrorCode::Bridge,
            principal,
            session,
            error.to_string(),
            now,
        );
    }

    pub(super) fn refuse_session(
        &self,
        state: &mut AppState,
        principal: Option<Principal>,
        session: Option<SessionId>,
        error: SessionError,
        now: u64,
    ) {
        self.refuse(
            state,
            AppErrorCode::InvalidLifecycle,
            principal,
            session,
            error.to_string(),
            now,
        );
    }

    pub(super) fn refuse_binding(&self, state: &mut AppState, error: BindingError, now: u64) {
        self.refuse(
            state,
            AppErrorCode::Binding,
            None,
            None,
            error.to_string(),
            now,
        );
    }
}
