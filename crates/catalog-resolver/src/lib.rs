//! Bounded coordinate lookup and policy-checked artifact acquisition.
//!
//! NMP selects one canonical manifest event through [`ManifestLookupPort`].
//! This crate validates finite lookup evidence and raw HTTPS acquisition facts,
//! then delegates all signature, manifest, path-hash, aggregate, and immutable
//! byte handling to `nmp-native-artifact`.

mod redirect;

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    io::Cursor,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use nmp_native_artifact::{
    ArtifactLimits, ArtifactSourcePolicy, BlobFetchRequest, BlobFetchResponse, BlobSourceError,
    FileArtifactCache, ManifestBlobSource, ManifestCoordinate, ManifestEventVerifier, Sha256Digest,
    SignedArtifactResolver, VerifiedArtifactHandle, VerifiedArtifactIndex,
};
use parking_lot::{Condvar, Mutex};
use redirect::{ResponseAction, classify_response};
use thiserror::Error;
use url::{Host, Url};

const KIND_SNAPSHOT: u16 = 5_129;
const KIND_ROOT: u16 = 15_129;
const KIND_NAMED: u16 = 35_129;

/// Finite resolver-wide limits. Every value must be non-zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolverLimits {
    pub maximum_in_flight: usize,
    pub maximum_reviews: usize,
    pub maximum_lookup_facts: usize,
    pub maximum_acquisition_facts: usize,
    pub maximum_manifest_bytes: usize,
    pub maximum_resolved_addresses: usize,
    pub maximum_url_bytes: usize,
    pub maximum_source_label_bytes: usize,
    pub maximum_reason_bytes: usize,
    /// Redirect hops followed per candidate before acquisition refuses. Each
    /// hop target is revalidated with the same HTTPS-only, credential-free,
    /// public-address policy as the original candidate URL.
    pub maximum_redirect_hops: usize,
}

impl Default for ResolverLimits {
    fn default() -> Self {
        Self {
            maximum_in_flight: 4,
            maximum_reviews: 16,
            maximum_lookup_facts: 64,
            maximum_acquisition_facts: 4_096,
            maximum_manifest_bytes: 256 * 1_024,
            maximum_resolved_addresses: 16,
            maximum_url_bytes: 2_048,
            maximum_source_label_bytes: 256,
            maximum_reason_bytes: 512,
            maximum_redirect_hops: 5,
        }
    }
}

impl ResolverLimits {
    fn validate(self) -> Result<Self, ResolveError> {
        if self.maximum_in_flight == 0
            || self.maximum_reviews == 0
            || self.maximum_lookup_facts == 0
            || self.maximum_acquisition_facts == 0
            || self.maximum_manifest_bytes == 0
            || self.maximum_resolved_addresses == 0
            || self.maximum_url_bytes == 0
            || self.maximum_source_label_bytes == 0
            || self.maximum_reason_bytes == 0
            || self.maximum_redirect_hops == 0
        {
            return Err(ResolveError::InvalidLimits);
        }
        Ok(self)
    }
}

const MAXIMUM_CANCELLATION_WAKEUPS: usize = 8;

/// Cloneable event-driven cancellation shared across one bounded operation.
///
/// Cancellation wakes every registered resolver wait immediately. A token has
/// a finite listener ceiling and never creates a polling thread.
#[derive(Clone, Debug)]
pub struct CancellationToken {
    state: Arc<CancellationState>,
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self {
            state: Arc::new(CancellationState::default()),
        }
    }
}

#[derive(Debug, Default)]
struct CancellationState {
    cancelled: AtomicBool,
    next_registration: AtomicU64,
    wakeups: Mutex<BTreeMap<u64, Arc<CancellationWake>>>,
}

#[derive(Debug)]
struct CancellationWake {
    ready: Arc<Condvar>,
}

#[derive(Debug)]
struct CancellationRegistration {
    state: Arc<CancellationState>,
    id: Option<u64>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        if self.state.cancelled.swap(true, Ordering::AcqRel) {
            return;
        }
        let wakeups = {
            let mut registered = self.state.wakeups.lock();
            std::mem::take(&mut *registered)
        };
        for wakeup in wakeups.into_values() {
            wakeup.ready.notify_all();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    fn register(&self, ready: Arc<Condvar>) -> Result<CancellationRegistration, ResolveError> {
        let mut wakeups = self.state.wakeups.lock();
        if self.is_cancelled() {
            drop(wakeups);
            ready.notify_all();
            return Ok(CancellationRegistration {
                state: Arc::clone(&self.state),
                id: None,
            });
        }
        if wakeups.len() >= MAXIMUM_CANCELLATION_WAKEUPS {
            return Err(ResolveError::CancellationSaturated {
                maximum: MAXIMUM_CANCELLATION_WAKEUPS,
            });
        }
        let id = self.state.next_registration.fetch_add(1, Ordering::Relaxed);
        wakeups.insert(id, Arc::new(CancellationWake { ready }));
        Ok(CancellationRegistration {
            state: Arc::clone(&self.state),
            id: Some(id),
        })
    }
}

impl Drop for CancellationRegistration {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            self.state.wakeups.lock().remove(&id);
        }
    }
}

#[derive(Clone, Debug)]
pub struct ManifestLookupRequest {
    coordinate: ManifestCoordinate,
    maximum_event_bytes: usize,
    maximum_facts: usize,
}

impl ManifestLookupRequest {
    pub fn coordinate(&self) -> &ManifestCoordinate {
        &self.coordinate
    }

    pub fn maximum_event_bytes(&self) -> usize {
        self.maximum_event_bytes
    }

    pub fn maximum_facts(&self) -> usize {
        self.maximum_facts
    }
}

/// A source-scoped fact. `Observed` never means globally complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoordinateLookupFact {
    source: Arc<str>,
    state: CoordinateLookupState,
}

impl CoordinateLookupFact {
    pub fn observed(source: impl Into<Arc<str>>, rows: usize) -> Self {
        Self {
            source: source.into(),
            state: CoordinateLookupState::Observed { rows },
        }
    }

    pub fn shortfall(source: impl Into<Arc<str>>, reason: impl Into<Arc<str>>) -> Self {
        Self {
            source: source.into(),
            state: CoordinateLookupState::Shortfall {
                reason: reason.into(),
            },
        }
    }

