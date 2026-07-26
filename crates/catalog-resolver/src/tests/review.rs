//! Review lifecycle tests and cancellation-wakes-a-blocked-port tests.

use std::{
    sync::{atomic::Ordering, mpsc},
    thread,
    time::Duration,
};

use nmp_native_artifact::ArtifactSourcePolicy;

use crate::{AcquisitionRefusal, ResolveError};

use super::*;

#[test]
fn review_freezes_exact_signed_selection_across_a_later_replacement() {
    #[derive(Debug)]
    struct MutableLookup {
        calls: AtomicUsize,
        event: Mutex<Arc<[u8]>>,
    }

    impl ManifestLookupPort for MutableLookup {
        fn start_lookup(
            &self,
            _request: ManifestLookupRequest,
            completion: ManifestLookupCompletion,
        ) -> Result<Arc<dyn ManifestLookupOperation>, LookupPortError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            assert!(completion.resolve(Ok(ManifestLookupResponse::found(
                Arc::clone(&self.event.lock()),
                vec![
                    CoordinateLookupFact::observed("author-outbox", 1),
                    CoordinateLookupFact::selected("nmp", EVENT_ID),
                ],
            ))));
            Ok(Arc::new(CompletedLookupOperation))
        }
    }

    let fixture = Fixture::new(TransportMode::Good);
    let lookup = Arc::new(MutableLookup {
        calls: AtomicUsize::new(0),
        event: Mutex::new(Arc::from(EVENT)),
    });
    let resolver = CatalogResolver::new(
        ResolverLimits::default(),
        ArtifactLimits::default(),
        ArtifactSourcePolicy::manifest_https_only(8).expect("source policy"),
        lookup.clone(),
        fixture.transport.clone(),
        Arc::new(
            FileArtifactCache::open(fixture.temp.path().join("artifacts")).expect("artifact cache"),
        ),
        fixture.sealed.clone(),
    )
    .expect("resolver");
    let review = resolver
        .begin_review(&Fixture::coordinate(), &CancellationToken::default())
        .expect("review A");
    assert_eq!(review.summary().event_id().as_str(), EVENT_ID);
    assert_eq!(review.summary().aggregate().as_str(), AGGREGATE);

    *lookup.event.lock() = Arc::from(&b"replacement B must never be read"[..]);
    let installed = resolver
        .confirm_review(&review, &CancellationToken::default())
        .expect("confirm exact A");
    assert_eq!(installed.handle().index().event_id().as_str(), EVENT_ID);
    assert_eq!(lookup.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn review_capacity_cancel_drop_and_stale_refusal_are_exact() {
    let fixture = Fixture::new(TransportMode::Good);
    let resolver = fixture.resolver(ResolverLimits {
        maximum_reviews: 1,
        ..ResolverLimits::default()
    });
    let first = resolver
        .begin_review(&Fixture::coordinate(), &CancellationToken::default())
        .expect("first review");
    assert!(matches!(
        resolver.begin_review(&Fixture::coordinate(), &CancellationToken::default()),
        Err(ResolveError::ReviewSaturated { maximum: 1 })
    ));
    first.cancel().expect("cancel first");
    assert!(matches!(
        resolver.confirm_review(&first, &CancellationToken::default()),
        Err(ResolveError::ReviewStale)
    ));

    let second = resolver
        .begin_review(&Fixture::coordinate(), &CancellationToken::default())
        .expect("capacity released by cancel");
    drop(second);
    resolver
        .begin_review(&Fixture::coordinate(), &CancellationToken::default())
        .expect("capacity released by drop");
}

#[derive(Debug, Default)]
struct BlockingTransportState {
    started: Mutex<bool>,
    started_ready: Condvar,
    cancelled: Mutex<bool>,
    cancelled_ready: Condvar,
}

impl BlockingTransportState {
    fn wait_started(&self) {
        let mut started = self.started.lock();
        while !*started {
            self.started_ready.wait(&mut started);
        }
    }

    fn wait_cancelled(&self) {
        let mut cancelled = self.cancelled.lock();
        while !*cancelled {
            self.cancelled_ready.wait(&mut cancelled);
        }
    }
}

#[derive(Debug)]
struct BlockingTransport {
    state: Arc<BlockingTransportState>,
}

#[derive(Debug)]
struct BlockingTransportOperation {
    state: Arc<BlockingTransportState>,
}

impl HttpsAcquisitionOperation for BlockingTransportOperation {
    fn cancel(&self) {
        *self.state.cancelled.lock() = true;
        self.state.cancelled_ready.notify_all();
    }
}

impl HttpsAcquisitionPort for BlockingTransport {
    fn start_fetch(
        &self,
        _request: HttpsFetchRequest,
        _completion: HttpsAcquisitionCompletion,
    ) -> Result<Arc<dyn HttpsAcquisitionOperation>, HttpsPortError> {
        *self.state.started.lock() = true;
        self.state.started_ready.notify_all();
        Ok(Arc::new(BlockingTransportOperation {
            state: Arc::clone(&self.state),
        }))
    }
}

#[test]
fn cancellation_wakes_blocked_https_and_aborts_the_owned_operation() {
    let fixture = Fixture::new(TransportMode::Good);
    let state = Arc::new(BlockingTransportState::default());
    let resolver = Arc::new(
        CatalogResolver::new(
            ResolverLimits::default(),
            ArtifactLimits::default(),
            ArtifactSourcePolicy::manifest_https_only(8).expect("source policy"),
            fixture.lookup.clone(),
            Arc::new(BlockingTransport {
                state: Arc::clone(&state),
            }),
            Arc::new(
                FileArtifactCache::open(fixture.temp.path().join("artifacts"))
                    .expect("artifact cache"),
            ),
            fixture.sealed.clone(),
        )
        .expect("resolver"),
    );
    let review = resolver
        .begin_review(&Fixture::coordinate(), &CancellationToken::default())
        .expect("review");
    let cancellation = CancellationToken::default();
    let thread_token = cancellation.clone();
    let thread_resolver = Arc::clone(&resolver);
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let result = thread_resolver.confirm_review(&review, &thread_token);
        result_tx.send(result.map(|_| ())).expect("send result");
    });

    state.wait_started();
    cancellation.cancel();
    let result = result_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("cancellation must wake the HTTPS wait");
    assert!(matches!(
        result,
        Err(ResolveError::Acquisition {
            reason: AcquisitionRefusal::Cancelled,
            ..
        })
    ));
    state.wait_cancelled();
    worker.join().expect("worker");
}
