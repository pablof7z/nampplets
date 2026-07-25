//! Snapshot projection, bounded observation, and controller shutdown.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use nmp_native_runtime_app::{AppSnapshot, InstalledBuildAvailability, PlatformCommand};

use super::{ObserverPermit, RuntimeController};
use crate::{
    ObservationStart, RuntimeActivitySnapshot, RuntimeBindingSnapshot, RuntimeErrorSnapshot,
    RuntimeExactBuildCoordinate, RuntimeInstalledBuildAvailability, RuntimeInstalledBuildSnapshot,
    RuntimeInstalledLibrarySnapshot, RuntimeObservation, RuntimeObservationFrame, RuntimeObserver,
    RuntimePendingWriteSnapshot, RuntimeReceiptSnapshot, RuntimeRelayDiagnosticsObservationStart,
    RuntimeRelayDiagnosticsObserver, RuntimeRelayDiagnosticsSnapshot, RuntimeSessionSnapshot,
    RuntimeSnapshot,
    projection::{project_event, project_profile},
    support::bump_signal,
    workspace::workspace_from_view,
};

#[uniffi::export]
impl RuntimeController {
    pub fn snapshot(&self) -> RuntimeSnapshot {
        self.project_snapshot(&self.app.snapshot())
    }

    /// The latest NMP-owned relay and wire-subscription read-out. It is only
    /// refreshed while an observation is open; check `observing`.
    pub fn relay_diagnostics(&self) -> RuntimeRelayDiagnosticsSnapshot {
        self.diagnostics.snapshot()
    }

    /// Open the NMP diagnostics observation for as long as the returned handle
    /// lives. The current read-out is delivered synchronously on registration.
    pub fn observe_relay_diagnostics(
        &self,
        observer: Box<dyn RuntimeRelayDiagnosticsObserver>,
    ) -> RuntimeRelayDiagnosticsObservationStart {
        match self.diagnostics.observe(Arc::from(observer)) {
            Ok(observation) => RuntimeRelayDiagnosticsObservationStart {
                observation: Some(observation),
                refusal: None,
            },
            Err(error) => RuntimeRelayDiagnosticsObservationStart {
                observation: None,
                refusal: Some(self.refusal("relay-diagnostics-observe", error.to_string())),
            },
        }
    }

    pub fn observe(self: Arc<Self>, observer: Box<dyn RuntimeObserver>) -> ObservationStart {
        let admitted = self
            .observers
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < self.maximum_observers).then_some(active + 1)
            });
        if admitted.is_err() {
            return ObservationStart {
                observation: None,
                refusal: Some(self.refusal(
                    "observer-capacity",
                    format!("observer capacity {} is full", self.maximum_observers),
                )),
            };
        }
        let stopped = Arc::new(AtomicBool::new(false));
        let handle = Arc::new(RuntimeObservation {
            stopped: Arc::clone(&stopped),
            signal: self.signal.clone(),
        });
        let controller = Arc::clone(&self);
        let observer: Arc<dyn RuntimeObserver> = Arc::from(observer);
        let observers = Arc::clone(&self.observers);
        let mut app_observer = self.app.observe();
        let mut signal = self.signal.subscribe();
        let mut catalog_signal = self.catalog.subscribe();
        let spawn = thread::Builder::new()
            .name("runtime-ffi-observer".to_owned())
            .spawn(move || {
                let _permit = ObserverPermit(observers);
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    controller.record_refusal(
                        "observer-thread",
                        "could not construct the observation runtime",
                    );
                    return;
                };
                runtime.block_on(async move {
                    let mut event_cursor = 0_u64;
                    loop {
                        if stopped.load(Ordering::Acquire) {
                            break;
                        }
                        let batch = controller.app.events_after(event_cursor);
                        event_cursor = batch.newest_available;
                        observer.update(RuntimeObservationFrame {
                            snapshot: controller.project_snapshot(&app_observer.latest()),
                            catalog: controller.catalog.feed_snapshot(None),
                            events: batch
                                .events
                                .into_iter()
                                .map(|event| project_event(event.sequence, &event.event))
                                .collect(),
                            oldest_available_event: batch.oldest_available,
                            newest_available_event: batch.newest_available,
                            event_cursor_was_stale: batch.cursor_was_stale,
                            lost_before_batch: batch.lost_before_batch,
                        });
                        tokio::select! {
                            changed = app_observer.changed() => {
                                if changed.is_err() {
                                    break;
                                }
                            }
                            changed = signal.changed() => {
                                if changed.is_err() {
                                    break;
                                }
                            }
                            changed = catalog_signal.changed() => {
                                if changed.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                });
            });
        if let Err(error) = spawn {
            handle.stop();
            self.observers.fetch_sub(1, Ordering::AcqRel);
            return ObservationStart {
                observation: None,
                refusal: Some(self.refusal("observer-thread", error.to_string())),
            };
        }
        ObservationStart {
            observation: Some(handle),
            refusal: None,
        }
    }

    pub fn close(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            self.catalog.close();
            self.diagnostics.close();
            self.app.dispatch(PlatformCommand::Close);
            self.data_plane.close();
            bump_signal(&self.signal);
        }
    }
}