    pub fn selected(source: impl Into<Arc<str>>, event_id: impl Into<Arc<str>>) -> Self {
        Self {
            source: source.into(),
            state: CoordinateLookupState::Selected {
                event_id: event_id.into(),
            },
        }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn state(&self) -> &CoordinateLookupState {
        &self.state
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoordinateLookupState {
    Observed { rows: usize },
    Shortfall { reason: Arc<str> },
    Selected { event_id: Arc<str> },
}

#[derive(Clone, Debug)]
pub struct ManifestLookupResponse {
    selected_event_json: Option<Arc<[u8]>>,
    facts: Arc<[CoordinateLookupFact]>,
}

impl ManifestLookupResponse {
    pub fn found(
        selected_event_json: impl Into<Arc<[u8]>>,
        facts: impl Into<Arc<[CoordinateLookupFact]>>,
    ) -> Self {
        Self {
            selected_event_json: Some(selected_event_json.into()),
            facts: facts.into(),
        }
    }

    pub fn not_found(facts: impl Into<Arc<[CoordinateLookupFact]>>) -> Self {
        Self {
            selected_event_json: None,
            facts: facts.into(),
        }
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("manifest lookup port failed: {reason}")]
pub struct LookupPortError {
    reason: Arc<str>,
}

impl LookupPortError {
    pub fn new(reason: impl Into<Arc<str>>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

/// One live NMP lookup registration. Cancellation must be idempotent and
/// nonblocking; it detaches the owned observation without joining app code.
pub trait ManifestLookupOperation: Send + Sync + fmt::Debug {
    fn cancel(&self);
}

#[derive(Clone, Debug)]
pub struct ManifestLookupCompletion {
    state: Arc<LookupCompletionState>,
}

#[derive(Debug)]
struct LookupCompletionState {
    result: Mutex<LookupCompletionResult>,
    ready: Arc<Condvar>,
}

#[derive(Debug)]
enum LookupCompletionResult {
    Pending,
    Ready(Result<ManifestLookupResponse, LookupPortError>),
    Closed,
}

impl ManifestLookupCompletion {
    fn pending() -> Self {
        Self {
            state: Arc::new(LookupCompletionState {
                result: Mutex::new(LookupCompletionResult::Pending),
                ready: Arc::new(Condvar::new()),
            }),
        }
    }

    /// Completes the lookup exactly once. `false` means the resolver already
    /// completed or cancelled the operation and the result was not retained.
    pub fn resolve(&self, result: Result<ManifestLookupResponse, LookupPortError>) -> bool {
        let accepted = {
            let mut state = self.state.result.lock();
            if !matches!(*state, LookupCompletionResult::Pending) {
                false
            } else {
                *state = LookupCompletionResult::Ready(result);
                true
            }
        };
        if accepted {
            self.state.ready.notify_all();
        }
        accepted
    }

    fn wait(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<ManifestLookupResponse, ResolveError> {
        let _registration = cancellation.register(Arc::clone(&self.state.ready))?;
        let mut state = self.state.result.lock();
        loop {
            if cancellation.is_cancelled() {
                *state = LookupCompletionResult::Closed;
                return Err(ResolveError::Cancelled);
            }
            match std::mem::replace(&mut *state, LookupCompletionResult::Closed) {
                LookupCompletionResult::Pending => {
                    *state = LookupCompletionResult::Pending;
                    self.state.ready.wait(&mut state);
                }
                LookupCompletionResult::Ready(result) => {
                    return result.map_err(|error| ResolveError::Lookup {
                        reason: error.reason,
                    });
                }
                LookupCompletionResult::Closed => return Err(ResolveError::LookupClosed),
            }
        }
    }
}

/// NMP/public-facade boundary. Implementations start one observation and
/// return its cancellation ownership without blocking. They return NMP's
/// selected row and scoped evidence through `completion`; relay choice and
/// replacement policy never cross this boundary.
pub trait ManifestLookupPort: Send + Sync + fmt::Debug {
    fn start_lookup(
        &self,
        request: ManifestLookupRequest,
        completion: ManifestLookupCompletion,
    ) -> Result<Arc<dyn ManifestLookupOperation>, LookupPortError>;
}

#[derive(Clone, Debug)]
pub struct HttpsFetchRequest {
    url: Arc<str>,
    maximum_bytes: usize,
}

impl HttpsFetchRequest {
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn maximum_bytes(&self) -> usize {
        self.maximum_bytes
    }
}

#[derive(Clone, Debug)]
pub struct HttpsFetchResponse {
    effective_url: Arc<str>,
    status: u16,
    redirect_location: Option<Arc<str>>,
    resolved_addresses: Arc<[IpAddr]>,
    body: Arc<[u8]>,
}

impl HttpsFetchResponse {
    pub fn new(
        effective_url: impl Into<Arc<str>>,
        status: u16,
        redirect_location: Option<Arc<str>>,
        resolved_addresses: impl Into<Arc<[IpAddr]>>,
        body: impl Into<Arc<[u8]>>,
    ) -> Self {
        Self {
            effective_url: effective_url.into(),
            status,
            redirect_location,
            resolved_addresses: resolved_addresses.into(),
            body: body.into(),
        }
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum HttpsPortError {
    #[error("HTTPS acquisition port failed: {reason}")]
    Transport { reason: Arc<str> },
    #[error("HTTPS acquisition was refused before connect: {reason}")]
    Refused { reason: AcquisitionRefusal },
    #[error("HTTPS executor is saturated at {maximum} operations")]
    Saturated { maximum: usize },
}

impl HttpsPortError {
    pub fn new(reason: impl Into<Arc<str>>) -> Self {
        Self::Transport {
            reason: reason.into(),
        }
    }
}

pub trait HttpsAcquisitionOperation: Send + Sync + fmt::Debug {
    fn cancel(&self);
}

#[derive(Clone, Debug)]
pub struct HttpsAcquisitionCompletion {
    state: Arc<HttpsCompletionState>,
}

#[derive(Debug)]
struct HttpsCompletionState {
    result: Mutex<HttpsCompletionResult>,
    ready: Arc<Condvar>,
}

#[derive(Debug)]
enum HttpsCompletionResult {
    Pending,
    Ready(Result<HttpsFetchResponse, HttpsPortError>),
    Closed,
}

#[derive(Debug)]
enum HttpsWaitError {
    Cancelled,
    Port(HttpsPortError),
    Closed,
    CancellationSaturated { maximum: usize },
}

impl HttpsAcquisitionCompletion {
    fn pending() -> Self {
        Self {
            state: Arc::new(HttpsCompletionState {
                result: Mutex::new(HttpsCompletionResult::Pending),
                ready: Arc::new(Condvar::new()),
            }),
        }
    }

    pub fn resolve(&self, result: Result<HttpsFetchResponse, HttpsPortError>) -> bool {
        let accepted = {
            let mut state = self.state.result.lock();
            if !matches!(*state, HttpsCompletionResult::Pending) {
                false
            } else {
                *state = HttpsCompletionResult::Ready(result);
                true
            }
        };
        if accepted {
            self.state.ready.notify_all();
        }
        accepted
    }

    fn wait(&self, cancellation: &CancellationToken) -> Result<HttpsFetchResponse, HttpsWaitError> {
        let _registration = cancellation
            .register(Arc::clone(&self.state.ready))
            .map_err(|error| match error {
                ResolveError::CancellationSaturated { maximum } => {
                    HttpsWaitError::CancellationSaturated { maximum }
                }
                _ => HttpsWaitError::Closed,
            })?;
        let mut state = self.state.result.lock();
        loop {
            if cancellation.is_cancelled() {
                *state = HttpsCompletionResult::Closed;
                return Err(HttpsWaitError::Cancelled);
            }
            match std::mem::replace(&mut *state, HttpsCompletionResult::Closed) {
                HttpsCompletionResult::Pending => {
                    *state = HttpsCompletionResult::Pending;
                    self.state.ready.wait(&mut state);
                }
                HttpsCompletionResult::Ready(result) => {
                    return result.map_err(HttpsWaitError::Port);
                }
                HttpsCompletionResult::Closed => return Err(HttpsWaitError::Closed),
            }
        }
    }
}

/// Raw HTTPS executor. Implementations start without blocking, disable
/// redirects, pin every connection to the exact reported DNS results while
/// preserving hostname TLS/SNI, and cap streaming reads to
/// `maximum_bytes + 1`.
pub trait HttpsAcquisitionPort: Send + Sync + fmt::Debug {
    fn start_fetch(
        &self,
        request: HttpsFetchRequest,
        completion: HttpsAcquisitionCompletion,
    ) -> Result<Arc<dyn HttpsAcquisitionOperation>, HttpsPortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcquisitionFact {
    logical_path: Arc<str>,
    source_url: Arc<str>,
    outcome: AcquisitionOutcome,
}

impl AcquisitionFact {
    pub fn logical_path(&self) -> &str {
        &self.logical_path
    }

    pub fn source_url(&self) -> &str {
        &self.source_url
    }

    pub fn outcome(&self) -> &AcquisitionOutcome {
        &self.outcome
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcquisitionOutcome {
    TransportFailed { reason: Arc<str> },
    HttpStatus { status: u16 },
    Refused { reason: AcquisitionRefusal },
    Succeeded { bytes: usize },
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AcquisitionRefusal {
    #[error("operation was cancelled")]
    Cancelled,
    #[error("HTTPS executor is saturated at {maximum} operations")]
    ExecutorSaturated { maximum: usize },
    #[error("cancellation wake capacity is saturated at {maximum} listeners")]
    CancellationCapacity { maximum: usize },
    #[error("candidate URL is invalid")]
    InvalidCandidate,
    #[error("candidate URL is not credential-free HTTPS")]
    NonHttps,
    #[error("candidate URL or DNS result is not a public address: {address}")]
    NonPublicAddress { address: IpAddr },
    #[error("HTTPS response has no resolved-address evidence")]
    MissingAddressEvidence,
    #[error("HTTPS response has {actual} resolved addresses; the maximum is {maximum}")]
    AddressLimit { actual: usize, maximum: usize },
    #[error("supported redirect response is missing a valid Location")]
    Redirect,
    #[error("redirect exceeded the maximum of {maximum} hops")]
    TooManyRedirects { maximum: usize },
    #[error("effective response URL differs from the exact requested candidate")]
    SourceConfusion,
    #[error("response is {actual} bytes; the maximum is {maximum}")]
    Oversize { actual: usize, maximum: usize },
    #[error("acquisition evidence reached its maximum of {maximum} facts")]
    EvidenceCapacity { maximum: usize },
    #[error("every finite approved source failed")]
    AllSourcesFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RustHttpsAcquisitionConfig {
    pub maximum_in_flight: usize,
    pub maximum_resolved_addresses: usize,
    pub maximum_url_bytes: usize,
    pub worker_threads: usize,
    pub deadline: Duration,
}

impl Default for RustHttpsAcquisitionConfig {
    fn default() -> Self {
        Self {
            maximum_in_flight: 4,
            maximum_resolved_addresses: 16,
            maximum_url_bytes: 2_048,
            worker_threads: 2,
            deadline: Duration::from_secs(15),
        }
    }
}

impl RustHttpsAcquisitionConfig {
    fn validate(self) -> Result<Self, HttpsPortError> {
        if self.maximum_in_flight == 0
            || self.maximum_resolved_addresses == 0
            || self.maximum_url_bytes == 0
            || self.worker_threads == 0
            || self.deadline.is_zero()
        {
            return Err(HttpsPortError::new(
                "Rust HTTPS executor limits must be finite and non-zero",
            ));
        }
        Ok(self)
    }
}

/// Production Rust HTTPS transport. A fixed Tokio runtime owns at most
/// `maximum_in_flight` zero-queue tasks. DNS is resolved once through the
/// system resolver, rejected by this crate's public-address policy before any
/// connect, then pinned into a redirect-disabled reqwest client under the
/// original URL hostname so TLS certificate validation and SNI are preserved.
pub struct RustHttpsAcquisitionPort {
    config: RustHttpsAcquisitionConfig,
    runtime: Option<tokio::runtime::Runtime>,
    admission: Arc<HttpsAdmission>,
}

impl fmt::Debug for RustHttpsAcquisitionPort {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustHttpsAcquisitionPort")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl RustHttpsAcquisitionPort {
    pub fn new(config: RustHttpsAcquisitionConfig) -> Result<Self, HttpsPortError> {
        let config = config.validate()?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(config.worker_threads)
            .max_blocking_threads(config.maximum_in_flight)
            .enable_io()
            .enable_time()
            .thread_name("nampplets-artifact-https")
            .build()
            .map_err(|error| HttpsPortError::new(error.to_string()))?;
        Ok(Self {
            config,
            runtime: Some(runtime),
            admission: Arc::new(HttpsAdmission::new(config.maximum_in_flight)),
        })
    }
}

impl Drop for RustHttpsAcquisitionPort {
    /// Tokio refuses to synchronously shut down a runtime from a thread that
    /// is itself executing inside another runtime's async context (observed
    /// when the last handle to this port is released from the FFI
    /// observation thread). Shutting the runtime down on a fresh, bare
    /// thread avoids that panic regardless of which thread drops this port.
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            let _ = thread::Builder::new()
                .name("nampplets-artifact-https-shutdown".to_owned())
                .spawn(move || drop(runtime));
        }
    }
}

#[derive(Debug)]
struct RustHttpsOperation {
    abort: tokio::task::AbortHandle,
}

impl HttpsAcquisitionOperation for RustHttpsOperation {
    fn cancel(&self) {
        self.abort.abort();
    }
}

impl Drop for RustHttpsOperation {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

impl HttpsAcquisitionPort for RustHttpsAcquisitionPort {
    fn start_fetch(
        &self,
        request: HttpsFetchRequest,
        completion: HttpsAcquisitionCompletion,
    ) -> Result<Arc<dyn HttpsAcquisitionOperation>, HttpsPortError> {
        let permit = self.admission.reserve()?;
        let config = self.config;
        let runtime = self
            .runtime
            .as_ref()
            .expect("runtime is only taken by Drop, after which the port is unreachable");
        let task = runtime.spawn(async move {
            let result = rust_https_fetch(request, config).await;
            completion.resolve(result);
            drop(permit);
        });
        Ok(Arc::new(RustHttpsOperation {
            abort: task.abort_handle(),
        }))
    }
}

#[derive(Debug)]
struct HttpsAdmission {
    maximum: usize,
    active: Mutex<usize>,
}

impl HttpsAdmission {
    fn new(maximum: usize) -> Self {
        Self {
            maximum,
            active: Mutex::new(0),
        }
    }

    fn reserve(self: &Arc<Self>) -> Result<HttpsPermit, HttpsPortError> {
        let mut active = self.active.lock();
        if *active >= self.maximum {
            return Err(HttpsPortError::Saturated {
                maximum: self.maximum,
            });
        }
        *active += 1;
        Ok(HttpsPermit {
            admission: Arc::clone(self),
        })
    }
}

#[derive(Debug)]
struct HttpsPermit {
    admission: Arc<HttpsAdmission>,
}

impl Drop for HttpsPermit {
    fn drop(&mut self) {
        let mut active = self.admission.active.lock();
        *active = active.saturating_sub(1);
    }
}

async fn rust_https_fetch(
    request: HttpsFetchRequest,
    config: RustHttpsAcquisitionConfig,
) -> Result<HttpsFetchResponse, HttpsPortError> {
    tokio::time::timeout(config.deadline, rust_https_fetch_inner(request, config))
        .await
        .map_err(|_| HttpsPortError::new("HTTPS acquisition deadline elapsed"))?
}

async fn rust_https_fetch_inner(
    request: HttpsFetchRequest,
    config: RustHttpsAcquisitionConfig,
) -> Result<HttpsFetchResponse, HttpsPortError> {
    let url = validate_candidate(request.url(), config.maximum_url_bytes)
        .map_err(|reason| HttpsPortError::Refused { reason })?;
    let host = url.host_str().ok_or(HttpsPortError::Refused {
        reason: AcquisitionRefusal::InvalidCandidate,
    })?;
    let port = url.port_or_known_default().ok_or(HttpsPortError::Refused {
        reason: AcquisitionRefusal::InvalidCandidate,
    })?;
    let socket_addresses = match url.host() {
        Some(Host::Ipv4(address)) => vec![SocketAddr::new(IpAddr::V4(address), port)],
        Some(Host::Ipv6(address)) => vec![SocketAddr::new(IpAddr::V6(address), port)],
        Some(Host::Domain(domain)) => {
            let resolved = tokio::net::lookup_host((domain, port))
                .await
                .map_err(|error| HttpsPortError::new(error.to_string()))?;
            let mut unique = BTreeSet::new();
            for address in resolved {
                unique.insert(address);
                if unique.len() > config.maximum_resolved_addresses {
                    return Err(HttpsPortError::Refused {
                        reason: AcquisitionRefusal::AddressLimit {
                            actual: unique.len(),
                            maximum: config.maximum_resolved_addresses,
                        },
                    });
                }
            }
            unique.into_iter().collect()
        }
        None => {
            return Err(HttpsPortError::Refused {
                reason: AcquisitionRefusal::InvalidCandidate,
            });
        }
    };
    let resolved_addresses: Vec<IpAddr> = socket_addresses
        .iter()
        .map(|address| address.ip())
        .collect();
    validate_resolved_addresses(&resolved_addresses, config.maximum_resolved_addresses)
        .map_err(|reason| HttpsPortError::Refused { reason })?;

    let mut client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(config.deadline)
        .timeout(config.deadline);
    if matches!(url.host(), Some(Host::Domain(_))) {
        client = client.resolve_to_addrs(host, &socket_addresses);
    }
    let client = client
        .build()
        .map_err(|error| HttpsPortError::new(error.to_string()))?;
    let mut response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|error| HttpsPortError::new(error.to_string()))?;
    let effective_url: Arc<str> = Arc::from(response.url().as_str());
    let status = response.status().as_u16();
    let redirect_location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(Arc::<str>::from);
    let maximum_with_probe =
        request
            .maximum_bytes
            .checked_add(1)
            .ok_or(HttpsPortError::Refused {
                reason: AcquisitionRefusal::Oversize {
                    actual: usize::MAX,
                    maximum: request.maximum_bytes,
                },
            })?;
    let mut body = Vec::with_capacity(maximum_with_probe.min(64 * 1_024));
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| HttpsPortError::new(error.to_string()))?
    {
        let remaining = maximum_with_probe.saturating_sub(body.len());
        if remaining == 0 {
            break;
        }
        let take = remaining.min(chunk.len());
        body.extend_from_slice(&chunk[..take]);
        if body.len() == maximum_with_probe {
            break;
        }
    }
    Ok(HttpsFetchResponse::new(
        effective_url,
        status,
        redirect_location,
        resolved_addresses,
        body,
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionOrigin {
    OnlineVerified,
    OfflineSealed,
}

#[derive(Clone, Debug)]
pub struct ResolvedArtifact {
    handle: VerifiedArtifactHandle,
    origin: ResolutionOrigin,
    lookup_facts: Arc<[CoordinateLookupFact]>,
    acquisition_facts: Arc<[AcquisitionFact]>,
}

impl ResolvedArtifact {
    pub fn handle(&self) -> &VerifiedArtifactHandle {
        &self.handle
    }

    pub fn origin(&self) -> ResolutionOrigin {
        self.origin
    }

    pub fn lookup_facts(&self) -> &[CoordinateLookupFact] {
        &self.lookup_facts
    }

    pub fn acquisition_facts(&self) -> &[AcquisitionFact] {
        &self.acquisition_facts
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum SealedCacheError {
    #[error("sealed cache is closed")]
    Closed,
    #[error("sealed cache entry conflicts with an existing aggregate")]
    Conflict,
    #[error("sealed cache capacity exceeded")]
    Capacity,
    #[error("sealed cache implementation failed: {reason}")]
    Implementation { reason: Arc<str> },
}

/// Exact verified identity for an offline artifact record. Aggregate alone is
/// insufficient because two publishers may intentionally ship identical bytes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SealedArtifactKey {
    coordinate: SealedCoordinate,
    aggregate: Sha256Digest,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SealedCoordinate {
    Snapshot {
        event_id: Sha256Digest,
        author: Sha256Digest,
    },
    Root {
        author: Sha256Digest,
    },
    Named {
        author: Sha256Digest,
        d_tag: Arc<str>,
    },
}

impl SealedArtifactKey {
    pub fn for_coordinate(coordinate: &ManifestCoordinate, aggregate: Sha256Digest) -> Self {
        let coordinate = match coordinate {
            ManifestCoordinate::Snapshot { event_id, author } => SealedCoordinate::Snapshot {
                event_id: event_id.clone(),
                author: author.clone(),
            },
            ManifestCoordinate::Root { author } => SealedCoordinate::Root {
                author: author.clone(),
            },
            ManifestCoordinate::Named { author, d_tag } => SealedCoordinate::Named {
                author: author.clone(),
                d_tag: Arc::clone(d_tag),
            },
        };
        Self {
            coordinate,
            aggregate,
        }
    }

    pub fn aggregate(&self) -> &Sha256Digest {
        &self.aggregate
    }
}

/// Indexes artifact-owned sealed handles. Implementations must never write or
/// reinterpret artifact bytes.
pub trait SealedArtifactCache: Send + Sync + fmt::Debug {
    fn load(
        &self,
        key: &SealedArtifactKey,
    ) -> Result<Option<VerifiedArtifactHandle>, SealedCacheError>;

    fn retain(
        &self,
        key: &SealedArtifactKey,
        handle: &VerifiedArtifactHandle,
    ) -> Result<(), SealedCacheError>;
}

#[derive(Debug)]
pub struct MemorySealedArtifactCache {
    maximum_entries: usize,
    maximum_bytes: usize,
    state: Mutex<MemoryCacheState>,
}

#[derive(Debug, Default)]
struct MemoryCacheState {
    total_bytes: usize,
    entries: BTreeMap<SealedArtifactKey, VerifiedArtifactHandle>,
}

impl MemorySealedArtifactCache {
    pub fn new(maximum_entries: usize, maximum_bytes: usize) -> Result<Self, SealedCacheError> {
        if maximum_entries == 0 || maximum_bytes == 0 {
            return Err(SealedCacheError::Capacity);
        }
        Ok(Self {
            maximum_entries,
            maximum_bytes,
            state: Mutex::new(MemoryCacheState::default()),
        })
    }
}

impl SealedArtifactCache for MemorySealedArtifactCache {
    fn load(
        &self,
        key: &SealedArtifactKey,
    ) -> Result<Option<VerifiedArtifactHandle>, SealedCacheError> {
        Ok(self.state.lock().entries.get(key).cloned())
    }

    fn retain(
        &self,
        key: &SealedArtifactKey,
        handle: &VerifiedArtifactHandle,
    ) -> Result<(), SealedCacheError> {
        if handle.index().aggregate() != key.aggregate()
            || !sealed_coordinate_matches_index(&key.coordinate, handle.index())
        {
            return Err(SealedCacheError::Conflict);
        }
        let bytes = handle
            .index()
            .entries()
            .try_fold(0usize, |total, entry| total.checked_add(entry.bytes()))
            .ok_or(SealedCacheError::Capacity)?;
        let mut state = self.state.lock();
        if let Some(existing) = state.entries.get(key) {
            return if existing.index() == handle.index() {
                Ok(())
            } else {
                Err(SealedCacheError::Conflict)
            };
        }
        let total_bytes = state
            .total_bytes
            .checked_add(bytes)
            .ok_or(SealedCacheError::Capacity)?;
        if state.entries.len() >= self.maximum_entries || total_bytes > self.maximum_bytes {
            return Err(SealedCacheError::Capacity);
        }
        state.total_bytes = total_bytes;
        state.entries.insert(key.clone(), handle.clone());
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("resolver limits must be finite and non-zero")]
    InvalidLimits,
    #[error("resolver is saturated at {maximum} concurrent operations")]
    Saturated { maximum: usize },
    #[error("artifact review capacity is saturated at {maximum} pending reviews")]
    ReviewSaturated { maximum: usize },
    #[error("cancellation wake capacity is saturated at {maximum} listeners")]
    CancellationSaturated { maximum: usize },
    #[error("operation was cancelled")]
    Cancelled,
    #[error("manifest lookup closed without a result")]
    LookupClosed,
    #[error("manifest lookup failed: {reason}")]
    Lookup { reason: Arc<str> },
    #[error("manifest lookup returned no selected row; inspect scoped lookup facts")]
    NotFound { facts: Arc<[CoordinateLookupFact]> },
    #[error("lookup returned {actual} facts; the maximum is {maximum}")]
    LookupFactLimit { actual: usize, maximum: usize },
    #[error("lookup fact violates bounded evidence policy")]
    InvalidLookupFact,
    #[error("selected manifest is {actual} bytes; the maximum is {maximum}")]
    ManifestTooLarge { actual: usize, maximum: usize },
    #[error("artifact verification or sealing failed: {reason}")]
    Artifact { reason: Arc<str> },
    #[error("artifact acquisition was refused: {reason}")]
    Acquisition {
        reason: AcquisitionRefusal,
        facts: Arc<[AcquisitionFact]>,
    },
    #[error("sealed artifact cache failed: {0}")]
    Cache(#[from] SealedCacheError),
    #[error("offline aggregate does not match the requested coordinate")]
    OfflineCoordinateMismatch,
    #[error("no sealed artifact exists for aggregate {aggregate:?}")]
    OfflineMiss { aggregate: Sha256Digest },
    #[error("artifact review was already confirmed or cancelled")]
    ReviewStale,
    #[error("artifact review belongs to a different resolver")]
    ReviewForeign,
}

impl ResolveError {
    pub fn lookup_facts(&self) -> Option<&[CoordinateLookupFact]> {
        match self {
            Self::NotFound { facts } => Some(facts),
            _ => None,
        }
    }

    pub fn acquisition_facts(&self) -> Option<&[AcquisitionFact]> {
        match self {
            Self::Acquisition { facts, .. } => Some(facts),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct CatalogResolver {
    limits: ResolverLimits,
    artifact_limits: ArtifactLimits,
    source_policy: ArtifactSourcePolicy,
    lookup: Arc<dyn ManifestLookupPort>,
    transport: Arc<dyn HttpsAcquisitionPort>,
    artifact_cache: Arc<FileArtifactCache>,
    sealed_cache: Arc<dyn SealedArtifactCache>,
    admission: Admission,
    review_admission: Arc<ReviewAdmission>,
    identity: Arc<ResolverIdentity>,
}

#[derive(Debug)]
struct ResolverIdentity;

#[derive(Clone, Debug)]
pub struct ArtifactReviewSummary {
    coordinate: ManifestCoordinate,
    event_id: Sha256Digest,
    aggregate: Sha256Digest,
    title: Option<Arc<str>>,
    description: Option<Arc<str>>,
    requirements: Arc<[Arc<str>]>,
    servers: Arc<[Arc<str>]>,
    lookup_facts: Arc<[CoordinateLookupFact]>,
}

impl ArtifactReviewSummary {
    pub fn coordinate(&self) -> &ManifestCoordinate {
        &self.coordinate
    }

    pub fn event_id(&self) -> &Sha256Digest {
        &self.event_id
    }

    pub fn aggregate(&self) -> &Sha256Digest {
        &self.aggregate
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn requirements(&self) -> impl ExactSizeIterator<Item = &str> {
        self.requirements.iter().map(AsRef::as_ref)
    }

    pub fn servers(&self) -> impl ExactSizeIterator<Item = &str> {
        self.servers.iter().map(AsRef::as_ref)
    }

    pub fn lookup_facts(&self) -> &[CoordinateLookupFact] {
        &self.lookup_facts
    }
}

#[derive(Debug)]
struct ArtifactReviewPayload {
    signed_event_json: Arc<[u8]>,
}

/// Opaque exact-install operation. The selected signed manifest and its
/// coordinate, event id, and aggregate are frozen when this value is created.
/// Confirming never performs another catalog lookup.
#[derive(Debug)]
pub struct ArtifactReview {
    owner: Arc<ResolverIdentity>,
    summary: ArtifactReviewSummary,
    payload: Mutex<Option<ArtifactReviewPayload>>,
    lease: Mutex<Option<ReviewLease>>,
}

impl ArtifactReview {
    pub fn summary(&self) -> &ArtifactReviewSummary {
        &self.summary
    }

    /// Releases the pending review immediately. A later confirm is refused as
    /// stale even if another `Arc` still retains this token.
    pub fn cancel(&self) -> Result<(), ResolveError> {
        let removed = self.payload.lock().take().is_some();
        self.lease.lock().take();
        if removed {
            Ok(())
        } else {
            Err(ResolveError::ReviewStale)
        }
    }

    fn take_for(
        &self,
        owner: &Arc<ResolverIdentity>,
    ) -> Result<ArtifactReviewPayload, ResolveError> {
        if !Arc::ptr_eq(&self.owner, owner) {
            return Err(ResolveError::ReviewForeign);
        }
        let payload = self
            .payload
            .lock()
            .take()
            .ok_or(ResolveError::ReviewStale)?;
        self.lease.lock().take();
        Ok(payload)
    }
}

impl Drop for ArtifactReview {
    fn drop(&mut self) {
        self.payload.get_mut().take();
        self.lease.get_mut().take();
    }
}

impl CatalogResolver {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        limits: ResolverLimits,
        artifact_limits: ArtifactLimits,
        source_policy: ArtifactSourcePolicy,
        lookup: Arc<dyn ManifestLookupPort>,
        transport: Arc<dyn HttpsAcquisitionPort>,
        artifact_cache: Arc<FileArtifactCache>,
        sealed_cache: Arc<dyn SealedArtifactCache>,
    ) -> Result<Self, ResolveError> {
        let limits = limits.validate()?;
        if artifact_limits.maximum_files == 0
            || artifact_limits.maximum_file_bytes == 0
            || artifact_limits.maximum_total_bytes == 0
        {
            return Err(ResolveError::InvalidLimits);
        }
        Ok(Self {
            limits,
            artifact_limits,
            source_policy,
            lookup,
            transport,
            artifact_cache,
            sealed_cache,
            admission: Admission::new(limits.maximum_in_flight),
            review_admission: Arc::new(ReviewAdmission::new(limits.maximum_reviews)),
            identity: Arc::new(ResolverIdentity),
        })
    }

    /// Compatibility convenience for callers that do not need a visible
    /// review pause. The exact selected event is still frozen before confirm.
    pub fn resolve(
        &self,
        coordinate: &ManifestCoordinate,
        cancellation: &CancellationToken,
    ) -> Result<ResolvedArtifact, ResolveError> {
        let review = self.begin_review(coordinate, cancellation)?;
        self.confirm_review(&review, cancellation)
    }

    pub fn begin_review(
        &self,
        coordinate: &ManifestCoordinate,
        cancellation: &CancellationToken,
    ) -> Result<ArtifactReview, ResolveError> {
        let _permit = self.admission.reserve()?;
        let review_lease = self.review_admission.reserve()?;
        ensure_not_cancelled(cancellation)?;
        let request = ManifestLookupRequest {
            coordinate: coordinate.clone(),
            maximum_event_bytes: self.limits.maximum_manifest_bytes,
            maximum_facts: self.limits.maximum_lookup_facts,
        };
        let completion = ManifestLookupCompletion::pending();
        let operation = self
            .lookup
            .start_lookup(request, completion.clone())
            .map_err(|error| ResolveError::Lookup {
                reason: bounded_reason(error.reason, self.limits.maximum_reason_bytes),
            })?;
        let lookup_result = completion.wait(cancellation);
        operation.cancel();
        let lookup = lookup_result.map_err(|error| match error {
            ResolveError::Lookup { reason } => ResolveError::Lookup {
                reason: bounded_reason(reason, self.limits.maximum_reason_bytes),
            },
            other => other,
        })?;
        ensure_not_cancelled(cancellation)?;
        self.validate_lookup_facts(&lookup.facts)?;
        let event_json = lookup
            .selected_event_json
            .ok_or_else(|| ResolveError::NotFound {
                facts: Arc::clone(&lookup.facts),
            })?;
        if event_json.len() > self.limits.maximum_manifest_bytes {
            return Err(ResolveError::ManifestTooLarge {
                actual: event_json.len(),
                maximum: self.limits.maximum_manifest_bytes,
            });
        }
        let manifest = ManifestEventVerifier::pinned()
            .verify_json(&event_json, coordinate)
            .map_err(|error| ResolveError::Artifact {
                reason: Arc::from(error.to_string()),
            })?;
        self.validate_selected_fact(&lookup.facts, manifest.event_id())?;
        let summary = ArtifactReviewSummary {
            coordinate: coordinate.clone(),
            event_id: manifest.event_id().clone(),
            aggregate: manifest.aggregate().clone(),
            title: manifest.title().map(Arc::from),
            description: manifest.description().map(Arc::from),
            requirements: manifest
                .requirements()
                .map(Arc::from)
                .collect::<Vec<_>>()
                .into(),
            servers: manifest.servers().map(Arc::from).collect::<Vec<_>>().into(),
            lookup_facts: lookup.facts,
        };
        Ok(ArtifactReview {
            owner: Arc::clone(&self.identity),
            summary,
            payload: Mutex::new(Some(ArtifactReviewPayload {
                signed_event_json: event_json,
            })),
            lease: Mutex::new(Some(review_lease)),
        })
    }

    pub fn confirm_review(
        &self,
        review: &ArtifactReview,
        cancellation: &CancellationToken,
    ) -> Result<ResolvedArtifact, ResolveError> {
        let _permit = self.admission.reserve()?;
        ensure_not_cancelled(cancellation)?;
        let payload = review.take_for(&self.identity)?;
        let coordinate = review.summary.coordinate();
        let manifest = ManifestEventVerifier::pinned()
            .verify_json(&payload.signed_event_json, coordinate)
            .map_err(|error| ResolveError::Artifact {
                reason: Arc::from(error.to_string()),
            })?;
        if manifest.event_id() != review.summary.event_id()
            || manifest.aggregate() != review.summary.aggregate()
        {
            return Err(ResolveError::ReviewStale);
        }
        let source = SafeManifestBlobSource::new(
            Arc::clone(&self.transport),
            cancellation.clone(),
            self.limits,
        );
        let resolver = SignedArtifactResolver::new(
            ManifestEventVerifier::pinned(),
            self.artifact_limits,
            self.source_policy.clone(),
            &source,
            &self.artifact_cache,
        )
        .map_err(|error| ResolveError::Artifact {
            reason: Arc::from(error.to_string()),
        })?;
        let handle = match resolver.resolve_verified(manifest) {
            Ok(handle) => handle,
            Err(error) => {
                let facts = source.facts();
                if let Some(reason) = source.terminal_refusal() {
                    return Err(ResolveError::Acquisition { reason, facts });
                }
                return Err(ResolveError::Artifact {
                    reason: Arc::from(error.to_string()),
                });
            }
        };
        ensure_not_cancelled(cancellation)?;
        let key = SealedArtifactKey::for_coordinate(coordinate, handle.index().aggregate().clone());
        self.sealed_cache.retain(&key, &handle)?;
        Ok(ResolvedArtifact {
            handle,
            origin: ResolutionOrigin::OnlineVerified,
            lookup_facts: Arc::clone(&review.summary.lookup_facts),
            acquisition_facts: source.facts(),
        })
    }

    pub fn resolve_offline(
        &self,
        coordinate: &ManifestCoordinate,
        aggregate: &Sha256Digest,
        cancellation: &CancellationToken,
    ) -> Result<ResolvedArtifact, ResolveError> {
        let _permit = self.admission.reserve()?;
        ensure_not_cancelled(cancellation)?;
        let key = SealedArtifactKey::for_coordinate(coordinate, aggregate.clone());
        let handle = self
            .sealed_cache
            .load(&key)?
            .ok_or_else(|| ResolveError::OfflineMiss {
                aggregate: aggregate.clone(),
            })?;
        if !index_matches_coordinate(handle.index(), coordinate) {
            return Err(ResolveError::OfflineCoordinateMismatch);
        }
        ensure_not_cancelled(cancellation)?;
        Ok(ResolvedArtifact {
            handle,
            origin: ResolutionOrigin::OfflineSealed,
            lookup_facts: Arc::from([]),
            acquisition_facts: Arc::from([]),
        })
    }

    fn validate_lookup_facts(&self, facts: &[CoordinateLookupFact]) -> Result<(), ResolveError> {
        if facts.len() > self.limits.maximum_lookup_facts {
            return Err(ResolveError::LookupFactLimit {
                actual: facts.len(),
                maximum: self.limits.maximum_lookup_facts,
            });
        }
        for fact in facts {
            if fact.source.is_empty() || fact.source.len() > self.limits.maximum_source_label_bytes
            {
                return Err(ResolveError::InvalidLookupFact);
            }
            match &fact.state {
                CoordinateLookupState::Observed { .. } => {}
                CoordinateLookupState::Shortfall { reason } => {
                    if reason.is_empty() || reason.len() > self.limits.maximum_reason_bytes {
                        return Err(ResolveError::InvalidLookupFact);
                    }
                }
                CoordinateLookupState::Selected { event_id } => {
                    if Sha256Digest::parse(event_id.to_string()).is_err() {
                        return Err(ResolveError::InvalidLookupFact);
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_selected_fact(
        &self,
        facts: &[CoordinateLookupFact],
        selected_event_id: &Sha256Digest,
    ) -> Result<(), ResolveError> {
        let mut selected = facts.iter().filter_map(|fact| match &fact.state {
            CoordinateLookupState::Selected { event_id } => Some(event_id.as_ref()),
            CoordinateLookupState::Observed { .. } | CoordinateLookupState::Shortfall { .. } => {
                None
            }
        });
        if selected.next() != Some(selected_event_id.as_str()) || selected.next().is_some() {
            return Err(ResolveError::InvalidLookupFact);
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Admission {
    maximum: usize,
    active: Mutex<usize>,
}

impl Admission {
    fn new(maximum: usize) -> Self {
        Self {
            maximum,
            active: Mutex::new(0),
        }
    }

    fn reserve(&self) -> Result<AdmissionPermit<'_>, ResolveError> {
        let mut active = self.active.lock();
        if *active >= self.maximum {
            return Err(ResolveError::Saturated {
                maximum: self.maximum,
            });
        }
        *active += 1;
        Ok(AdmissionPermit { admission: self })
    }
}

struct AdmissionPermit<'a> {
    admission: &'a Admission,
}

impl Drop for AdmissionPermit<'_> {
    fn drop(&mut self) {
        let mut active = self.admission.active.lock();
        *active = active.saturating_sub(1);
    }
}

#[derive(Debug)]
struct ReviewAdmission {
    maximum: usize,
    active: Mutex<usize>,
}

impl ReviewAdmission {
    fn new(maximum: usize) -> Self {
        Self {
            maximum,
            active: Mutex::new(0),
        }
    }

    fn reserve(self: &Arc<Self>) -> Result<ReviewLease, ResolveError> {
        let mut active = self.active.lock();
        if *active >= self.maximum {
            return Err(ResolveError::ReviewSaturated {
                maximum: self.maximum,
            });
        }
        *active += 1;
        Ok(ReviewLease {
            admission: Arc::clone(self),
            active: true,
        })
    }
}

#[derive(Debug)]
struct ReviewLease {
    admission: Arc<ReviewAdmission>,
    active: bool,
}

impl Drop for ReviewLease {
    fn drop(&mut self) {
        if self.active {
            let mut active = self.admission.active.lock();
            *active = active.saturating_sub(1);
            self.active = false;
        }
    }
}

#[derive(Debug)]
struct SafeManifestBlobSource {
    transport: Arc<dyn HttpsAcquisitionPort>,
    cancellation: CancellationToken,
    limits: ResolverLimits,
    state: Mutex<AcquisitionState>,
}

#[derive(Debug, Default)]
struct AcquisitionState {
    facts: Vec<AcquisitionFact>,
    terminal_refusal: Option<AcquisitionRefusal>,
}

impl SafeManifestBlobSource {
    fn new(
        transport: Arc<dyn HttpsAcquisitionPort>,
        cancellation: CancellationToken,
        limits: ResolverLimits,
    ) -> Self {
        Self {
            transport,
            cancellation,
            limits,
            state: Mutex::new(AcquisitionState::default()),
        }
    }

    fn facts(&self) -> Arc<[AcquisitionFact]> {
        self.state.lock().facts.clone().into()
    }

    fn terminal_refusal(&self) -> Option<AcquisitionRefusal> {
        self.state.lock().terminal_refusal.clone()
    }

    fn refuse(
        &self,
        logical_path: &str,
        source_url: &str,
        reason: AcquisitionRefusal,
    ) -> BlobSourceError {
        let fact = AcquisitionFact {
            logical_path: Arc::from(logical_path),
            source_url: Arc::from(source_url),
            outcome: AcquisitionOutcome::Refused {
                reason: reason.clone(),
            },
        };
        let mut state = self.state.lock();
        if state.facts.len() < self.limits.maximum_acquisition_facts {
            state.facts.push(fact);
            state.terminal_refusal = Some(reason.clone());
        } else {
            state.terminal_refusal = Some(AcquisitionRefusal::EvidenceCapacity {
                maximum: self.limits.maximum_acquisition_facts,
            });
        }
        BlobSourceError {
            reason: state
                .terminal_refusal
                .as_ref()
                .expect("terminal refusal was just assigned")
                .to_string(),
        }
    }

    fn record(
        &self,
        logical_path: &str,
        source_url: &str,
        outcome: AcquisitionOutcome,
    ) -> Result<(), BlobSourceError> {
        let mut state = self.state.lock();
        if state.facts.len() >= self.limits.maximum_acquisition_facts {
            let reason = AcquisitionRefusal::EvidenceCapacity {
                maximum: self.limits.maximum_acquisition_facts,
            };
            state.terminal_refusal = Some(reason.clone());
            return Err(BlobSourceError {
                reason: reason.to_string(),
            });
        }
        state.facts.push(AcquisitionFact {
            logical_path: Arc::from(logical_path),
            source_url: Arc::from(source_url),
            outcome,
        });
        Ok(())
    }
}

impl ManifestBlobSource for SafeManifestBlobSource {
    /// Every candidate, and every redirect hop it leads to, is refetched
    /// through the same HTTPS-only / credential-free / public-address /
    /// effective-URL policy. A redirect never substitutes a location that
    /// bypasses that policy; it only ever advances to another location this
    /// policy has independently approved. Hops are capped by
    /// `maximum_redirect_hops` so a redirect chain cannot loop or stall
    /// acquisition indefinitely. Content is still sealed only after its
    /// bytes hash-match the manifest-pinned digest, independent of origin.
    fn fetch(&self, request: &BlobFetchRequest) -> Result<BlobFetchResponse, BlobSourceError> {
        'candidates: for candidate in request.candidate_urls() {
            if self.cancellation.is_cancelled() {
                return Err(self.refuse(
                    request.logical_path(),
                    candidate,
                    AcquisitionRefusal::Cancelled,
                ));
            }
            let mut current_url = match validate_candidate(candidate, self.limits.maximum_url_bytes)
            {
                Ok(url) => url,
                Err(reason) => {
                    return Err(self.refuse(request.logical_path(), candidate, reason));
                }
            };
            let mut hops = 0usize;
            loop {
                let current = current_url.as_str().to_owned();
                let raw_request = HttpsFetchRequest {
                    url: Arc::from(current.as_str()),
                    maximum_bytes: request.maximum_bytes(),
                };
                let completion = HttpsAcquisitionCompletion::pending();
                let operation = match self.transport.start_fetch(raw_request, completion.clone()) {
                    Ok(operation) => operation,
                    Err(HttpsPortError::Refused { reason }) => {
                        return Err(self.refuse(request.logical_path(), &current, reason));
                    }
                    Err(HttpsPortError::Saturated { maximum }) => {
                        return Err(self.refuse(
                            request.logical_path(),
                            &current,
                            AcquisitionRefusal::ExecutorSaturated { maximum },
                        ));
                    }
                    Err(HttpsPortError::Transport { reason }) => {
                        let reason = bounded_reason(reason, self.limits.maximum_reason_bytes);
                        self.record(
                            request.logical_path(),
                            &current,
                            AcquisitionOutcome::TransportFailed { reason },
                        )?;
                        continue 'candidates;
                    }
                };
                let response_result = completion.wait(&self.cancellation);
                operation.cancel();
                let response = match response_result {
                    Ok(response) => response,
                    Err(HttpsWaitError::Cancelled) => {
                        return Err(self.refuse(
                            request.logical_path(),
                            &current,
                            AcquisitionRefusal::Cancelled,
                        ));
                    }
                    Err(HttpsWaitError::Port(HttpsPortError::Refused { reason })) => {
                        return Err(self.refuse(request.logical_path(), &current, reason));
                    }
                    Err(HttpsWaitError::Port(HttpsPortError::Saturated { maximum })) => {
                        return Err(self.refuse(
                            request.logical_path(),
                            &current,
                            AcquisitionRefusal::ExecutorSaturated { maximum },
                        ));
                    }
                    Err(HttpsWaitError::CancellationSaturated { maximum }) => {
                        return Err(self.refuse(
                            request.logical_path(),
                            &current,
                            AcquisitionRefusal::CancellationCapacity { maximum },
                        ));
                    }
                    Err(HttpsWaitError::Port(HttpsPortError::Transport { reason })) => {
                        let reason = bounded_reason(reason, self.limits.maximum_reason_bytes);
                        self.record(
                            request.logical_path(),
                            &current,
                            AcquisitionOutcome::TransportFailed { reason },
                        )?;
                        continue 'candidates;
                    }
                    Err(HttpsWaitError::Closed) => {
                        self.record(
                            request.logical_path(),
                            &current,
                            AcquisitionOutcome::TransportFailed {
                                reason: Arc::from("HTTPS operation closed without a result"),
                            },
                        )?;
                        continue 'candidates;
                    }
                };
                if self.cancellation.is_cancelled() {
                    return Err(self.refuse(
                        request.logical_path(),
                        &current,
                        AcquisitionRefusal::Cancelled,
                    ));
                }
                if let Err(reason) = validate_resolved_addresses(
                    &response.resolved_addresses,
                    self.limits.maximum_resolved_addresses,
                ) {
                    return Err(self.refuse(request.logical_path(), &current, reason));
                }
                match classify_response(
                    &current_url,
                    &response.effective_url,
                    response.status,
                    response.redirect_location.as_deref(),
                    self.limits.maximum_url_bytes,
                ) {
                    Ok(ResponseAction::Follow(next_url)) => {
                        if hops >= self.limits.maximum_redirect_hops {
                            return Err(self.refuse(
                                request.logical_path(),
                                &current,
                                AcquisitionRefusal::TooManyRedirects {
                                    maximum: self.limits.maximum_redirect_hops,
                                },
                            ));
                        }
                        current_url = next_url;
                        hops += 1;
                        continue;
                    }
                    Ok(ResponseAction::HandleStatus) => {}
                    Err(reason) => {
                        return Err(self.refuse(request.logical_path(), &current, reason));
                    }
                }
                if response.body.len() > request.maximum_bytes() {
                    return Err(self.refuse(
                        request.logical_path(),
                        &current,
                        AcquisitionRefusal::Oversize {
                            actual: response.body.len(),
                            maximum: request.maximum_bytes(),
                        },
                    ));
                }
                if response.status != 200 {
                    self.record(
                        request.logical_path(),
                        &current,
                        AcquisitionOutcome::HttpStatus {
                            status: response.status,
                        },
                    )?;
                    continue 'candidates;
                }
                self.record(
                    request.logical_path(),
                    &current,
                    AcquisitionOutcome::Succeeded {
                        bytes: response.body.len(),
                    },
                )?;
                return Ok(BlobFetchResponse::ok(
                    current,
                    Box::new(Cursor::new(response.body)),
                ));
            }
        }
        Err(self.refuse(
            request.logical_path(),
            "",
            AcquisitionRefusal::AllSourcesFailed,
        ))
    }
}

fn validate_candidate(
    candidate: &str,
    maximum_url_bytes: usize,
) -> Result<Url, AcquisitionRefusal> {
    if candidate.len() > maximum_url_bytes {
        return Err(AcquisitionRefusal::InvalidCandidate);
    }
    let url = Url::parse(candidate).map_err(|_| AcquisitionRefusal::InvalidCandidate)?;
    if url.scheme() != "https"
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AcquisitionRefusal::NonHttps);
    }
    match url.host() {
        Some(Host::Ipv4(address)) if !is_public_ip(IpAddr::V4(address)) => {
            Err(AcquisitionRefusal::NonPublicAddress {
                address: IpAddr::V4(address),
            })
        }
        Some(Host::Ipv6(address)) if !is_public_ip(IpAddr::V6(address)) => {
            Err(AcquisitionRefusal::NonPublicAddress {
                address: IpAddr::V6(address),
            })
        }
        Some(Host::Domain(domain))
            if domain.eq_ignore_ascii_case("localhost")
                || domain.to_ascii_lowercase().ends_with(".localhost") =>
        {
            Err(AcquisitionRefusal::NonHttps)
        }
        Some(_) => Ok(url),
        None => Err(AcquisitionRefusal::InvalidCandidate),
    }
}

fn validate_resolved_addresses(
    addresses: &[IpAddr],
    maximum: usize,
) -> Result<(), AcquisitionRefusal> {
    if addresses.is_empty() {
        return Err(AcquisitionRefusal::MissingAddressEvidence);
    }
    if addresses.len() > maximum {
        return Err(AcquisitionRefusal::AddressLimit {
            actual: addresses.len(),
            maximum,
        });
    }
    for address in addresses.iter().copied() {
        if !is_public_ip(address) {
            return Err(AcquisitionRefusal::NonPublicAddress { address });
        }
    }
    Ok(())
}

fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => is_public_ipv6(address),
    }
}

fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || address == Ipv4Addr::BROADCAST)
}

fn is_public_ipv6(address: Ipv6Addr) -> bool {
    if address.is_unspecified() || address.is_loopback() || address.is_multicast() {
        return false;
    }
    let segments = address.segments();
    if segments[0] & 0xfe00 == 0xfc00
        || segments[0] & 0xffc0 == 0xfe80
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
    {
        return false;
    }
    if let Some(mapped) = address.to_ipv4_mapped() {
        return is_public_ipv4(mapped);
    }
    true
}

fn bounded_reason(reason: Arc<str>, maximum: usize) -> Arc<str> {
    if reason.len() <= maximum {
        return reason;
    }
    let mut end = maximum;
    while !reason.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    Arc::from(&reason[..end])
}

fn ensure_not_cancelled(cancellation: &CancellationToken) -> Result<(), ResolveError> {
    if cancellation.is_cancelled() {
        Err(ResolveError::Cancelled)
    } else {
        Ok(())
    }
}

fn index_matches_coordinate(
    index: &VerifiedArtifactIndex,
    coordinate: &ManifestCoordinate,
) -> bool {
    match coordinate {
        ManifestCoordinate::Snapshot { event_id, author } => {
            index.kind() == KIND_SNAPSHOT
                && index.event_id() == event_id
                && index.author() == author
                && index.d_tag().is_none()
        }
        ManifestCoordinate::Root { author } => {
            index.kind() == KIND_ROOT && index.author() == author && index.d_tag().is_none()
        }
        ManifestCoordinate::Named { author, d_tag } => {
            index.kind() == KIND_NAMED
                && index.author() == author
                && index.d_tag() == Some(d_tag.as_ref())
        }
    }
}

fn sealed_coordinate_matches_index(
    coordinate: &SealedCoordinate,
    index: &VerifiedArtifactIndex,
) -> bool {
    match coordinate {
        SealedCoordinate::Snapshot { event_id, author } => {
            index.kind() == KIND_SNAPSHOT
                && index.event_id() == event_id
                && index.author() == author
                && index.d_tag().is_none()
        }
        SealedCoordinate::Root { author } => {
            index.kind() == KIND_ROOT && index.author() == author && index.d_tag().is_none()
        }
        SealedCoordinate::Named { author, d_tag } => {
            index.kind() == KIND_NAMED
                && index.author() == author
                && index.d_tag() == Some(d_tag.as_ref())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        thread,
        time::Duration,
    };

    use nmp_native_artifact::{ArtifactSourcePolicy, INDEX_PATH};
    use tempfile::TempDir;

    use super::*;

    const EVENT: &[u8] =
        include_bytes!("../../../conformance/napplet-corpus/published/good-morning/event.json");
    const INDEX: &[u8] =
        include_bytes!("../../../conformance/napplet-corpus/published/good-morning/index.html");
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
                FileArtifactCache::open(self.temp.path().join("artifacts"))
                    .expect("artifact cache"),
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
                FileArtifactCache::open(fixture.temp.path().join("artifacts"))
                    .expect("artifact cache"),
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
    struct BlockingLookupState {
        started: Mutex<bool>,
        started_ready: Condvar,
        cancelled: Mutex<bool>,
        cancelled_ready: Condvar,
    }

    impl BlockingLookupState {
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
    struct BlockingLookup {
        state: Arc<BlockingLookupState>,
    }

    #[derive(Debug)]
    struct BlockingLookupOperation {
        state: Arc<BlockingLookupState>,
    }

    impl ManifestLookupOperation for BlockingLookupOperation {
        fn cancel(&self) {
            *self.state.cancelled.lock() = true;
            self.state.cancelled_ready.notify_all();
        }
    }

    impl ManifestLookupPort for BlockingLookup {
        fn start_lookup(
            &self,
            _request: ManifestLookupRequest,
            _completion: ManifestLookupCompletion,
        ) -> Result<Arc<dyn ManifestLookupOperation>, LookupPortError> {
            *self.state.started.lock() = true;
            self.state.started_ready.notify_all();
            Ok(Arc::new(BlockingLookupOperation {
                state: Arc::clone(&self.state),
            }))
        }
    }

    #[test]
    fn cancellation_wakes_a_blocked_lookup_and_cancels_its_nmp_operation() {
        let fixture = Fixture::new(TransportMode::Good);
        let state = Arc::new(BlockingLookupState::default());
        let resolver = Arc::new(
            CatalogResolver::new(
                ResolverLimits::default(),
                ArtifactLimits::default(),
                ArtifactSourcePolicy::manifest_https_only(8).expect("source policy"),
                Arc::new(BlockingLookup {
                    state: Arc::clone(&state),
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
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let result = thread_resolver.begin_review(&Fixture::coordinate(), &thread_token);
            result_tx.send(result.map(|_| ())).expect("send result");
        });

        state.wait_started();
        cancellation.cancel();
        let result = result_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("cancellation must wake the resolver");
        assert!(matches!(result, Err(ResolveError::Cancelled)));
        state.wait_cancelled();
        worker.join().expect("worker");
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

    #[test]
    fn rust_https_port_refuses_literal_private_target_before_connect() {
        let port = RustHttpsAcquisitionPort::new(RustHttpsAcquisitionConfig::default())
            .expect("Rust HTTPS port");
        let completion = HttpsAcquisitionCompletion::pending();
        let operation = port
            .start_fetch(
                HttpsFetchRequest {
                    url: Arc::from("https://127.0.0.1/artifact"),
                    maximum_bytes: 1_024,
                },
                completion.clone(),
            )
            .expect("start");
        let result = completion.wait(&CancellationToken::default());
        operation.cancel();
        assert!(matches!(
            result,
            Err(HttpsWaitError::Port(HttpsPortError::Refused {
                reason: AcquisitionRefusal::NonPublicAddress { .. }
            }))
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
}
