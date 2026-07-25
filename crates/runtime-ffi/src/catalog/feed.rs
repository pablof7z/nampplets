//! The profile-owned permanent catalog feed worker and its bounded state.

use std::{
    collections::BTreeMap,
    sync::Arc,
    thread::{self, JoinHandle},
};

use nmp::WindowLoad;
use nmp_native_artifact::ManifestCoordinate;
use nmp_native_nmp_adapter::catalog::{
    CatalogBrowseFrame, CatalogBrowseRequest, NmpManifestCatalog,
};
use parking_lot::Mutex;
use tokio::sync::watch;

use super::{
    MAXIMUM_PAGE_ENTRIES,
    admission::BrowseOperationControl,
    projection::{candidate_coordinate, map_browse_error, project_page},
    types::{RuntimeCatalogError, RuntimeCatalogPage},
};

#[derive(Debug)]
pub(super) struct CatalogFeedState {
    pub(super) revision: u64,
    pub(super) frame: Option<CatalogBrowseFrame>,
    pub(super) candidates: BTreeMap<String, ManifestCoordinate>,
    pub(super) failure: Option<RuntimeCatalogError>,
    pub(super) closed: bool,
}

pub(super) fn spawn_catalog_feed(
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

pub(super) fn bump_catalog_signal(signal: &watch::Sender<u64>) {
    signal.send_modify(|revision| {
        *revision = revision.saturating_add(1);
    });
}

pub(super) fn advance_catalog_revision(state: &mut CatalogFeedState) -> bool {
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
pub(super) fn connecting_catalog_frame() -> CatalogBrowseFrame {
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

pub(super) fn browse_feed_state(
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
