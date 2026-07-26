//! Snapshot publication plus the bounded activity, event, and refusal rings.

use std::sync::Arc;

use super::{AppState, RuntimeApp, install::installed_library_view, revisions::advance_revisions};
use crate::views::{
    AppSnapshot, BindingView, ProviderPushLaneView, ProviderWriteProposalView, SectionRevisions,
    SessionDomainView, SnapshotSection, WorkspaceView,
};

impl RuntimeApp {
    pub(super) fn publish(&self, state: &mut AppState) {
        if state.terminal_reason.is_some() {
            return;
        }
        let previous = Arc::clone(&self.snapshots.borrow());
        let Some(next_revision) = state.revision.checked_add(1) else {
            self.enter_revision_terminal_from(state, &previous, SnapshotSection::FusedSnapshot);
            return;
        };
        let mut snapshot = self.build_snapshot(state);
        snapshot.revisions = match advance_revisions(&previous, &snapshot, state) {
            Ok(revisions) => revisions,
            Err(section) => {
                self.enter_revision_terminal_from(state, &previous, section);
                return;
            }
        };
        state.revision = next_revision;
        snapshot.revision = next_revision;
        self.snapshots.send_replace(Arc::new(snapshot));
    }

    pub(super) fn build_snapshot(&self, state: &AppState) -> AppSnapshot {
        AppSnapshot {
            revision: state.revision,
            revisions: SectionRevisions::default(),
            closed: state.closed,
            terminal_reason: state.terminal_reason.clone(),
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
                    unavailable_domains: entry.unavailable_domains.iter().cloned().collect(),
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
}
