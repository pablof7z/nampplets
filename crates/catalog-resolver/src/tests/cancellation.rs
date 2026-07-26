//! Event-driven cancellation proof for a blocked NMP-facing manifest lookup.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use nmp_native_artifact::{ArtifactLimits, ArtifactSourcePolicy, FileArtifactCache};
use parking_lot::{Condvar, Mutex};

use crate::ResolveError;

use super::*;

// This is a test-safety deadline, not a product performance budget.
const TEST_SAFETY_DEADLINE: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LifecycleState {
    LookupStarted,
    TokenCancelled,
    OperationCancelled,
    ResolverResultSent,
    WorkerJoined,
}

#[derive(Debug, Default)]
struct LifecycleProbe {
    observed: Mutex<Vec<LifecycleState>>,
    changed: Condvar,
}

impl LifecycleProbe {
    fn record(&self, state: LifecycleState) {
        self.observed.lock().push(state);
        self.changed.notify_all();
    }

    fn record_after(&self, state: LifecycleState, action: impl FnOnce()) {
        let mut observed = self.observed.lock();
        action();
        observed.push(state);
        self.changed.notify_all();
    }

    fn wait_for(&self, expected: LifecycleState) {
        let deadline = Instant::now() + TEST_SAFETY_DEADLINE;
        let mut observed = self.observed.lock();
        while !observed.contains(&expected) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || self.changed.wait_for(&mut observed, remaining).timed_out() {
                panic!(
                    "timed out waiting for {expected:?}; last observed lifecycle state: {:?}",
                    observed.last()
                );
            }
        }
    }

    fn last_observed(&self) -> Option<LifecycleState> {
        self.observed.lock().last().copied()
    }

    fn snapshot(&self) -> Vec<LifecycleState> {
        self.observed.lock().clone()
    }
}

#[derive(Debug)]
struct BlockingLookup {
    lifecycle: Arc<LifecycleProbe>,
    cancellation_count: Arc<AtomicUsize>,
}

#[derive(Debug)]
struct BlockingLookupOperation {
    lifecycle: Arc<LifecycleProbe>,
    cancellation_count: Arc<AtomicUsize>,
}

impl ManifestLookupOperation for BlockingLookupOperation {
    fn cancel(&self) {
        self.cancellation_count.fetch_add(1, Ordering::Relaxed);
        self.lifecycle.record(LifecycleState::OperationCancelled);
    }
}

impl ManifestLookupPort for BlockingLookup {
    fn start_lookup(
        &self,
        _request: ManifestLookupRequest,
        _completion: ManifestLookupCompletion,
    ) -> Result<Arc<dyn ManifestLookupOperation>, LookupPortError> {
        self.lifecycle.record(LifecycleState::LookupStarted);
        Ok(Arc::new(BlockingLookupOperation {
            lifecycle: Arc::clone(&self.lifecycle),
            cancellation_count: Arc::clone(&self.cancellation_count),
        }))
    }
}

#[test]
fn cancellation_wakes_a_blocked_lookup_and_cancels_its_nmp_operation() {
    let fixture = Fixture::new(TransportMode::Good);
    let lifecycle = Arc::new(LifecycleProbe::default());
    let cancellation_count = Arc::new(AtomicUsize::new(0));
    let resolver = Arc::new(
        CatalogResolver::new(
            ResolverLimits::default(),
            ArtifactLimits::default(),
            ArtifactSourcePolicy::manifest_https_only(8).expect("source policy"),
            Arc::new(BlockingLookup {
                lifecycle: Arc::clone(&lifecycle),
                cancellation_count: Arc::clone(&cancellation_count),
            }),
            fixture.transport.clone(),
            Arc::new(
                FileArtifactCache::open(fixture.temp.path().join("artifacts"))
                    .expect("artifact cache"),
            ),
            fixture.sealed.clone(),
        )
        .expect("resolver"),
    );
    let cancellation = CancellationToken::default();
    let thread_token = cancellation.clone();
    let thread_resolver = Arc::clone(&resolver);
    let thread_lifecycle = Arc::clone(&lifecycle);
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let (finished_tx, finished_rx) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let result = thread_resolver.begin_review(&Fixture::coordinate(), &thread_token);
        result_tx.send(result.map(|_| ())).expect("send result");
        thread_lifecycle.record(LifecycleState::ResolverResultSent);
        finished_tx.send(()).expect("send worker completion");
    });

    lifecycle.wait_for(LifecycleState::LookupStarted);
    lifecycle.record_after(LifecycleState::TokenCancelled, || cancellation.cancel());
    assert!(cancellation.is_cancelled());
    lifecycle.wait_for(LifecycleState::OperationCancelled);
    lifecycle.wait_for(LifecycleState::ResolverResultSent);
    let result = result_rx
        .recv_timeout(TEST_SAFETY_DEADLINE)
        .unwrap_or_else(|error| {
            panic!(
                "resolver result unavailable ({error}); last observed lifecycle state: {:?}",
                lifecycle.last_observed()
            )
        });
    assert!(matches!(result, Err(ResolveError::Cancelled)));
    finished_rx
        .recv_timeout(TEST_SAFETY_DEADLINE)
        .unwrap_or_else(|error| {
            panic!(
                "worker did not finish ({error}); last observed lifecycle state: {:?}",
                lifecycle.last_observed()
            )
        });
    worker.join().expect("worker");
    lifecycle.record(LifecycleState::WorkerJoined);

    assert_eq!(cancellation_count.load(Ordering::Relaxed), 1);
    assert_eq!(
        lifecycle.snapshot(),
        vec![
            LifecycleState::LookupStarted,
            LifecycleState::TokenCancelled,
            LifecycleState::OperationCancelled,
            LifecycleState::ResolverResultSent,
            LifecycleState::WorkerJoined,
        ]
    );
}
