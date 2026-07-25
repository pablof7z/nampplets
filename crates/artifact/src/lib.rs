//! Verified artifact acquisition and immutable content-addressed storage.
//!
//! No blob is committed until every path hash and the baseline-supplied
//! aggregate policy succeeds.

use std::{collections::BTreeSet, fmt, io, sync::Arc};

use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

mod aggregate;
mod file_cache;
mod manifest;
mod resolver;

pub(crate) use aggregate::nip5a_path_tags_aggregate;
pub use aggregate::{FramedSha256Aggregate, MemoryBlobSource, Nip5aPathTagsAggregate};
pub use file_cache::FileArtifactCache;
pub use manifest::{
    ArtifactMode, ArtifactSourcePolicy, BlobFetchRequest, BlobFetchResponse, ManifestBlobSource,
    ManifestCoordinate, ManifestError, ManifestEventLimits, ManifestEventVerifier,
    SignedArtifactResolver, VerifiedArtifactHandle, VerifiedArtifactIndex,
    VerifiedArtifactIndexEntry, VerifiedManifest,
};
pub use resolver::{ArtifactResolver, CachedArtifact};

pub const INDEX_PATH: &str = "/index.html";

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    pub fn parse(value: impl Into<String>) -> Result<Self, ArtifactError> {
        let value = value.into();
        if value.len() != 64
            || value
                .bytes()
                .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
        {
            return Err(ArtifactError::InvalidDigest);
        }
        Ok(Self(value))
    }

    pub fn of(bytes: &[u8]) -> Self {
        Self(hex::encode(Sha256::digest(bytes)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Debug for Sha256Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("Sha256Digest")
            .field(&self.0)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactPath {
    pub path: String,
    pub sha256: Sha256Digest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactManifest {
    pub aggregate: Sha256Digest,
    pub paths: Vec<ArtifactPath>,
}

impl ArtifactManifest {
    pub fn validate(&self, limits: &ArtifactLimits) -> Result<(), ArtifactError> {
        if self.paths.is_empty() || self.paths.len() > limits.maximum_files {
            return Err(ArtifactError::FileCount {
                actual: self.paths.len(),
                maximum: limits.maximum_files,
            });
        }

        let mut seen = BTreeSet::new();
        for entry in &self.paths {
            validate_artifact_path(&entry.path)?;
            if !seen.insert(entry.path.as_str()) {
                return Err(ArtifactError::DuplicatePath(entry.path.clone()));
            }
        }
        if !seen.contains(INDEX_PATH) {
            return Err(ArtifactError::MissingIndex);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArtifactLimits {
    pub maximum_files: usize,
    pub maximum_file_bytes: usize,
    pub maximum_total_bytes: usize,
}

impl Default for ArtifactLimits {
    fn default() -> Self {
        Self {
            maximum_files: 256,
            maximum_file_bytes: 8 * 1024 * 1024,
            maximum_total_bytes: 32 * 1024 * 1024,
        }
    }
}

pub trait BlobSource: Send + Sync + fmt::Debug {
    fn open(
        &self,
        path: &str,
        expected: &Sha256Digest,
    ) -> Result<Box<dyn io::Read + Send>, BlobSourceError>;
}

#[derive(Debug, Error)]
#[error("{reason}")]
pub struct BlobSourceError {
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct VerifiedFile {
    pub path: Arc<str>,
    pub digest: Sha256Digest,
    pub bytes: Arc<[u8]>,
}

/// Compatibility-lock-owned aggregate computation.
///
/// The runtime intentionally has no guessed default. A caller must select the
/// exact pinned NIP-5D/NIP-5A policy.
pub trait AggregateVerifier: Send + Sync + fmt::Debug {
    fn compute(&self, files: &[VerifiedFile]) -> Result<Sha256Digest, ArtifactError>;
}

pub trait ArtifactCache: Send + Sync + fmt::Debug {
    fn commit(
        &self,
        aggregate: &Sha256Digest,
        files: &[VerifiedFile],
    ) -> Result<CachedArtifact, ArtifactError>;
    fn contains(&self, aggregate: &Sha256Digest) -> bool;
}

#[derive(Debug, Error)]
pub enum ArtifactError {
    #[error("artifact limits must be finite and non-zero")]
    InvalidLimits,
    #[error("invalid lowercase SHA-256 digest")]
    InvalidDigest,
    #[error("artifact contains {actual} files; the maximum is {maximum}")]
    FileCount { actual: usize, maximum: usize },
    #[error("invalid artifact path {0}")]
    InvalidPath(String),
    #[error("duplicate artifact path {0}")]
    DuplicatePath(String),
    #[error("artifact has no verified /index.html")]
    MissingIndex,
    #[error("blob source failed for {path}: {source}")]
    Source {
        path: String,
        #[source]
        source: BlobSourceError,
    },
    #[error("blob read failed for {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("blob {path} is {actual} bytes; the maximum is {maximum}")]
    FileTooLarge {
        path: String,
        actual: usize,
        maximum: usize,
    },
    #[error("artifact is {actual} bytes; the maximum is {maximum}")]
    TotalTooLarge { actual: usize, maximum: usize },
    #[error("blob hash mismatch for {path}: expected {expected:?}, got {actual:?}")]
    PathHashMismatch {
        path: String,
        expected: Sha256Digest,
        actual: Sha256Digest,
    },
    #[error("aggregate mismatch: expected {expected:?}, got {actual:?}")]
    AggregateMismatch {
        expected: Sha256Digest,
        actual: Sha256Digest,
    },
    #[error("aggregate policy failed: {0}")]
    AggregatePolicy(String),
    #[error("artifact cache I/O failed: {0}")]
    CacheIo(#[source] io::Error),
    #[error("artifact cache index could not be decoded: {0}")]
    CacheIndex(#[source] serde_json::Error),
    #[error("artifact cache index for {aggregate:?} is unreadable: {source}")]
    CorruptCacheIndex {
        aggregate: Sha256Digest,
        #[source]
        source: io::Error,
    },
    #[error("artifact cache for {aggregate:?} is corrupt at {path}")]
    CorruptCache {
        aggregate: Sha256Digest,
        path: String,
    },
    #[error("verified artifact has no path {0}")]
    MissingCachedPath(String),
}

pub(crate) fn validate_artifact_path(path: &str) -> Result<(), ArtifactError> {
    if !path.starts_with('/')
        || path.len() < 2
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .skip(1)
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err(ArtifactError::InvalidPath(path.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
