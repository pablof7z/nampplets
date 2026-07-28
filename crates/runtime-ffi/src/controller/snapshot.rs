//! Fail-closed projection of one runtime application snapshot.

use std::collections::BTreeSet;

use nmp_native_runtime_app::{AppSnapshot, InstalledBuildAvailability, WorkspaceView};
use nmp_native_runtime_core::Principal;

use super::RuntimeController;
use crate::activity::activity_snapshot;
use crate::{
    RuntimeBindingSnapshot, RuntimeErrorSnapshot, RuntimeExactBuildCoordinate,
    RuntimeInstalledBuildAvailability, RuntimeInstalledBuildSnapshot,
    RuntimeInstalledLibrarySnapshot, RuntimePendingWriteSnapshot, RuntimeSessionSnapshot,
    RuntimeSnapshot, RuntimeSnapshotProjection, project_receipt, projection::project_profile,
    snapshot_integrity::check_snapshot_integrity, workspace::workspace_from_view,
};

/// Domains the build required that no provider advertises, for one session.
fn unavailable_domains_for(
    session: &nmp_native_runtime_core::SessionSnapshot,
    source: &nmp_native_runtime_app::AppSnapshot,
) -> Vec<String> {
    source
        .session_domains
        .iter()
        .find(|view| view.session == session.id)
        .map(|view| {
            view.unavailable_domains
                .iter()
                .map(|domain| domain.as_str().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// The lifecycle state, widened so a session missing a domain its own content
/// requires cannot read as a whole one.
///
/// The shortfall is also on the snapshot as a set, but a consumer that renders
/// only `state` -- which is what the Inspector did all day -- would otherwise
/// show "running" for a napplet running without the capabilities it declared.
/// Reporting the degraded case as healthy has to be something a consumer
/// chooses, not something it gets by not looking.
fn project_session_state(
    session: &nmp_native_runtime_core::SessionSnapshot,
    source: &nmp_native_runtime_app::AppSnapshot,
) -> String {
    let lifecycle = format!("{:?}", session.state).to_ascii_lowercase();
    if lifecycle == "running" && !unavailable_domains_for(session, source).is_empty() {
        return "running-degraded".to_owned();
    }
    lifecycle
}

struct ProjectedWorkspaces {
    workspaces: Vec<crate::RuntimeWorkspaceDefinition>,
    published_ids: BTreeSet<String>,
    unprojectable: Vec<(String, String)>,
}

fn project_workspaces(views: &[WorkspaceView]) -> ProjectedWorkspaces {
    let mut workspaces = Vec::with_capacity(views.len());
    let mut published_ids = BTreeSet::new();
    let mut unprojectable = Vec::new();
    for view in views {
        match workspace_from_view(view) {
            Ok(workspace) => {
                published_ids.insert(workspace.workspace_id.clone());
                workspaces.push(workspace);
            }
            Err(error) => unprojectable.push((view.id.to_string(), error)),
        }
    }
    ProjectedWorkspaces {
        workspaces,
        published_ids,
        unprojectable,
    }
}

fn assigned_workspace_ids(
    views: &[WorkspaceView],
    published_ids: &BTreeSet<String>,
    principal: &Principal,
) -> Vec<String> {
    views
        .iter()
        .filter(|workspace| {
            published_ids.contains(workspace.id.as_ref())
                && workspace.assigned_builds.contains(principal)
        })
        .map(|workspace| workspace.id.to_string())
        .collect()
}

#[uniffi::export]
impl RuntimeController {
    pub fn snapshot(&self) -> RuntimeSnapshotProjection {
        self.project_snapshot(&self.app.snapshot())
    }
}

impl RuntimeController {
    pub(crate) fn project_snapshot(&self, source: &AppSnapshot) -> RuntimeSnapshotProjection {
        let ProjectedWorkspaces {
            workspaces,
            published_ids,
            unprojectable,
        } = project_workspaces(&source.workspaces);
        let refused_operator_relays = self.refused_operator_relays.clone();
        let (boundary_refusals, dropped_boundary_refusals) = {
            let refusals = self.boundary_refusals.lock();
            (refusals.iter().cloned().collect(), refusals.dropped())
        };

        let candidate = RuntimeSnapshot {
            revision: source.revision,
            closed: source.closed,
            installed_library: RuntimeInstalledLibrarySnapshot {
                query: source.library.query.to_string(),
                total_installed: source.library.total_installed as u64,
                builds: source
                    .library
                    .builds
                    .iter()
                    .map(|view| RuntimeInstalledBuildSnapshot {
                        coordinate: RuntimeExactBuildCoordinate {
                            manifest_author: view.build.principal.manifest_author().to_owned(),
                            d_tag: view.build.principal.d_tag().to_owned(),
                            aggregate_hash: view.build.principal.aggregate_hash().to_owned(),
                        },
                        title: view.build.title.to_string(),
                        manifest_metadata_json: view.build.manifest_metadata.as_str().to_owned(),
                        availability: match view.availability {
                            InstalledBuildAvailability::MetadataOnly => {
                                RuntimeInstalledBuildAvailability::MetadataOnly
                            }
                            InstalledBuildAvailability::SealedExactBytesReady => {
                                RuntimeInstalledBuildAvailability::SealedExactBytesReady
                            }
                        },
                        active_session_ids: view
                            .active_sessions
                            .iter()
                            .map(|session| session.0)
                            .collect(),
                        assigned_workspace_ids: assigned_workspace_ids(
                            &source.workspaces,
                            &published_ids,
                            &view.build.principal,
                        ),
                    })
                    .collect(),
            },
            sessions: source
                .sessions
                .iter()
                .map(|session| RuntimeSessionSnapshot {
                    id: session.id.0,
                    author: session.principal.manifest_author().to_owned(),
                    d_tag: session.principal.d_tag().to_owned(),
                    aggregate_hash: session.principal.aggregate_hash().to_owned(),
                    profile: project_profile(session.profile),
                    state: project_session_state(session, source),
                    domains: source
                        .session_domains
                        .iter()
                        .find(|view| view.session == session.id)
                        .map(|view| {
                            view.domains
                                .iter()
                                .map(|domain| domain.as_str().to_owned())
                                .collect()
                        })
                        .unwrap_or_default(),
                    unavailable_domains: unavailable_domains_for(session, source),
                })
                .collect(),
            bindings: source
                .bindings
                .iter()
                .map(|binding| RuntimeBindingSnapshot {
                    id: binding.id.to_string(),
                    schema: binding.schema.to_string(),
                    logical_source_id: binding.logical_source_id.as_deref().map(str::to_owned),
                    revision: binding.revision,
                })
                .collect(),
            pending_writes: source
                .pending_writes
                .iter()
                .map(|pending| RuntimePendingWriteSnapshot {
                    operation_id: pending.operation.0,
                    approval_id: pending.approval_id.to_string(),
                    author: pending.principal.manifest_author().to_owned(),
                    d_tag: pending.principal.d_tag().to_owned(),
                    aggregate_hash: pending.principal.aggregate_hash().to_owned(),
                    session_id: pending.session.0,
                    account: pending.account.0.to_string(),
                    draft_json: pending.draft.as_str().to_owned(),
                })
                .collect(),
            receipts: source.receipts.iter().map(project_receipt).collect(),
            workspaces,
            recent_activity: source
                .recent_activity
                .iter()
                .map(activity_snapshot)
                .collect(),
            dropped_activity: source.dropped_activity,
            recent_errors: source
                .recent_errors
                .iter()
                .map(|fact| RuntimeErrorSnapshot {
                    code: format!("{:?}", fact.code).to_ascii_lowercase(),
                    author: fact
                        .principal
                        .as_ref()
                        .map(|principal| principal.manifest_author().to_owned()),
                    d_tag: fact
                        .principal
                        .as_ref()
                        .map(|principal| principal.d_tag().to_owned()),
                    aggregate_hash: fact
                        .principal
                        .as_ref()
                        .map(|principal| principal.aggregate_hash().to_owned()),
                    session_id: fact.session.map(|session| session.0),
                    detail: fact.detail.to_string(),
                    occurred_at_millis: fact.occurred_at_millis,
                })
                .collect(),
            dropped_errors: source.dropped_errors,
            boundary_refusals,
            dropped_boundary_refusals,
            refused_operator_relays,
            active_resources: source.resources.admitted as u64,
            resource_high_watermark: source.resources.high_watermark as u64,
            resource_refusal_count: source.resources.refusal_count,
        };

        let refusal = unprojectable.first().map(|(workspace_id, error)| {
            self.refusal(
                "workspace-projection",
                format!("stored workspace {workspace_id} cannot cross the boundary: {error}"),
            )
        });
        let refusal = refusal.or_else(|| {
            check_snapshot_integrity(&candidate)
                .err()
                .map(|violation| self.refusal(violation.code(), violation.to_string()))
        });
        match refusal {
            Some(refusal) => {
                let revision = candidate.revision;
                let closed = candidate.closed;
                self.report_projection_fault(refusal.clone());
                RuntimeSnapshotProjection::Refused {
                    revision,
                    closed,
                    refusal,
                }
            }
            None => RuntimeSnapshotProjection::Snapshot {
                snapshot: candidate,
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn projection_signal_revision(&self) -> u64 {
        *self.signal.borrow()
    }
}
