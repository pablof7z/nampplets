//! Test fixtures shared across the catalog-resolver test suite, split by
//! concern: [`resolution`] (core resolve/offline/redirect/cancel behavior),
//! [`review`] (review lifecycle and blocked HTTPS cancellation),
//! [`cancellation`] (blocked lookup cancellation lifecycle), and
//! [`acquisition`] (raw Rust HTTPS port behavior).

use std::{
    fmt,
    net::{IpAddr, Ipv4Addr},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use nmp_native_artifact::{
    ArtifactLimits, ArtifactSourcePolicy, FileArtifactCache, ManifestCoordinate,
};
use parking_lot::{Condvar, Mutex};
use tempfile::TempDir;

use crate::{
    CancellationToken, CatalogResolver, CoordinateLookupFact, HttpsAcquisitionCompletion,
    HttpsAcquisitionOperation, HttpsAcquisitionPort, HttpsFetchRequest, HttpsFetchResponse,
    HttpsPortError, LookupPortError, ManifestLookupCompletion, ManifestLookupOperation,
    ManifestLookupPort, ManifestLookupRequest, ManifestLookupResponse, MemorySealedArtifactCache,
    ResolverLimits,
};

mod acquisition;
mod cancellation;
mod resolution;
mod review;

const EVENT: &[u8] =
    include_bytes!("../../../../conformance/napplet-corpus/published/good-morning/event.json");
const INDEX: &[u8] =
    include_bytes!("../../../../conformance/napplet-corpus/published/good-morning/index.html");
const AUTHOR: &str = "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5";
const EVENT_ID: &str = "b330bfaefd2ddf268ebe4196403e6163533c54f41dabc3518bdc1a896c68f40e";
const AGGREGATE: &str = "828a6df02afd56782ea20f805084acce65c53f7c37554948c1e0a64aa5a2b0a8";
const PUBLIC_ADDRESS: IpAddr = IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34));

#[derive(Debug)]
struct FixtureLookup {
    calls: AtomicUsize,
}

#[derive(Debug)]
struct CompletedLookupOperation;

impl ManifestLookupOperation for CompletedLookupOperation {
    fn cancel(&self) {}
}

impl ManifestLookupPort for FixtureLookup {
    fn start_lookup(
        &self,
        _request: ManifestLookupRequest,
        completion: ManifestLookupCompletion,
    ) -> Result<Arc<dyn ManifestLookupOperation>, LookupPortError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        assert!(completion.resolve(Ok(ManifestLookupResponse::found(
            EVENT,
            vec![
                CoordinateLookupFact::observed("author-outbox", 1),
                CoordinateLookupFact::selected("nmp", EVENT_ID),
            ],
        ))));
        Ok(Arc::new(CompletedLookupOperation))
    }
}

#[derive(Clone, Debug)]
enum TransportMode {
    Good,
    Redirect,
    RedirectOnce,
    RedirectToPrivate,
    Private,
    Confused,
    Oversize,
}

#[derive(Debug)]
struct FixtureTransport {
    calls: AtomicUsize,
    mode: TransportMode,
}

#[derive(Debug)]
struct CompletedFetchOperation;

impl HttpsAcquisitionOperation for CompletedFetchOperation {
    fn cancel(&self) {}
}

impl HttpsAcquisitionPort for FixtureTransport {
    fn start_fetch(
        &self,
        request: HttpsFetchRequest,
        completion: HttpsAcquisitionCompletion,
    ) -> Result<Arc<dyn HttpsAcquisitionOperation>, HttpsPortError> {
        let call_number = self.calls.fetch_add(1, Ordering::Relaxed) + 1;
        let (effective, status, redirect, addresses, body) = match self.mode {
            TransportMode::Good => (
                Arc::from(request.url()),
                200,
                None,
                Arc::from([PUBLIC_ADDRESS]),
                Arc::<[u8]>::from(INDEX),
            ),
            TransportMode::Redirect => (
                Arc::from(request.url()),
                302,
                Some(Arc::from("https://evil.example/blob")),
                Arc::from([PUBLIC_ADDRESS]),
                Arc::<[u8]>::from([]),
            ),
            TransportMode::RedirectOnce if call_number == 1 => (
                Arc::from(request.url()),
                302,
                Some(Arc::from("https://mirror.example/blob")),
                Arc::from([PUBLIC_ADDRESS]),
                Arc::<[u8]>::from([]),
            ),
            TransportMode::RedirectOnce => (
                Arc::from(request.url()),
                200,
                None,
                Arc::from([PUBLIC_ADDRESS]),
                Arc::<[u8]>::from(INDEX),
            ),
            TransportMode::RedirectToPrivate => (
                Arc::from(request.url()),
                302,
                Some(Arc::from("https://127.0.0.1/blob")),
                Arc::from([PUBLIC_ADDRESS]),
                Arc::<[u8]>::from([]),
            ),
            TransportMode::Private => (
                Arc::from(request.url()),
                200,
                None,
                Arc::from([IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))]),
                Arc::<[u8]>::from(INDEX),
            ),
            TransportMode::Confused => (
                Arc::from("https://other.example/blob"),
                200,
                None,
                Arc::from([PUBLIC_ADDRESS]),
                Arc::<[u8]>::from(INDEX),
            ),
            TransportMode::Oversize => (
                Arc::from(request.url()),
                200,
                None,
                Arc::from([PUBLIC_ADDRESS]),
                Arc::<[u8]>::from(vec![0; request.maximum_bytes() + 1]),
            ),
        };
        assert!(completion.resolve(Ok(HttpsFetchResponse::new(
            effective, status, redirect, addresses, body,
        ))));
        Ok(Arc::new(CompletedFetchOperation))
    }
}

struct Fixture {
    temp: TempDir,
    lookup: Arc<FixtureLookup>,
    transport: Arc<FixtureTransport>,
    sealed: Arc<MemorySealedArtifactCache>,
}

impl fmt::Debug for Fixture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Fixture").finish_non_exhaustive()
    }
}

impl Fixture {
    fn new(mode: TransportMode) -> Self {
        Self {
            temp: TempDir::new().expect("temp"),
            lookup: Arc::new(FixtureLookup {
                calls: AtomicUsize::new(0),
            }),
            transport: Arc::new(FixtureTransport {
                calls: AtomicUsize::new(0),
                mode,
            }),
            sealed: Arc::new(
                MemorySealedArtifactCache::new(4, 64 * 1_024 * 1_024).expect("bounded cache"),
            ),
        }
    }

    fn coordinate() -> ManifestCoordinate {
        ManifestCoordinate::named(AUTHOR, "good-morning").expect("coordinate")
    }

    fn resolver(&self, limits: ResolverLimits) -> CatalogResolver {
        let artifact_cache = Arc::new(
            FileArtifactCache::open(self.temp.path().join("artifacts")).expect("artifact cache"),
        );
        CatalogResolver::new(
            limits,
            ArtifactLimits::default(),
            ArtifactSourcePolicy::manifest_https_only(8).expect("source policy"),
            self.lookup.clone(),
            self.transport.clone(),
            artifact_cache,
            self.sealed.clone(),
        )
        .expect("resolver")
    }

    fn with_resolver<T>(&self, operation: impl FnOnce(&CatalogResolver) -> T) -> T {
        let resolver = self.resolver(ResolverLimits::default());
        operation(&resolver)
    }
}
