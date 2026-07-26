//! Snapshot projection, bounded observation, and controller shutdown.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use nmp_native_runtime_app::PlatformCommand;

use super::{ObserverPermit, RuntimeController};
use crate::{
    ObservationStart, RuntimeObservation, RuntimeObservationFrame, RuntimeObserver,
    RuntimeRelayDiagnosticsObservationStart, RuntimeRelayDiagnosticsObserver,
    RuntimeRelayDiagnosticsSnapshot, projection::project_event, support::bump_signal,
};

#[uniffi::export]
impl RuntimeController {
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
