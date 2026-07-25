//! Bounded Rust-owned catalog browse, review, and verified-artifact handoff.
//!
//! NMP remains the canonical manifest-event owner. This module keeps only
//! screen-shaped browse projections and at most sixteen opaque, exact reviews.
//! Confirming a review returns immutable verified bytes; it never installs,
//! grants, or launches a napplet.

use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use nmp::WindowLoad;
use nmp_native_artifact::{
    ArtifactLimits, ArtifactSourcePolicy, FileArtifactCache, ManifestCoordinate,
    VerifiedArtifactHandle,
};
use nmp_native_catalog_resolver::{
    ArtifactReview, CancellationToken, CatalogResolver, CoordinateLookupFact,
    CoordinateLookupState, MemorySealedArtifactCache, ResolveError, ResolverLimits,
    RustHttpsAcquisitionConfig, RustHttpsAcquisitionPort,
};
use nmp_native_nmp_adapter::{
    NmpDataPlane,
    catalog::{
        CatalogAccessContext, CatalogBrowseCancel, CatalogBrowseFrame, CatalogBrowseRequest,
        CatalogManifestCandidate, CatalogShortfall, CatalogSourceEvidence, CatalogSourceStatus,
        ManifestCatalogError, NmpManifestCatalog,
    },
};
use parking_lot::Mutex;
use thiserror::Error;
use tokio::sync::watch;

use super::{
    CapabilityRequirement, GOOD_MORNING_AGGREGATE_HASH, GOOD_MORNING_AUTHOR,
    GOOD_MORNING_CAPABILITY_PROFILE, GOOD_MORNING_D_TAG, RuntimePermissionRequirement,
    VerifiedArtifact,
};

const MAXIMUM_PAGE_ENTRIES: usize = 100;
const MAXIMUM_PENDING_REVIEWS: usize = 16;
const MAXIMUM_ONE_SHOT_OPERATIONS: usize = 4;
const OPERATION_DEADLINE: Duration = Duration::from_secs(15);

