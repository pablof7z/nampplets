//! [`CatalogResolver`], the public entry point that ties bounded manifest
//! lookup, policy-checked HTTPS acquisition, and the sealed artifact cache
//! together behind a review/confirm workflow.

mod blob_source;

use std::sync::Arc;

use nmp_native_artifact::{
    ArtifactLimits, ArtifactSourcePolicy, FileArtifactCache, ManifestCoordinate,
    ManifestEventVerifier, Sha256Digest, SignedArtifactResolver,
};
use parking_lot::Mutex;

use crate::{
    ArtifactReview, ArtifactReviewSummary, CancellationToken, CoordinateLookupFact,
    CoordinateLookupState, HttpsAcquisitionPort, ManifestLookupCompletion, ManifestLookupPort,
    ManifestLookupRequest, ResolutionOrigin, ResolveError, ResolvedArtifact, ResolverLimits,
    SealedArtifactCache, SealedArtifactKey,
    cache::index_matches_coordinate,
    review::{ArtifactReviewPayload, ReviewAdmission},
};
pub(crate) use blob_source::SafeManifestBlobSource;

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
pub(crate) struct ResolverIdentity;

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
pub(crate) struct Admission {
    maximum: usize,
    active: Mutex<usize>,
}

impl Admission {
    pub(crate) fn new(maximum: usize) -> Self {
        Self {
            maximum,
            active: Mutex::new(0),
        }
    }

    pub(crate) fn reserve(&self) -> Result<AdmissionPermit<'_>, ResolveError> {
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

pub(crate) struct AdmissionPermit<'a> {
    admission: &'a Admission,
}

impl Drop for AdmissionPermit<'_> {
    fn drop(&mut self) {
        let mut active = self.admission.active.lock();
        *active = active.saturating_sub(1);
    }
}

pub(crate) fn bounded_reason(reason: Arc<str>, maximum: usize) -> Arc<str> {
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