impl RuntimeController {
    pub(super) fn project_snapshot(&self, snapshot: &AppSnapshot) -> RuntimeSnapshot {
        let refusals = self.boundary_refusals.lock();
        RuntimeSnapshot {
            revision: snapshot.revision,
            closed: snapshot.closed,
            installed_library: RuntimeInstalledLibrarySnapshot {
                query: snapshot.library.query.to_string(),
                total_installed: snapshot.library.total_installed as u64,
                builds: snapshot
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
                        assigned_workspace_ids: snapshot
                            .workspaces
                            .iter()
                            .filter(|workspace| {
                                workspace.assigned_builds.contains(&view.build.principal)
                            })
                            .map(|workspace| workspace.id.to_string())
                            .collect(),
                    })
                    .collect(),
            },
            sessions: snapshot
                .sessions
                .iter()
                .map(|session| RuntimeSessionSnapshot {
                    id: session.id.0,
                    author: session.principal.manifest_author().to_owned(),
                    d_tag: session.principal.d_tag().to_owned(),
                    aggregate_hash: session.principal.aggregate_hash().to_owned(),
                    profile: project_profile(session.profile),
                    state: format!("{:?}", session.state).to_ascii_lowercase(),
                    domains: snapshot
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
                })
                .collect(),
            bindings: snapshot
                .bindings
                .iter()
                .map(|binding| RuntimeBindingSnapshot {
                    id: binding.id.to_string(),
                    schema: binding.schema.to_string(),
                    logical_source_id: binding.logical_source_id.as_deref().map(str::to_owned),
                    revision: binding.revision,
                })
                .collect(),
            pending_writes: snapshot
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
            receipts: snapshot
                .receipts
                .iter()
                .map(|receipt| RuntimeReceiptSnapshot {
                    receipt_id: receipt.receipt_id.0.to_string(),
                    delivery: format!("{:?}", receipt.delivery).to_ascii_lowercase(),
                    latest_state_json: receipt
                        .latest
                        .as_ref()
                        .map(|latest| latest.state.as_str().to_owned()),
                })
                .collect(),
            workspaces: snapshot
                .workspaces
                .iter()
                .filter_map(|workspace| workspace_from_view(workspace).ok())
                .collect(),
            recent_activity: snapshot
                .recent_activity
                .iter()
                .map(|fact| RuntimeActivitySnapshot {
                    author: fact.principal.manifest_author().to_owned(),
                    d_tag: fact.principal.d_tag().to_owned(),
                    aggregate_hash: fact.principal.aggregate_hash().to_owned(),
                    category: fact.category.to_string(),
                    operation: fact.operation.to_string(),
                    outcome: fact.outcome.to_string(),
                    occurred_at_millis: fact.occurred_at_millis,
                })
                .collect(),
            dropped_activity: snapshot.dropped_activity,
            recent_errors: snapshot
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
            dropped_errors: snapshot.dropped_errors,
            boundary_refusals: refusals.iter().cloned().collect(),
            dropped_boundary_refusals: refusals.dropped(),
            active_resources: snapshot.resources.admitted as u64,
            resource_high_watermark: snapshot.resources.high_watermark as u64,
            resource_refusal_count: snapshot.resources.refusal_count,
        }
    }
}
