use std::sync::Arc;

use nmp_native_artifact::Sha256Digest;
use thiserror::Error;

use crate::{AcquisitionFact, AcquisitionRefusal, CoordinateLookupFact, SealedCacheError};

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
