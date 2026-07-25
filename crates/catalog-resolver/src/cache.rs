use std::{collections::BTreeMap, fmt, sync::Arc};

use nmp_native_artifact::{
    ManifestCoordinate, Sha256Digest, VerifiedArtifactHandle, VerifiedArtifactIndex,
};
use parking_lot::Mutex;
use thiserror::Error;

use crate::{AcquisitionFact, CoordinateLookupFact};

const KIND_SNAPSHOT: u16 = 5_129;
const KIND_ROOT: u16 = 15_129;
const KIND_NAMED: u16 = 35_129;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionOrigin {
    OnlineVerified,
    OfflineSealed,
}

#[derive(Clone, Debug)]
pub struct ResolvedArtifact {
    pub(crate) handle: VerifiedArtifactHandle,
    pub(crate) origin: ResolutionOrigin,
    pub(crate) lookup_facts: Arc<[CoordinateLookupFact]>,
    pub(crate) acquisition_facts: Arc<[AcquisitionFact]>,
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
    pub(crate) state: Mutex<MemoryCacheState>,
}

#[derive(Debug, Default)]
pub(crate) struct MemoryCacheState {
    total_bytes: usize,
    pub(crate) entries: BTreeMap<SealedArtifactKey, VerifiedArtifactHandle>,
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

pub(crate) fn index_matches_coordinate(
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
