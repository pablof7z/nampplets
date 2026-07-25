//! Bounded relay and wire-subscription read-out for the native inspector.
//!
//! NMP owns every fact projected here. The NMP diagnostics observation is
//! opened on the first registered observer and cancelled once the last one
//! goes away, so an unobserved inspector costs no relay accounting; this
//! module never recomputes, merges, or estimates a relay fact.

use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

use nmp_native_nmp_adapter::{
    NmpDataPlane,
    diagnostics::{
        DiagnosticsAccessContext, DiagnosticsLane, NmpRelayDiagnostics, RelayDiagnosticsCancel,
        RelayDiagnosticsError as AdapterRelayDiagnosticsError, RelayDiagnosticsFrame,
        RelayDiagnosticsView,
    },
};
use parking_lot::Mutex;
use thiserror::Error;

use super::{RuntimeRefusal, support::now_millis};

const MAXIMUM_DIAGNOSTICS_OBSERVERS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeRelayLane {
    Nip65Write,
    Nip65Read,
    Hint,
    Provenance,
    UserConfigured,
    IndexerDiscovery,
    GroupHost,
    DmInbox,
    AppRelay,
    Fallback,
    ExplicitPinned,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeRelayAccess {
    Public,
    Nip42 { public_key: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeRelayLaneCount {
    pub lane: RuntimeRelayLane,
    pub wire_subscriptions: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeRelayKindCount {
    pub kind: u16,
    pub events: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeRelayCoverage {
    pub from_seconds: u64,
    pub through_seconds: u64,
}

/// One currently active wire subscription. `coverage` is absent when the relay
/// has no proven row for this filter's shape; absent is unproven, never zero.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeRelaySubscription {
    pub filter: String,
    pub coverage: Option<RuntimeRelayCoverage>,
}

/// One physical relay session. A relay planned under several access contexts
/// yields several rows.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeRelayDiagnostics {
    pub relay: String,
    pub access: RuntimeRelayAccess,
    pub wire_subscription_count: u64,
    pub authors_served: u64,
    pub lanes: Vec<RuntimeRelayLaneCount>,
    pub omitted_lanes: u64,
    pub subscriptions: Vec<RuntimeRelaySubscription>,
    pub omitted_subscriptions: u64,
    pub events_by_kind: Vec<RuntimeRelayKindCount>,
    pub omitted_kinds: u64,
    pub supported_nips: Option<Vec<u16>>,
    pub omitted_supported_nips: u64,
    pub nip11_document_revision: Option<String>,
    pub nip11_freshness: Option<String>,
    pub nip11_last_error: Option<String>,
    pub nip77_advertisement: String,
    pub nip77_behavior: String,
    pub nip77_handoff: String,
}

/// Latest replacement from the on-demand NMP diagnostics observation.
///
/// `observing` is false when no observer is registered. Empty `relays` with
/// `observing` false means "not currently accounted", never a claim that the
/// engine has planned no relay session.
#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeRelayDiagnosticsSnapshot {
    pub revision: u64,
    pub observing: bool,
    pub relays: Vec<RuntimeRelayDiagnostics>,
    pub omitted_relays: u64,
    pub uncovered_author_count: u64,
    pub dropped_merge_rules: Vec<String>,
    pub omitted_dropped_merge_rules: u64,
    pub discovered_private_relays_rejected: u64,
    pub sessions_rejected_over_cap: u64,
    pub store_degraded: Option<String>,
    pub transport_degraded: Option<String>,
    pub failure: Option<RuntimeRefusal>,
}

#[uniffi::export(callback_interface)]
pub trait RuntimeRelayDiagnosticsObserver: Send + Sync {
    fn update(&self, snapshot: RuntimeRelayDiagnosticsSnapshot);
}

/// Cancellation handle for one registered observer. The NMP observation is
/// withdrawn once the last handle stops or drops.
#[derive(Debug, uniffi::Object)]
pub struct RuntimeRelayDiagnosticsObservation {
    service: Arc<RuntimeDiagnosticsService>,
    id: u64,
    stopped: AtomicBool,
}

#[uniffi::export]
impl RuntimeRelayDiagnosticsObservation {
    pub fn stop(&self) {
        if !self.stopped.swap(true, Ordering::AcqRel) {
            self.service.remove_observer(self.id);
        }
    }
}

impl Drop for RuntimeRelayDiagnosticsObservation {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeRelayDiagnosticsObservationStart {
    pub observation: Option<Arc<RuntimeRelayDiagnosticsObservation>>,
    pub refusal: Option<RuntimeRefusal>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum RuntimeRelayDiagnosticsError {
    #[error("the relay diagnostics worker could not start: {reason}")]
    WorkerUnavailable { reason: String },
    #[error("relay diagnostics observer capacity {capacity} is full")]
    ObserverCapacity { capacity: usize },
    #[error("the relay diagnostics service is closed")]
    Closed,
}

struct DiagnosticsState {
    revision: u64,
    frame: Option<RelayDiagnosticsFrame>,
    failure: Option<AdapterRelayDiagnosticsError>,
    closed: bool,
    next_observer_id: u64,
    observers: BTreeMap<u64, Arc<dyn RuntimeRelayDiagnosticsObserver>>,
    control: Option<Arc<DiagnosticsFeedControl>>,
    worker: Option<JoinHandle<()>>,
}

impl fmt::Debug for DiagnosticsState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DiagnosticsState")
            .field("revision", &self.revision)
            .field("observers", &self.observers.len())
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

/// Profile-owned relay diagnostics service. The NMP observation exists only
/// while at least one observer is registered.
#[derive(Debug)]
pub struct RuntimeDiagnosticsService {
    diagnostics: NmpRelayDiagnostics,
    state: Mutex<DiagnosticsState>,
}

impl RuntimeDiagnosticsService {
    pub fn new(data_plane: &NmpDataPlane) -> Self {
        Self {
            diagnostics: data_plane.relay_diagnostics(),
            state: Mutex::new(DiagnosticsState {
                revision: 0,
                frame: None,
                failure: None,
                closed: false,
                next_observer_id: 0,
                observers: BTreeMap::new(),
                control: None,
                worker: None,
            }),
        }
    }

    /// The latest delivered read-out. Without a registered observer this is the
    /// last value seen, marked `observing: false`.
    pub fn snapshot(&self) -> RuntimeRelayDiagnosticsSnapshot {
        project_state(&self.state.lock())
    }

    pub fn observe(
        self: &Arc<Self>,
        observer: Arc<dyn RuntimeRelayDiagnosticsObserver>,
    ) -> Result<Arc<RuntimeRelayDiagnosticsObservation>, RuntimeRelayDiagnosticsError> {
        let (id, current, should_start) = {
            let mut state = self.state.lock();
            if state.closed {
                return Err(RuntimeRelayDiagnosticsError::Closed);
            }
            if state.observers.len() >= MAXIMUM_DIAGNOSTICS_OBSERVERS {
                return Err(RuntimeRelayDiagnosticsError::ObserverCapacity {
                    capacity: MAXIMUM_DIAGNOSTICS_OBSERVERS,
                });
            }
            let id = state.next_observer_id;
            state.next_observer_id = id.saturating_add(1);
            state.observers.insert(id, Arc::clone(&observer));
            let should_start = state.control.is_none();
            (id, project_state(&state), should_start)
        };

        if should_start && let Err(error) = self.start_feed() {
            self.remove_observer(id);
            return Err(error);
        }

        observer.update(current);
        Ok(Arc::new(RuntimeRelayDiagnosticsObservation {
            service: Arc::clone(self),
            id,
            stopped: AtomicBool::new(false),
        }))
    }

    fn start_feed(self: &Arc<Self>) -> Result<(), RuntimeRelayDiagnosticsError> {
        let stale_worker = self.state.lock().worker.take();
        if let Some(worker) = stale_worker {
            let _ = worker.join();
        }

        let control = Arc::new(DiagnosticsFeedControl::default());
        let service = Arc::clone(self);
        let feed_control = Arc::clone(&control);
        let worker = thread::Builder::new()
            .name("runtime-diagnostics-feed".to_owned())
            .spawn(move || service.run_feed(&feed_control))
            .map_err(|error| RuntimeRelayDiagnosticsError::WorkerUnavailable {
                reason: error.to_string(),
            })?;

        let mut state = self.state.lock();
        state.control = Some(control);
        state.worker = Some(worker);
        Ok(())
    }

    fn run_feed(&self, control: &DiagnosticsFeedControl) {
        let observation = match self.diagnostics.observe() {
            Ok(observation) => observation,
            Err(error) => {
                self.publish_failure(error);
                return;
            }
        };
        control.attach(observation.cancel_handle());
        if control.is_cancelled() {
            return;
        }
        loop {
            let frame = match observation.recv() {
                Ok(frame) => frame,
                Err(error) => {
                    if !control.is_cancelled() {
                        self.publish_failure(error);
                    }
                    return;
                }
            };
            let delivery = {
                let mut state = self.state.lock();
                if state.closed || control.is_cancelled() {
                    return;
                }
                state.revision = state.revision.saturating_add(1);
                state.frame = Some(frame);
                state.failure = None;
                delivery(&state)
            };
            deliver(delivery);
        }
    }

    fn publish_failure(&self, failure: AdapterRelayDiagnosticsError) {
        let pending = {
            let mut state = self.state.lock();
            if state.closed {
                return;
            }
            state.revision = state.revision.saturating_add(1);
            state.failure = Some(failure);
            delivery(&state)
        };
        deliver(pending);
    }

    /// The drain thread is cancelled but deliberately not joined here: a native
    /// observer may stop from inside its own `update`, which runs on that very
    /// thread. The handle is reaped by the next `start_feed` or by `close`.
    fn remove_observer(&self, id: u64) {
        let control = {
            let mut state = self.state.lock();
            state.observers.remove(&id);
            if !state.observers.is_empty() {
                return;
            }
            state.control.take()
        };
        if let Some(control) = control {
            control.cancel();
        }
    }

    pub fn close(&self) {
        let (control, worker) = {
            let mut state = self.state.lock();
            if state.closed {
                return;
            }
            state.closed = true;
            state.observers.clear();
            (state.control.take(), state.worker.take())
        };
        if let Some(control) = control {
            control.cancel();
        }
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

type DiagnosticsDelivery = (
    Vec<Arc<dyn RuntimeRelayDiagnosticsObserver>>,
    RuntimeRelayDiagnosticsSnapshot,
);

fn delivery(state: &DiagnosticsState) -> DiagnosticsDelivery {
    (
        state.observers.values().cloned().collect(),
        project_state(state),
    )
}

/// Callbacks run outside the state lock: a native observer may stop its own
/// observation from inside `update`.
fn deliver((observers, snapshot): DiagnosticsDelivery) {
    for observer in observers {
        observer.update(snapshot.clone());
    }
}

#[derive(Debug, Default)]
struct DiagnosticsFeedControl {
    cancel: Mutex<Option<RelayDiagnosticsCancel>>,
    cancelled: AtomicBool,
}

impl DiagnosticsFeedControl {
    fn attach(&self, cancel: RelayDiagnosticsCancel) {
        *self.cancel.lock() = Some(cancel);
        if self.cancelled.load(Ordering::Acquire) {
            self.cancel();
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(cancel) = self.cancel.lock().as_ref() {
            cancel.cancel();
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

fn project_state(state: &DiagnosticsState) -> RuntimeRelayDiagnosticsSnapshot {
    let failure = state.failure.as_ref().map(project_diagnostics_error);
    let observing = state.control.is_some();
    let Some(frame) = state.frame.as_ref() else {
        return RuntimeRelayDiagnosticsSnapshot {
            revision: state.revision,
            observing,
            relays: Vec::new(),
            omitted_relays: 0,
            uncovered_author_count: 0,
            dropped_merge_rules: Vec::new(),
            omitted_dropped_merge_rules: 0,
            discovered_private_relays_rejected: 0,
            sessions_rejected_over_cap: 0,
            store_degraded: None,
            transport_degraded: None,
            failure,
        };
    };
    RuntimeRelayDiagnosticsSnapshot {
        revision: state.revision,
        observing,
        relays: frame.relays.iter().map(project_relay).collect(),
        omitted_relays: frame.omitted_relays as u64,
        uncovered_author_count: frame.uncovered_author_count as u64,
        dropped_merge_rules: frame
            .dropped_merge_rules
            .iter()
            .map(|rule| rule.to_string())
            .collect(),
        omitted_dropped_merge_rules: frame.omitted_dropped_merge_rules as u64,
        discovered_private_relays_rejected: frame.discovered_private_relays_rejected,
        sessions_rejected_over_cap: frame.sessions_rejected_over_cap,
        store_degraded: frame.store_degraded.as_ref().map(|value| value.to_string()),
        transport_degraded: frame
            .transport_degraded
            .as_ref()
            .map(|value| value.to_string()),
        failure,
    }
}

fn project_relay(view: &RelayDiagnosticsView) -> RuntimeRelayDiagnostics {
    RuntimeRelayDiagnostics {
        relay: view.relay.to_string(),
        access: match &view.access {
            DiagnosticsAccessContext::Public => RuntimeRelayAccess::Public,
            DiagnosticsAccessContext::Nip42 { public_key } => RuntimeRelayAccess::Nip42 {
                public_key: public_key.to_string(),
            },
        },
        wire_subscription_count: view.wire_subscription_count as u64,
        authors_served: view.authors_served as u64,
        lanes: view
            .lanes
            .iter()
            .map(|entry| RuntimeRelayLaneCount {
                lane: project_lane(entry.lane),
                wire_subscriptions: entry.wire_subscriptions as u64,
            })
            .collect(),
        omitted_lanes: view.omitted_lanes as u64,
        subscriptions: view
            .subscriptions
            .iter()
            .map(|entry| RuntimeRelaySubscription {
                filter: entry.filter.to_string(),
                coverage: entry.coverage.map(|window| RuntimeRelayCoverage {
                    from_seconds: window.from_seconds,
                    through_seconds: window.through_seconds,
                }),
            })
            .collect(),
        omitted_subscriptions: view.omitted_subscriptions as u64,
        events_by_kind: view
            .events_by_kind
            .iter()
            .map(|entry| RuntimeRelayKindCount {
                kind: entry.kind,
                events: entry.events,
            })
            .collect(),
        omitted_kinds: view.omitted_kinds as u64,
        supported_nips: view.nip11_supported_nips.as_ref().map(|nips| nips.to_vec()),
        omitted_supported_nips: view.omitted_supported_nips as u64,
        nip11_document_revision: view
            .nip11_document_revision
            .as_ref()
            .map(|value| value.to_string()),
        nip11_freshness: view.nip11_freshness.as_ref().map(|value| value.to_string()),
        nip11_last_error: view
            .nip11_last_error
            .as_ref()
            .map(|value| value.to_string()),
        nip77_advertisement: view.nip77_advertisement.to_string(),
        nip77_behavior: view.nip77_behavior.to_string(),
        nip77_handoff: view.nip77_handoff.to_string(),
    }
}

fn project_lane(lane: DiagnosticsLane) -> RuntimeRelayLane {
    match lane {
        DiagnosticsLane::Nip65Write => RuntimeRelayLane::Nip65Write,
        DiagnosticsLane::Nip65Read => RuntimeRelayLane::Nip65Read,
        DiagnosticsLane::Hint => RuntimeRelayLane::Hint,
        DiagnosticsLane::Provenance => RuntimeRelayLane::Provenance,
        DiagnosticsLane::UserConfigured => RuntimeRelayLane::UserConfigured,
        DiagnosticsLane::IndexerDiscovery => RuntimeRelayLane::IndexerDiscovery,
        DiagnosticsLane::GroupHost => RuntimeRelayLane::GroupHost,
        DiagnosticsLane::DmInbox => RuntimeRelayLane::DmInbox,
        DiagnosticsLane::AppRelay => RuntimeRelayLane::AppRelay,
        DiagnosticsLane::Fallback => RuntimeRelayLane::Fallback,
        DiagnosticsLane::ExplicitPinned => RuntimeRelayLane::ExplicitPinned,
    }
}

fn project_diagnostics_error(error: &AdapterRelayDiagnosticsError) -> RuntimeRefusal {
    let code = match error {
        AdapterRelayDiagnosticsError::InvalidLimits => "diagnostics-limits-invalid",
        AdapterRelayDiagnosticsError::ObservationRefused { .. } => "diagnostics-refused",
        AdapterRelayDiagnosticsError::ObservationEnded => "diagnostics-ended",
        AdapterRelayDiagnosticsError::ObservationCapacity { .. } => "diagnostics-capacity",
    };
    RuntimeRefusal {
        code: code.to_owned(),
        detail: error.to_string(),
        occurred_at_millis: now_millis(),
    }
}
