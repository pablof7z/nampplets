use std::sync::Arc;

use nmp_native_artifact::{ManifestCoordinate, Sha256Digest};
use parking_lot::Mutex;

use crate::{CoordinateLookupFact, ResolveError, resolver::ResolverIdentity};

#[derive(Clone, Debug)]
pub struct ArtifactReviewSummary {
    pub(crate) coordinate: ManifestCoordinate,
    pub(crate) event_id: Sha256Digest,
    pub(crate) aggregate: Sha256Digest,
    pub(crate) title: Option<Arc<str>>,
    pub(crate) description: Option<Arc<str>>,
    pub(crate) requirements: Arc<[Arc<str>]>,
    pub(crate) servers: Arc<[Arc<str>]>,
    pub(crate) lookup_facts: Arc<[CoordinateLookupFact]>,
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
pub(crate) struct ArtifactReviewPayload {
    pub(crate) signed_event_json: Arc<[u8]>,
}

/// Opaque exact-install operation. The selected signed manifest and its
/// coordinate, event id, and aggregate are frozen when this value is created.
/// Confirming never performs another catalog lookup.
#[derive(Debug)]
pub struct ArtifactReview {
    pub(crate) owner: Arc<ResolverIdentity>,
    pub(crate) summary: ArtifactReviewSummary,
    pub(crate) payload: Mutex<Option<ArtifactReviewPayload>>,
    pub(crate) lease: Mutex<Option<ReviewLease>>,
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

    pub(crate) fn take_for(
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

#[derive(Debug)]
pub(crate) struct ReviewAdmission {
    maximum: usize,
    active: Mutex<usize>,
}

impl ReviewAdmission {
    pub(crate) fn new(maximum: usize) -> Self {
        Self {
            maximum,
            active: Mutex::new(0),
        }
    }

    pub(crate) fn reserve(self: &Arc<Self>) -> Result<ReviewLease, ResolveError> {
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
pub(crate) struct ReviewLease {
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
