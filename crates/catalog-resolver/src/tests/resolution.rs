//! Core resolve/offline/redirect/cancel behavior tests.

use std::sync::atomic::Ordering;

use nmp_native_artifact::{INDEX_PATH, Sha256Digest};

use crate::{
    AcquisitionRefusal, ResolutionOrigin, ResolveError, SealedArtifactCache, SealedArtifactKey,
    https::validate_candidate, resolver::Admission,
};

use super::*;

#[test]
fn online_resolution_seals_then_offline_reinstall_uses_no_ports() {
    let fixture = Fixture::new(TransportMode::Good);
    fixture.with_resolver(|resolver| {
        let online = resolver
            .resolve(&Fixture::coordinate(), &CancellationToken::default())
            .expect("online resolution");
        assert_eq!(online.origin(), ResolutionOrigin::OnlineVerified);
        assert_eq!(
            online
                .handle()
                .read_verified(INDEX_PATH, 4 * 1_024 * 1_024)
                .expect("sealed bytes"),
            INDEX
        );
        assert_eq!(online.acquisition_facts().len(), 1);

        let offline = resolver
            .resolve_offline(
                &Fixture::coordinate(),
                &Sha256Digest::parse(AGGREGATE).expect("aggregate"),
                &CancellationToken::default(),
            )
            .expect("offline resolution");
        assert_eq!(offline.origin(), ResolutionOrigin::OfflineSealed);
        assert_eq!(
            offline
                .handle()
                .read_verified(INDEX_PATH, 4 * 1_024 * 1_024)
                .expect("offline sealed bytes"),
            INDEX
        );
    });
    assert_eq!(fixture.lookup.calls.load(Ordering::Relaxed), 1);
    assert_eq!(fixture.transport.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn redirect_private_dns_source_confusion_and_oversize_fail_before_retention() {
    let cases = [
        (TransportMode::Redirect, "hops"),
        (TransportMode::RedirectToPrivate, "public address"),
        (TransportMode::Private, "public address"),
        (TransportMode::Confused, "effective response URL"),
        (TransportMode::Oversize, "maximum"),
    ];
    for (mode, expected) in cases {
        let fixture = Fixture::new(mode);
        fixture.with_resolver(|resolver| {
            let error = resolver
                .resolve(&Fixture::coordinate(), &CancellationToken::default())
                .expect_err("policy refusal");
            assert!(error.to_string().contains(expected), "{error}");
            assert!(matches!(error, ResolveError::Acquisition { .. }));
            assert!(matches!(
                resolver.resolve_offline(
                    &Fixture::coordinate(),
                    &Sha256Digest::parse(AGGREGATE).expect("aggregate"),
                    &CancellationToken::default(),
                ),
                Err(ResolveError::OfflineMiss { .. })
            ));
        });
    }
}

#[test]
fn redirect_to_a_revalidated_public_https_target_is_followed() {
    let fixture = Fixture::new(TransportMode::RedirectOnce);
    fixture.with_resolver(|resolver| {
        let online = resolver
            .resolve(&Fixture::coordinate(), &CancellationToken::default())
            .expect("a redirect to a policy-approved target is followed");
        assert_eq!(
            online
                .handle()
                .read_verified(INDEX_PATH, 4 * 1_024 * 1_024)
                .expect("sealed bytes"),
            INDEX
        );
    });
    assert_eq!(fixture.transport.calls.load(Ordering::Relaxed), 2);
}

#[test]
fn cancelled_operation_never_calls_lookup_or_transport() {
    let fixture = Fixture::new(TransportMode::Good);
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    fixture.with_resolver(|resolver| {
        assert!(matches!(
            resolver.resolve(&Fixture::coordinate(), &cancellation),
            Err(ResolveError::Cancelled)
        ));
    });
    assert_eq!(fixture.lookup.calls.load(Ordering::Relaxed), 0);
    assert_eq!(fixture.transport.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn malformed_lookup_evidence_is_refused_before_transport() {
    #[derive(Debug)]
    struct InvalidLookup;
    impl ManifestLookupPort for InvalidLookup {
        fn start_lookup(
            &self,
            _request: ManifestLookupRequest,
            completion: ManifestLookupCompletion,
        ) -> Result<Arc<dyn ManifestLookupOperation>, LookupPortError> {
            assert!(completion.resolve(Ok(ManifestLookupResponse::found(
                EVENT,
                vec![CoordinateLookupFact::shortfall("", "missing source")],
            ))));
            Ok(Arc::new(CompletedLookupOperation))
        }
    }
    let fixture = Fixture::new(TransportMode::Good);
    let artifact_cache = Arc::new(
        FileArtifactCache::open(fixture.temp.path().join("artifacts")).expect("artifact cache"),
    );
    let resolver = CatalogResolver::new(
        ResolverLimits::default(),
        ArtifactLimits::default(),
        ArtifactSourcePolicy::manifest_https_only(8).expect("source policy"),
        Arc::new(InvalidLookup),
        fixture.transport.clone(),
        artifact_cache,
        fixture.sealed.clone(),
    )
    .expect("resolver");
    assert!(matches!(
        resolver.resolve(&Fixture::coordinate(), &CancellationToken::default()),
        Err(ResolveError::InvalidLookupFact)
    ));
    assert_eq!(fixture.transport.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn literal_private_https_candidate_is_refused_without_network() {
    assert!(matches!(
        validate_candidate("https://127.0.0.1/blob", 2_048),
        Err(AcquisitionRefusal::NonPublicAddress { .. })
    ));
    assert!(matches!(
        validate_candidate("https://[::1]/blob", 2_048),
        Err(AcquisitionRefusal::NonPublicAddress { .. })
    ));
}

#[test]
fn bounded_cache_is_immutable_for_an_exact_aggregate() {
    let fixture = Fixture::new(TransportMode::Good);
    fixture.with_resolver(|resolver| {
        let first = resolver
            .resolve(&Fixture::coordinate(), &CancellationToken::default())
            .expect("first");
        let key = SealedArtifactKey::for_coordinate(
            &Fixture::coordinate(),
            first.handle().index().aggregate().clone(),
        );
        fixture
            .sealed
            .retain(&key, first.handle())
            .expect("idempotent");
        assert_eq!(fixture.sealed.state.lock().entries.len(), 1);
    });
}

#[test]
fn admission_is_finite_and_has_no_waiting_queue() {
    let admission = Admission::new(1);
    let _permit = admission.reserve().expect("first permit");
    assert!(matches!(
        admission.reserve(),
        Err(ResolveError::Saturated { maximum: 1 })
    ));
}

#[test]
fn not_found_preserves_scoped_shortfall_facts() {
    #[derive(Debug)]
    struct EmptyLookup;
    impl ManifestLookupPort for EmptyLookup {
        fn start_lookup(
            &self,
            _request: ManifestLookupRequest,
            completion: ManifestLookupCompletion,
        ) -> Result<Arc<dyn ManifestLookupOperation>, LookupPortError> {
            assert!(
                completion.resolve(Ok(ManifestLookupResponse::not_found(vec![
                    CoordinateLookupFact::shortfall("author-outbox", "relay unavailable"),
                ])))
            );
            Ok(Arc::new(CompletedLookupOperation))
        }
    }
    let fixture = Fixture::new(TransportMode::Good);
    let artifact_cache = Arc::new(
        FileArtifactCache::open(fixture.temp.path().join("artifacts")).expect("artifact cache"),
    );
    let resolver = CatalogResolver::new(
        ResolverLimits::default(),
        ArtifactLimits::default(),
        ArtifactSourcePolicy::manifest_https_only(8).expect("source policy"),
        Arc::new(EmptyLookup),
        fixture.transport.clone(),
        artifact_cache,
        fixture.sealed.clone(),
    )
    .expect("resolver");
    let error = resolver
        .resolve(&Fixture::coordinate(), &CancellationToken::default())
        .expect_err("not found");
    assert_eq!(
        error
            .lookup_facts()
            .expect("facts")
            .first()
            .expect("fact")
            .source(),
        "author-outbox"
    );
    assert_eq!(fixture.transport.calls.load(Ordering::Relaxed), 0);
}
