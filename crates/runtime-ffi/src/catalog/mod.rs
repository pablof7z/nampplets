//! Bounded Rust-owned catalog browse, review, and verified-artifact handoff.
//!
//! NMP remains the canonical manifest-event owner. This module keeps only
//! screen-shaped browse projections and at most sixteen opaque, exact reviews.
//! Confirming a review returns immutable verified bytes; it never installs,
//! grants, or launches a napplet.

mod admission;
mod feed;
mod install_eligibility;
mod projection;
mod review;
mod types;

#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use nmp_native_artifact::{ArtifactLimits, ArtifactSourcePolicy, FileArtifactCache};
use nmp_native_catalog_resolver::{
    ArtifactReview, CatalogResolver, MemorySealedArtifactCache, ResolverLimits,
    RustHttpsAcquisitionConfig, RustHttpsAcquisitionPort,
};
use nmp_native_nmp_adapter::{NmpDataPlane, catalog::CatalogBrowseRequest};
use parking_lot::Mutex;
use tokio::sync::watch;

use admission::{ActiveCancellation, BrowseOperationControl, OneShotAdmission};
use feed::{
    CatalogFeedState, advance_catalog_revision, browse_feed_state, bump_catalog_signal,
    spawn_catalog_feed,
};
use projection::map_browse_error;

pub use install_eligibility::RuntimeCatalogInstallEligibility;
pub use projection::project_catalog_error;
pub use types::{
    RuntimeCatalogCancellationResult, RuntimeCatalogCapability, RuntimeCatalogConfirmation,
    RuntimeCatalogConfirmationResult, RuntimeCatalogEntry, RuntimeCatalogError,
    RuntimeCatalogFailure, RuntimeCatalogFeedSnapshot, RuntimeCatalogLookupState,
    RuntimeCatalogPage, RuntimeCatalogPageResult, RuntimeCatalogProvenance, RuntimeCatalogReview,
    RuntimeCatalogReviewResult, RuntimeCatalogShortfall, RuntimeCatalogSource,
    RuntimeCatalogSourceAccess, RuntimeCatalogSourceState, RuntimeCatalogWindowState,
};

const MAXIMUM_PAGE_ENTRIES: usize = 100;
const MAXIMUM_PENDING_REVIEWS: usize = 16;
const MAXIMUM_ONE_SHOT_OPERATIONS: usize = 4;
const OPERATION_DEADLINE: Duration = Duration::from_secs(15);

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