/// One candidate from the current bounded NMP window.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogEntry {
    pub event_id: String,
    pub coordinate: Option<String>,
    pub manifest_author: String,
    pub kind: u16,
    pub created_at: u64,
    pub d_tag: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub aggregate_hash: Option<String>,
    pub observed_sources: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeCatalogSourceAccess {
    Public,
    Nip42 { public_key: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeCatalogSourceState {
    Requesting,
    Connecting,
    Disconnected,
    AwaitingAuth,
    AuthDenied,
    Error,
}

/// Source-scoped evidence. It never implies global completeness.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogSource {
    pub relay: String,
    pub access: RuntimeCatalogSourceAccess,
    pub reconciled_through: Option<u64>,
    pub state: RuntimeCatalogSourceState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeCatalogShortfall {
    NoPlannedSource,
    NoResolvedDemand,
    LocalLimit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeCatalogWindowState {
    Idle,
    Requesting,
    Returned { added: u64 },
    AtBound { maximum: u64 },
    Unknown,
}

/// A finite page for one screen.
///
/// `has_more` means matching rows were omitted by the 100-row screen
/// projection. It does not claim that NMP, a relay, or the network is complete
/// when false.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogPage {
    pub entries: Vec<RuntimeCatalogEntry>,
    pub query_was_local_filter: bool,
    pub locally_filtered_rows: u64,
    pub projection_limited_rows: u64,
    pub refused_rows: u64,
    pub has_more: bool,
    pub window: RuntimeCatalogWindowState,
    pub sources: Vec<RuntimeCatalogSource>,
    pub shortfalls: Vec<RuntimeCatalogShortfall>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeCatalogLookupState {
    Observed { rows: u64 },
    Shortfall { reason: String },
    Selected { event_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogProvenance {
    pub source: String,
    pub state: RuntimeCatalogLookupState,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogCapability {
    pub domain: String,
    pub requirement: RuntimePermissionRequirement,
}

/// An opaque exact review frozen from one verified signed manifest event.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogReview {
    pub token: String,
    pub event_id: String,
    pub coordinate: String,
    pub manifest_author: String,
    pub d_tag: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub aggregate_hash: String,
    pub capabilities: Vec<RuntimeCatalogCapability>,
    pub blob_sources: Vec<String>,
    pub provenance: Vec<RuntimeCatalogProvenance>,
}

/// Screen record paired with the verified handle returned by confirmation.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogConfirmation {
    pub event_id: String,
    pub coordinate: String,
    pub manifest_author: String,
    pub d_tag: Option<String>,
    pub title: Option<String>,
    pub aggregate_hash: String,
    pub capabilities: Vec<RuntimeCatalogCapability>,
    pub provenance: Vec<RuntimeCatalogProvenance>,
}

#[derive(Clone, Debug)]
pub struct RuntimeCatalogConfirmedArtifact {
    handle: VerifiedArtifactHandle,
    pub confirmation: RuntimeCatalogConfirmation,
}

impl RuntimeCatalogConfirmedArtifact {
    pub fn into_handle(self) -> VerifiedArtifactHandle {
        self.handle
    }
}

/// Typed, state-shaped refusal for every catalog boundary operation.
///
/// The controller returns these inside records instead of throwing across the
/// FFI boundary, keeping refusal and cancellation observable native state.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogFailure {
    pub code: String,
    pub detail: String,
    pub provenance: Vec<RuntimeCatalogProvenance>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeCatalogPageResult {
    pub page: Option<RuntimeCatalogPage>,
    pub failure: Option<RuntimeCatalogFailure>,
}

/// Latest replacement from the profile's single permanent NMP catalog feed.
#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeCatalogFeedSnapshot {
    pub revision: u64,
    pub result: RuntimeCatalogPageResult,
    pub closed: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeCatalogReviewResult {
    pub review: Option<RuntimeCatalogReview>,
    pub failure: Option<RuntimeCatalogFailure>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeCatalogConfirmationResult {
    pub confirmation: Option<RuntimeCatalogConfirmation>,
    pub artifact: Option<Arc<VerifiedArtifact>>,
    pub failure: Option<RuntimeCatalogFailure>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeCatalogCancellationResult {
    pub cancelled: bool,
    pub failure: Option<RuntimeCatalogFailure>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum RuntimeCatalogError {
    #[error("catalog configuration is invalid: {reason}")]
    InvalidConfiguration { reason: String },
    #[error("catalog operation capacity is full at {maximum}")]
    Busy { maximum: u64 },
    #[error("catalog operation exceeded its {milliseconds}ms deadline")]
    Deadline { milliseconds: u64 },
    #[error("catalog worker could not start or ended unexpectedly: {reason}")]
    WorkerUnavailable { reason: String },
    #[error("catalog query was refused: {reason}")]
    Browse { reason: String },
    #[error("manifest coordinate is invalid: {reason}")]
    InvalidCoordinate { reason: String },
    #[error("no manifest was selected from the scoped sources")]
    NotFound {
        provenance: Vec<RuntimeCatalogProvenance>,
    },
    #[error("catalog review capacity is full at {maximum}")]
    ReviewCapacity { maximum: u64 },
    #[error("catalog review token is stale")]
    StaleReview,
    #[error("catalog operation was cancelled")]
    Cancelled,
    #[error("catalog resolution failed: {reason}")]
    Resolve { reason: String },
}

#[derive(Debug)]
struct StoredReview {
    review: Arc<ArtifactReview>,
    projection: RuntimeCatalogReview,
}

#[derive(Debug)]
struct ReviewState {
    next_token: u64,
    reviews: BTreeMap<String, StoredReview>,
}

#[derive(Debug)]
struct CatalogFeedState {
    revision: u64,
    frame: Option<CatalogBrowseFrame>,
    candidates: BTreeMap<String, ManifestCoordinate>,
    failure: Option<RuntimeCatalogError>,
    closed: bool,
}

/// Profile-owned catalog service.
///
/// One bounded NMP browse observation stays open for the lifetime of the
/// profile and replaces `feed_state` as frames arrive. Exact review and
/// artifact acquisition remain separately bounded one-shot operations.
pub struct RuntimeCatalogService {
    resolver: Arc<CatalogResolver>,
    feed_state: Arc<Mutex<CatalogFeedState>>,
    feed_signal: watch::Sender<u64>,
    feed_control: Arc<BrowseOperationControl>,
    feed_worker: Mutex<Option<JoinHandle<()>>>,
    reviews: Mutex<ReviewState>,
    admission: Arc<OneShotAdmission>,
    active_operations: Mutex<BTreeMap<u64, ActiveCancellation>>,
    next_operation: AtomicU64,
    deadline: Duration,
}

impl fmt::Debug for RuntimeCatalogService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeCatalogService")
            .field("pending_reviews", &self.reviews.lock().reviews.len())
            .field("feed_revision", &self.feed_state.lock().revision)
            .field(
                "active_operations",
                &self.admission.active.load(Ordering::Acquire),
            )
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

impl RuntimeCatalogService {
    pub fn new(
        data_plane: Arc<NmpDataPlane>,
        artifact_cache: Arc<FileArtifactCache>,
        artifact_limits: ArtifactLimits,
        maximum_manifest_bytes: usize,
        maximum_blob_sources: usize,
    ) -> Result<Self, RuntimeCatalogError> {
        if maximum_manifest_bytes == 0 || maximum_blob_sources == 0 {
            return Err(RuntimeCatalogError::InvalidConfiguration {
                reason: "manifest and source limits must be non-zero".to_owned(),
            });
        }
        let catalog = data_plane.manifest_catalog();
        let limits = ResolverLimits {
            maximum_manifest_bytes,
            maximum_reviews: MAXIMUM_PENDING_REVIEWS,
            maximum_in_flight: MAXIMUM_ONE_SHOT_OPERATIONS,
            ..ResolverLimits::default()
        };
        let source_policy = ArtifactSourcePolicy::manifest_https_only(maximum_blob_sources)
            .map_err(|error| RuntimeCatalogError::InvalidConfiguration {
                reason: error.to_string(),
            })?;
        let sealed_maximum_bytes = artifact_limits
            .maximum_total_bytes
            .checked_mul(MAXIMUM_PENDING_REVIEWS)
            .ok_or_else(|| RuntimeCatalogError::InvalidConfiguration {
                reason: "sealed artifact cache byte limit overflowed".to_owned(),
            })?;
        let sealed_cache = Arc::new(
            MemorySealedArtifactCache::new(MAXIMUM_PENDING_REVIEWS, sealed_maximum_bytes).map_err(
                |error| RuntimeCatalogError::InvalidConfiguration {
                    reason: error.to_string(),
                },
            )?,
        );
        let transport = Arc::new(
            RustHttpsAcquisitionPort::new(RustHttpsAcquisitionConfig::default()).map_err(
                |error| RuntimeCatalogError::InvalidConfiguration {
                    reason: error.to_string(),
                },
            )?,
        );
        let resolver = CatalogResolver::new(
            limits,
            artifact_limits,
            source_policy,
            Arc::new(catalog.clone()),
            transport,
            artifact_cache,
            sealed_cache,
        )
        .map_err(|error| RuntimeCatalogError::InvalidConfiguration {
            reason: error.to_string(),
        })?;
        let feed_state = Arc::new(Mutex::new(CatalogFeedState {
            revision: 0,
            frame: None,
            candidates: BTreeMap::new(),
            failure: None,
            closed: false,
        }));
        let (feed_signal, _) = watch::channel(0_u64);
        let feed_control = Arc::new(BrowseOperationControl::default());
        let worker = spawn_catalog_feed(
            catalog,
            Arc::clone(&feed_state),
            feed_signal.clone(),
            Arc::clone(&feed_control),
        )?;
        Ok(Self {
            resolver: Arc::new(resolver),
            feed_state,
            feed_signal,
            feed_control,
            feed_worker: Mutex::new(Some(worker)),
            reviews: Mutex::new(ReviewState {
                next_token: 0,
                reviews: BTreeMap::new(),
            }),
            admission: Arc::new(OneShotAdmission::new(MAXIMUM_ONE_SHOT_OPERATIONS)),
            active_operations: Mutex::new(BTreeMap::new()),
            next_operation: AtomicU64::new(0),
            deadline: OPERATION_DEADLINE,
        })
    }

    pub fn browse(&self, query: Option<&str>) -> Result<RuntimeCatalogPage, RuntimeCatalogError> {
        let query_was_local_filter = query.is_some_and(|value| !value.trim().is_empty());
        let request = CatalogBrowseRequest::new(query).map_err(map_browse_error)?;
        let state = self.feed_state.lock();
        browse_feed_state(&state, &request, query_was_local_filter)
    }

    pub fn feed_snapshot(&self, query: Option<&str>) -> RuntimeCatalogFeedSnapshot {
        let query_was_local_filter = query.is_some_and(|value| !value.trim().is_empty());
        let request = match CatalogBrowseRequest::new(query) {
            Ok(request) => request,
            Err(error) => {
                return RuntimeCatalogFeedSnapshot {
                    revision: self.feed_state.lock().revision,
                    result: RuntimeCatalogPageResult {
                        page: None,
                        failure: Some(project_catalog_error(map_browse_error(error))),
                    },
                    closed: false,
                };
            }
        };
        let state = self.feed_state.lock();
        let revision = state.revision;
        let closed = state.closed;
        let result = match browse_feed_state(&state, &request, query_was_local_filter) {
            Ok(page) => RuntimeCatalogPageResult {
                page: Some(page),
                failure: None,
            },
            Err(error) => RuntimeCatalogPageResult {
                page: None,
                failure: Some(project_catalog_error(error)),
            },
        };
        RuntimeCatalogFeedSnapshot {
            revision,
            result,
            closed,
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.feed_signal.subscribe()
    }

    pub fn close(&self) {
        let should_close = {
            let mut state = self.feed_state.lock();
            if state.closed {
                false
            } else {
                state.closed = true;
                advance_catalog_revision(&mut state);
                true
            }
        };
        if should_close {
            self.feed_control.cancel();
            bump_catalog_signal(&self.feed_signal);
        }
        if let Some(worker) = self.feed_worker.lock().take() {
            let _ = worker.join();
        }
    }

    pub fn begin_review_for_entry(
        &self,
        event_id: &str,
    ) -> Result<RuntimeCatalogReview, RuntimeCatalogError> {
        let coordinate = self
            .feed_state
            .lock()
            .candidates
            .get(event_id)
            .cloned()
            .ok_or_else(|| RuntimeCatalogError::InvalidCoordinate {
                reason: "catalog entry is stale or outside the current page".to_owned(),
            })?;
        self.begin_review(coordinate)
    }

    pub fn begin_review(
        &self,
        coordinate: ManifestCoordinate,
    ) -> Result<RuntimeCatalogReview, RuntimeCatalogError> {
        {
            let state = self.reviews.lock();
            if state.reviews.len() >= MAXIMUM_PENDING_REVIEWS {
                return Err(RuntimeCatalogError::ReviewCapacity {
                    maximum: MAXIMUM_PENDING_REVIEWS as u64,
                });
            }
        }
        let permit = self.admission.reserve()?;
        let resolver = Arc::clone(&self.resolver);
        let cancellation = CancellationToken::default();
        let operation_id =
            self.register_operation(ActiveCancellation::Resolve(cancellation.clone()))?;
        let worker_cancellation = cancellation.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("runtime-catalog-review".to_owned())
            .spawn(move || {
                let _permit = permit;
                let result = resolver
                    .begin_review(&coordinate, &worker_cancellation)
                    .map(Arc::new);
                let _ = sender.send(result);
            })
            .map_err(|error| {
                self.remove_operation(operation_id);
                RuntimeCatalogError::WorkerUnavailable {
                    reason: error.to_string(),
                }
            })?;
        let result = match receiver.recv_timeout(self.deadline) {
            Ok(result) => result.map_err(map_resolve_error),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                cancellation.cancel();
                Err(RuntimeCatalogError::Deadline {
                    milliseconds: duration_millis(self.deadline),
                })
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(RuntimeCatalogError::WorkerUnavailable {
                    reason: "catalog review worker ended without a result".to_owned(),
                })
            }
        };
        self.remove_operation(operation_id);
        let _ = worker.join();
        let review = result?;
        let mut state = self.reviews.lock();
        if state.reviews.len() >= MAXIMUM_PENDING_REVIEWS {
            let _ = review.cancel();
            return Err(RuntimeCatalogError::ReviewCapacity {
                maximum: MAXIMUM_PENDING_REVIEWS as u64,
            });
        }
        state.next_token =
            state
                .next_token
                .checked_add(1)
                .ok_or_else(|| RuntimeCatalogError::Resolve {
                    reason: "catalog review token space is exhausted".to_owned(),
                })?;
        let token = format!("catalog-review-{}", state.next_token);
        let projection = project_review(&token, review.as_ref());
        state.reviews.insert(
            token,
            StoredReview {
                review,
                projection: projection.clone(),
            },
        );
        Ok(projection)
    }

    pub fn cancel_review(&self, token: &str) -> Result<(), RuntimeCatalogError> {
        let stored = self
            .reviews
            .lock()
            .reviews
            .remove(token)
            .ok_or(RuntimeCatalogError::StaleReview)?;
        stored
            .review
            .cancel()
            .map_err(|_| RuntimeCatalogError::StaleReview)
    }

    pub fn confirm_review(
        &self,
        token: &str,
    ) -> Result<RuntimeCatalogConfirmedArtifact, RuntimeCatalogError> {
        let permit = self.admission.reserve()?;
        let stored = self
            .reviews
            .lock()
            .reviews
            .remove(token)
            .ok_or(RuntimeCatalogError::StaleReview)?;
        let resolver = Arc::clone(&self.resolver);
        let review = Arc::clone(&stored.review);
        let cancellation = CancellationToken::default();
        let operation_id =
            self.register_operation(ActiveCancellation::Resolve(cancellation.clone()))?;
        let worker_cancellation = cancellation.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("runtime-catalog-confirm".to_owned())
            .spawn(move || {
                let _permit = permit;
                let result = resolver.confirm_review(&review, &worker_cancellation);
                let _ = sender.send(result);
            })
            .map_err(|error| {
                self.remove_operation(operation_id);
                RuntimeCatalogError::WorkerUnavailable {
                    reason: error.to_string(),
                }
            })?;
        let result = match receiver.recv_timeout(self.deadline) {
            Ok(result) => result.map_err(map_resolve_error),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                cancellation.cancel();
                let _ = receiver.recv();
                Err(RuntimeCatalogError::Deadline {
                    milliseconds: duration_millis(self.deadline),
                })
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(RuntimeCatalogError::WorkerUnavailable {
                    reason: "catalog confirmation worker ended without a result".to_owned(),
                })
            }
        };
        self.remove_operation(operation_id);
        // A timed-out resolver is cancelled cooperatively, but the public
        // operation must remain bounded even if a transport does not wake
        // immediately. Dropping the handle detaches that already-cancelled
        // worker; successful and terminal workers are joined normally.
        if !matches!(&result, Err(RuntimeCatalogError::Deadline { .. })) {
            let _ = worker.join();
        }
        let resolved = result?;
        Ok(RuntimeCatalogConfirmedArtifact {
            confirmation: RuntimeCatalogConfirmation {
                event_id: stored.projection.event_id,
                coordinate: stored.projection.coordinate,
                manifest_author: stored.projection.manifest_author,
                d_tag: stored.projection.d_tag,
                title: stored.projection.title,
                aggregate_hash: stored.projection.aggregate_hash,
                capabilities: stored.projection.capabilities,
                provenance: stored.projection.provenance,
            },
            handle: resolved.handle().clone(),
        })
    }

    pub fn cancel_pending(&self) {
        for operation in self.active_operations.lock().values() {
            operation.cancel();
        }
    }

    fn register_operation(
        &self,
        cancellation: ActiveCancellation,
    ) -> Result<u64, RuntimeCatalogError> {
        let id = self
            .next_operation
            .fetch_add(1, Ordering::AcqRel)
            .checked_add(1)
            .ok_or_else(|| RuntimeCatalogError::WorkerUnavailable {
                reason: "catalog operation identifier space is exhausted".to_owned(),
            })?;
        self.active_operations.lock().insert(id, cancellation);
        Ok(id)
    }

    fn remove_operation(&self, id: u64) {
        self.active_operations.lock().remove(&id);
    }
}

impl Drop for RuntimeCatalogService {
    fn drop(&mut self) {
        self.close();
        if let Some(worker) = self.feed_worker.get_mut().take() {
            let _ = worker.join();
        }
    }
}

#[derive(Debug)]
enum ActiveCancellation {
    Resolve(CancellationToken),
}

impl ActiveCancellation {
    fn cancel(&self) {
        match self {
            Self::Resolve(cancellation) => cancellation.cancel(),
        }
    }
}

#[derive(Debug, Default)]
struct BrowseOperationControl {
    cancelled: AtomicBool,
    handle: Mutex<Option<CatalogBrowseCancel>>,
}

impl BrowseOperationControl {
    fn attach(&self, handle: CatalogBrowseCancel) {
        *self.handle.lock() = Some(handle);
        if self.cancelled.load(Ordering::Acquire) {
            self.cancel();
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(handle) = self.handle.lock().as_ref() {
            handle.cancel();
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

fn spawn_catalog_feed(
    catalog: NmpManifestCatalog,
    state: Arc<Mutex<CatalogFeedState>>,
    signal: watch::Sender<u64>,
    control: Arc<BrowseOperationControl>,
) -> Result<JoinHandle<()>, RuntimeCatalogError> {
    thread::Builder::new()
        .name("runtime-catalog-feed".to_owned())
        .spawn(move || run_catalog_feed(catalog, state, signal, control))
        .map_err(|error| RuntimeCatalogError::WorkerUnavailable {
            reason: error.to_string(),
        })
}

fn run_catalog_feed(
    catalog: NmpManifestCatalog,
    state: Arc<Mutex<CatalogFeedState>>,
    signal: watch::Sender<u64>,
    control: Arc<BrowseOperationControl>,
) {
    let request = match CatalogBrowseRequest::new(None) {
        Ok(request) => request,
        Err(error) => {
            publish_catalog_failure(&state, &signal, map_browse_error(error));
            return;
        }
    };
    let observation = match catalog.observe_browse(request) {
        Ok(observation) => observation,
        Err(error) => {
            publish_catalog_failure(&state, &signal, map_browse_error(error));
            return;
        }
    };
    control.attach(observation.cancel_handle());
    if control.is_cancelled() {
        return;
    }
    if let Err(error) = observation.request_rows(MAXIMUM_PAGE_ENTRIES) {
        publish_catalog_failure(&state, &signal, map_browse_error(error));
        return;
    }
    loop {
        let frame = match observation.recv() {
            Ok(frame) => frame,
            Err(error) => {
                if !control.is_cancelled() {
                    publish_catalog_failure(&state, &signal, map_browse_error(error));
                }
                return;
            }
        };
        {
            let mut latest = state.lock();
            if latest.closed {
                return;
            }
            if !advance_catalog_revision(&mut latest) {
                drop(latest);
                bump_catalog_signal(&signal);
                return;
            }
            latest.candidates.clear();
            for candidate in frame.candidates.iter() {
                if let Some(coordinate) = candidate_coordinate(candidate) {
                    latest
                        .candidates
                        .insert(candidate.event_id.to_string(), coordinate);
                }
            }
            latest.frame = Some(frame);
            latest.failure = None;
        }
        bump_catalog_signal(&signal);
    }
}

fn publish_catalog_failure(
    state: &Mutex<CatalogFeedState>,
    signal: &watch::Sender<u64>,
    failure: RuntimeCatalogError,
) {
    let mut latest = state.lock();
    if latest.closed {
        return;
    }
    advance_catalog_revision(&mut latest);
    latest.failure = Some(failure);
    latest.closed = true;
    drop(latest);
    bump_catalog_signal(signal);
}

fn bump_catalog_signal(signal: &watch::Sender<u64>) {
    signal.send_modify(|revision| {
        *revision = revision.saturating_add(1);
    });
}

fn advance_catalog_revision(state: &mut CatalogFeedState) -> bool {
    let Some(revision) = state.revision.checked_add(1) else {
        state.failure = Some(RuntimeCatalogError::WorkerUnavailable {
            reason: "catalog feed revision space is exhausted".to_owned(),
        });
        state.closed = true;
        return false;
    };
    state.revision = revision;
    true
}

#[derive(Debug)]
struct OneShotAdmission {
    maximum: usize,
    active: AtomicUsize,
}

impl OneShotAdmission {
    fn new(maximum: usize) -> Self {
        Self {
            maximum,
            active: AtomicUsize::new(0),
        }
    }

    fn reserve(self: &Arc<Self>) -> Result<OneShotPermit, RuntimeCatalogError> {
        let mut active = self.active.load(Ordering::Acquire);
        loop {
            if active >= self.maximum {
                return Err(RuntimeCatalogError::Busy {
                    maximum: self.maximum as u64,
                });
            }
            match self.active.compare_exchange_weak(
                active,
                active + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(OneShotPermit {
                        admission: Arc::clone(self),
                    });
                }
                Err(observed) => active = observed,
            }
        }
    }
}

#[derive(Debug)]
struct OneShotPermit {
    admission: Arc<OneShotAdmission>,
}

impl Drop for OneShotPermit {
    fn drop(&mut self) {
        self.admission.active.fetch_sub(1, Ordering::AcqRel);
    }
}

fn connecting_catalog_frame() -> CatalogBrowseFrame {
    CatalogBrowseFrame {
        candidates: Arc::from([]),
        refused: Arc::from([]),
        locally_filtered_rows: 0,
        projection_limit_rows: 0,
        source_evidence: Arc::from([]),
        shortfalls: Arc::from([]),
        window_load: WindowLoad::Requesting,
    }
}

fn browse_feed_state(
    state: &CatalogFeedState,
    request: &CatalogBrowseRequest,
    query_was_local_filter: bool,
) -> Result<RuntimeCatalogPage, RuntimeCatalogError> {
    if let Some(error) = &state.failure {
        return Err(error.clone());
    }
    if state.closed {
        return Err(RuntimeCatalogError::Cancelled);
    }
    let frame = state
        .frame
        .as_ref()
        .map(|frame| frame.filtered(request))
        .unwrap_or_else(connecting_catalog_frame);
    Ok(project_page(&frame, query_was_local_filter))
}

fn candidate_coordinate(candidate: &CatalogManifestCandidate) -> Option<ManifestCoordinate> {
    let author = nmp_native_artifact::Sha256Digest::parse(candidate.author.to_string()).ok()?;
    match candidate.kind {
        5_129 => Some(ManifestCoordinate::Snapshot {
            event_id: nmp_native_artifact::Sha256Digest::parse(candidate.event_id.to_string())
                .ok()?,
            author,
        }),
        15_129 => Some(ManifestCoordinate::Root { author }),
        35_129 => Some(ManifestCoordinate::Named {
            author,
            d_tag: Arc::clone(candidate.d_tag.as_ref()?),
        }),
        _ => None,
    }
}

fn project_page(frame: &CatalogBrowseFrame, query_was_local_filter: bool) -> RuntimeCatalogPage {
    RuntimeCatalogPage {
        entries: frame.candidates.iter().map(project_entry).collect(),
        query_was_local_filter,
        locally_filtered_rows: usize_to_u64(frame.locally_filtered_rows),
        projection_limited_rows: usize_to_u64(frame.projection_limit_rows),
        refused_rows: usize_to_u64(frame.refused.len()),
        has_more: frame.projection_limit_rows > 0,
        window: match frame.window_load {
            WindowLoad::Idle => RuntimeCatalogWindowState::Idle,
            WindowLoad::Requesting => RuntimeCatalogWindowState::Requesting,
            WindowLoad::Returned { added } => RuntimeCatalogWindowState::Returned {
                added: usize_to_u64(added),
            },
            WindowLoad::AtBound { max } => RuntimeCatalogWindowState::AtBound {
                maximum: usize_to_u64(max),
            },
            _ => RuntimeCatalogWindowState::Unknown,
        },
        sources: frame.source_evidence.iter().map(project_source).collect(),
        shortfalls: frame
            .shortfalls
            .iter()
            .map(|shortfall| match shortfall {
                CatalogShortfall::NoPlannedSource => RuntimeCatalogShortfall::NoPlannedSource,
                CatalogShortfall::NoResolvedDemand => RuntimeCatalogShortfall::NoResolvedDemand,
                CatalogShortfall::LocalLimit => RuntimeCatalogShortfall::LocalLimit,
            })
            .collect(),
    }
}

fn project_entry(candidate: &CatalogManifestCandidate) -> RuntimeCatalogEntry {
    RuntimeCatalogEntry {
        event_id: candidate.event_id.to_string(),
        coordinate: candidate_coordinate(candidate)
            .as_ref()
            .map(catalog_coordinate_string),
        manifest_author: candidate.author.to_string(),
        kind: candidate.kind,
        created_at: candidate.created_at,
        d_tag: candidate.d_tag.as_deref().map(str::to_owned),
        title: candidate.title.as_deref().map(str::to_owned),
        description: candidate.description.as_deref().map(str::to_owned),
        aggregate_hash: candidate.aggregate.as_deref().map(str::to_owned),
        observed_sources: candidate
            .observed_sources
            .iter()
            .map(ToString::to_string)
            .collect(),
    }
}

fn project_source(source: &CatalogSourceEvidence) -> RuntimeCatalogSource {
    RuntimeCatalogSource {
        relay: source.relay.to_string(),
        access: match &source.access {
            CatalogAccessContext::Public => RuntimeCatalogSourceAccess::Public,
            CatalogAccessContext::Nip42 { public_key } => RuntimeCatalogSourceAccess::Nip42 {
                public_key: public_key.to_string(),
            },
        },
        reconciled_through: source.reconciled_through,
        state: match source.status {
            CatalogSourceStatus::Requesting => RuntimeCatalogSourceState::Requesting,
            CatalogSourceStatus::Connecting => RuntimeCatalogSourceState::Connecting,
            CatalogSourceStatus::Disconnected => RuntimeCatalogSourceState::Disconnected,
            CatalogSourceStatus::AwaitingAuth => RuntimeCatalogSourceState::AwaitingAuth,
            CatalogSourceStatus::AuthDenied => RuntimeCatalogSourceState::AuthDenied,
            CatalogSourceStatus::Error => RuntimeCatalogSourceState::Error,
        },
    }
}

fn project_review(token: &str, review: &ArtifactReview) -> RuntimeCatalogReview {
    let summary = review.summary();
    let (manifest_author, d_tag) = coordinate_identity(summary.coordinate());
    RuntimeCatalogReview {
        token: token.to_owned(),
        event_id: summary.event_id().as_str().to_owned(),
        coordinate: catalog_coordinate_string(summary.coordinate()),
        manifest_author,
        d_tag,
        title: summary.title().map(str::to_owned),
        description: summary.description().map(str::to_owned),
        aggregate_hash: summary.aggregate().as_str().to_owned(),
        capabilities: review_capabilities(summary),
        blob_sources: summary.servers().map(str::to_owned).collect(),
        provenance: project_lookup_facts(summary.lookup_facts()),
    }
}

fn review_capabilities(
    summary: &nmp_native_catalog_resolver::ArtifactReviewSummary,
) -> Vec<RuntimeCatalogCapability> {
    let (author, d_tag) = coordinate_identity(summary.coordinate());
    if author == GOOD_MORNING_AUTHOR
        && d_tag.as_deref() == Some(GOOD_MORNING_D_TAG)
        && summary.aggregate().as_str() == GOOD_MORNING_AGGREGATE_HASH
    {
        return GOOD_MORNING_CAPABILITY_PROFILE
            .iter()
            .map(|(domain, requirement)| RuntimeCatalogCapability {
                domain: (*domain).to_owned(),
                requirement: match requirement {
                    CapabilityRequirement::Required => RuntimePermissionRequirement::Required,
                    CapabilityRequirement::Optional => RuntimePermissionRequirement::Optional,
                },
            })
            .collect();
    }
    summary
        .requirements()
        .map(|domain| RuntimeCatalogCapability {
            domain: domain.to_owned(),
            requirement: RuntimePermissionRequirement::Required,
        })
        .collect()
}

fn coordinate_identity(coordinate: &ManifestCoordinate) -> (String, Option<String>) {
    match coordinate {
        ManifestCoordinate::Snapshot { author, .. } | ManifestCoordinate::Root { author } => {
            (author.as_str().to_owned(), None)
        }
        ManifestCoordinate::Named { author, d_tag } => {
            (author.as_str().to_owned(), Some(d_tag.to_string()))
        }
    }
}

fn catalog_coordinate_string(coordinate: &ManifestCoordinate) -> String {
    match coordinate {
        ManifestCoordinate::Snapshot { event_id, author } => {
            format!("5129:{}:{}", event_id.as_str(), author.as_str())
        }
        ManifestCoordinate::Root { author } => format!("15129:{}", author.as_str()),
        ManifestCoordinate::Named { author, d_tag } => {
            format!("35129:{}:{d_tag}", author.as_str())
        }
    }
}

fn project_lookup_facts(facts: &[CoordinateLookupFact]) -> Vec<RuntimeCatalogProvenance> {
    facts
        .iter()
        .map(|fact| RuntimeCatalogProvenance {
            source: fact.source().to_owned(),
            state: match fact.state() {
                CoordinateLookupState::Observed { rows } => RuntimeCatalogLookupState::Observed {
                    rows: usize_to_u64(*rows),
                },
                CoordinateLookupState::Shortfall { reason } => {
                    RuntimeCatalogLookupState::Shortfall {
                        reason: reason.to_string(),
                    }
                }
                CoordinateLookupState::Selected { event_id } => {
                    RuntimeCatalogLookupState::Selected {
                        event_id: event_id.to_string(),
                    }
                }
            },
        })
        .collect()
}

fn map_browse_error(error: ManifestCatalogError) -> RuntimeCatalogError {
    match error {
        ManifestCatalogError::BrowseCapacity { maximum }
        | ManifestCatalogError::LookupCapacity { maximum } => RuntimeCatalogError::Busy {
            maximum: maximum as u64,
        },
        other => RuntimeCatalogError::Browse {
            reason: other.to_string(),
        },
    }
}

fn map_resolve_error(error: ResolveError) -> RuntimeCatalogError {
    match error {
        ResolveError::Cancelled => RuntimeCatalogError::Cancelled,
        ResolveError::Saturated { maximum } => RuntimeCatalogError::Busy {
            maximum: maximum as u64,
        },
        ResolveError::ReviewSaturated { maximum } => RuntimeCatalogError::ReviewCapacity {
            maximum: maximum as u64,
        },
        ResolveError::ReviewStale | ResolveError::ReviewForeign => RuntimeCatalogError::StaleReview,
        ResolveError::NotFound { facts } => RuntimeCatalogError::NotFound {
            provenance: project_lookup_facts(&facts),
        },
        other => RuntimeCatalogError::Resolve {
            reason: other.to_string(),
        },
    }
}

pub fn project_catalog_error(error: RuntimeCatalogError) -> RuntimeCatalogFailure {
    let provenance = match &error {
        RuntimeCatalogError::NotFound { provenance } => provenance.clone(),
        _ => Vec::new(),
    };
    let code = match &error {
        RuntimeCatalogError::InvalidConfiguration { .. } => "invalid-configuration",
        RuntimeCatalogError::Busy { .. } => "busy",
        RuntimeCatalogError::Deadline { .. } => "deadline",
        RuntimeCatalogError::WorkerUnavailable { .. } => "worker-unavailable",
        RuntimeCatalogError::Browse { .. } => "browse-refused",
        RuntimeCatalogError::InvalidCoordinate { .. } => "invalid-coordinate",
        RuntimeCatalogError::NotFound { .. } => "not-found",
        RuntimeCatalogError::ReviewCapacity { .. } => "review-capacity",
        RuntimeCatalogError::StaleReview => "stale-review",
        RuntimeCatalogError::Cancelled => "cancelled",
        RuntimeCatalogError::Resolve { .. } => "resolve-refused",
    };
    RuntimeCatalogFailure {
        code: code.to_owned(),
        detail: error.to_string(),
        provenance,
    }
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn usize_to_u64(value: usize) -> u64 {
    value.try_into().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;
