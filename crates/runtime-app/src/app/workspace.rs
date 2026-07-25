//! Workspace persistence, build assignment, and restoration.

use std::sync::Arc;

use nmp_native_runtime_core::Principal;
use nmp_native_runtime_store::WorkspaceRecord;

use super::{AppState, RuntimeApp};
use crate::{commands::PlatformEvent, views::AppErrorCode};

impl RuntimeApp {
    pub(super) fn save_workspace(
        &self,
        state: &mut AppState,
        workspace: WorkspaceRecord,
        now: u64,
    ) {
        if let Err(error) = self.store.save_workspace(&workspace) {
            self.refuse_store(state, None, None, error, now);
            return;
        }
        let workspace_id = Arc::clone(&workspace.id);
        state
            .workspaces
            .insert(Arc::clone(&workspace_id), workspace);
        self.push_event(state, PlatformEvent::WorkspaceSaved { workspace_id });
    }

    pub(super) fn assign_workspace_build(
        &self,
        state: &mut AppState,
        workspace_id: Arc<str>,
        principal: Principal,
        assigned: bool,
        now: u64,
    ) {
        let result = if assigned {
            self.store
                .assign_build_to_workspace(&workspace_id, &principal)
                .map(|()| true)
        } else {
            self.store
                .remove_build_from_workspace(&workspace_id, &principal)
        };
        let changed = match result {
            Ok(changed) => changed,
            Err(error) => {
                self.refuse_store(state, Some(principal), None, error, now);
                return;
            }
        };
        let assignments = state
            .workspace_assignments
            .entry(Arc::clone(&workspace_id))
            .or_default();
        if assigned {
            assignments.insert(principal.clone());
        } else {
            assignments.remove(&principal);
        }
        if changed || assigned {
            self.push_event(
                state,
                PlatformEvent::WorkspaceAssignmentChanged {
                    workspace_id,
                    principal,
                    assigned,
                },
            );
        }
    }

    pub(super) fn restore_workspaces(&self, state: &mut AppState, now: u64) {
        let workspaces = match self.store.load_workspaces() {
            Ok(workspaces) => workspaces,
            Err(error) => {
                self.refuse_store(state, None, None, error, now);
                return;
            }
        };
        for workspace in workspaces {
            let workspace_id = Arc::clone(&workspace.id);
            let assignments = match self.store.workspace_assignments(&workspace_id) {
                Ok(assignments) => assignments,
                Err(error) => {
                    self.refuse_store(state, None, None, error, now);
                    continue;
                }
            };
            for receipt_id in workspace.retained_receipts.iter().cloned() {
                if state.receipts.contains_key(&receipt_id) {
                    continue;
                }
                if state.receipts.len() >= self.limits.maximum_receipts {
                    self.refuse(
                        state,
                        AppErrorCode::Capacity,
                        None,
                        None,
                        "receipt restoration capacity is full",
                        now,
                    );
                    break;
                }
                self.reattach_receipt(state, receipt_id, now);
            }
            state.workspaces.insert(workspace_id.clone(), workspace);
            state
                .workspace_assignments
                .insert(workspace_id.clone(), assignments.into_iter().collect());
            self.push_event(state, PlatformEvent::WorkspaceRestored { workspace_id });
        }
    }
}
