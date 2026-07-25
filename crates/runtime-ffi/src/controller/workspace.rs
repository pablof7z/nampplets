//! Versioned native workspace persistence and restore.

use std::sync::atomic::Ordering;

use nmp_native_runtime_app::{PlatformCommand, PlatformEvent};

use super::RuntimeController;
use crate::{
    RuntimeWorkspaceDefinition, RuntimeWorkspaceRestore, RuntimeWorkspaceUpdate,
    support::bump_signal,
    workspace::{workspace_from_record, workspace_from_view, workspace_record_from_ffi},
};

#[uniffi::export]
impl RuntimeController {
    /// Persists one complete, versioned native workspace definition.
    ///
    /// The boundary refuses partial/unknown schemas before dispatch. The
    /// runtime store performs the replacement atomically and the returned
    /// value is projected from the Rust-owned snapshot, never echoed from the
    /// Swift request.
    pub fn save_workspace(&self, workspace: RuntimeWorkspaceDefinition) -> RuntimeWorkspaceUpdate {
        if self.closed.load(Ordering::Acquire) {
            return RuntimeWorkspaceUpdate {
                accepted: false,
                workspace: None,
                refusal: Some(self.refusal("closed", "runtime is closed")),
            };
        }
        let workspace_id = workspace.workspace_id.clone();
        let record = match workspace_record_from_ffi(workspace) {
            Ok(record) => record,
            Err(detail) => {
                return RuntimeWorkspaceUpdate {
                    accepted: false,
                    workspace: None,
                    refusal: Some(self.workspace_refusal("invalid-workspace", detail)),
                };
            }
        };
        let cursor = self.app.events_after(0).newest_available;
        self.app
            .dispatch(PlatformCommand::SaveWorkspace { workspace: record });
        let saved = self.app.events_after(cursor).events.iter().any(|event| {
            matches!(
                &event.event,
                PlatformEvent::WorkspaceSaved { workspace_id: saved }
                    if saved.as_ref() == workspace_id
            )
        });
        bump_signal(&self.signal);
        if !saved {
            let detail = self.app.snapshot().recent_errors.last().map_or_else(
                || "workspace persistence was refused".to_owned(),
                |error| error.detail.to_string(),
            );
            return RuntimeWorkspaceUpdate {
                accepted: false,
                workspace: None,
                refusal: Some(self.workspace_refusal("workspace-store", detail)),
            };
        }
        let projected = self
            .app
            .snapshot()
            .workspaces
            .iter()
            .find(|candidate| candidate.id.as_ref() == workspace_id)
            .and_then(|candidate| workspace_from_view(candidate).ok());
        match projected {
            Some(workspace) => RuntimeWorkspaceUpdate {
                accepted: true,
                workspace: Some(workspace),
                refusal: None,
            },
            None => RuntimeWorkspaceUpdate {
                accepted: false,
                workspace: None,
                refusal: Some(self.workspace_refusal(
                    "workspace-projection",
                    "saved workspace could not be projected through the versioned schema",
                )),
            },
        }
    }

    /// Validates every durable row before making any restored workspace
    /// visible. Unknown versions or malformed rows refuse the whole restore.
    pub fn restore_workspaces(&self) -> RuntimeWorkspaceRestore {
        if self.closed.load(Ordering::Acquire) {
            return RuntimeWorkspaceRestore {
                accepted: false,
                workspaces: Vec::new(),
                refusal: Some(self.refusal("closed", "runtime is closed")),
            };
        }
        let durable = match self.runtime_store.load_workspaces() {
            Ok(workspaces) => workspaces,
            Err(error) => {
                return RuntimeWorkspaceRestore {
                    accepted: false,
                    workspaces: Vec::new(),
                    refusal: Some(self.workspace_refusal("workspace-store", error.to_string())),
                };
            }
        };
        let mut validated = Vec::with_capacity(durable.len());
        for workspace in &durable {
            match workspace_from_record(workspace) {
                Ok(workspace) => validated.push(workspace),
                Err(detail) => {
                    return RuntimeWorkspaceRestore {
                        accepted: false,
                        workspaces: Vec::new(),
                        refusal: Some(self.workspace_refusal("invalid-workspace", detail)),
                    };
                }
            }
        }
        self.app.dispatch(PlatformCommand::RestoreWorkspaces);
        bump_signal(&self.signal);
        RuntimeWorkspaceRestore {
            accepted: true,
            workspaces: validated,
            refusal: None,
        }
    }
}
