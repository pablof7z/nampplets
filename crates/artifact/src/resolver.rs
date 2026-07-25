use std::{collections::BTreeMap, fmt, fs::File, path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::{
    AggregateVerifier, ArtifactCache, ArtifactError, ArtifactLimits, ArtifactManifest, BlobSource,
    Sha256Digest, VerifiedFile,
    file_cache::{CACHE_BLOBS_DIRECTORY, ReadBoundedError, read_bounded},
    validate_artifact_path,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedArtifact {
    pub aggregate: Sha256Digest,
    pub files: usize,
    pub bytes: usize,
    pub(crate) root: PathBuf,
    pub(crate) index: Arc<BTreeMap<String, CachedBlob>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CachedBlob {
    pub(crate) digest: Sha256Digest,
    pub(crate) bytes: usize,
}

impl CachedArtifact {
    /// Read one exact logical path from digest-addressed storage and verify it
    /// again before returning executable bytes.
    ///
    /// Callers never derive native filesystem paths from artifact URLs.
    pub fn read_verified(
        &self,
        logical_path: &str,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, ArtifactError> {
        validate_artifact_path(logical_path)?;
        let entry = self
            .index
            .get(logical_path)
            .ok_or_else(|| ArtifactError::MissingCachedPath(logical_path.to_owned()))?;
        if entry.bytes > maximum_bytes {
            return Err(ArtifactError::FileTooLarge {
                path: logical_path.to_owned(),
                actual: entry.bytes,
                maximum: maximum_bytes,
            });
        }
        let path = self
            .root
            .join(CACHE_BLOBS_DIRECTORY)
            .join(entry.digest.as_str());
        let bytes = read_bounded(
            Box::new(File::open(path).map_err(ArtifactError::CacheIo)?),
            maximum_bytes,
        )
        .map_err(|error| match error {
            ReadBoundedError::Io(source) => ArtifactError::CacheIo(source),
            ReadBoundedError::TooLarge { actual, maximum } => ArtifactError::FileTooLarge {
                path: logical_path.to_owned(),
                actual,
                maximum,
            },
        })?;
        let actual = Sha256Digest::of(&bytes);
        if actual != entry.digest || bytes.len() != entry.bytes {
            return Err(ArtifactError::CorruptCache {
                aggregate: self.aggregate.clone(),
                path: logical_path.to_owned(),
            });
        }
        Ok(bytes)
    }

    pub fn logical_paths(&self) -> impl ExactSizeIterator<Item = &str> {
        self.index.keys().map(String::as_str)
    }
}

pub struct ArtifactResolver<'a> {
    limits: ArtifactLimits,
    source: &'a dyn BlobSource,
    aggregate: &'a dyn AggregateVerifier,
    cache: &'a dyn ArtifactCache,
}

impl fmt::Debug for ArtifactResolver<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ArtifactResolver")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl<'a> ArtifactResolver<'a> {
    pub fn new(
        limits: ArtifactLimits,
        source: &'a dyn BlobSource,
        aggregate: &'a dyn AggregateVerifier,
        cache: &'a dyn ArtifactCache,
    ) -> Result<Self, ArtifactError> {
        if limits.maximum_files == 0
            || limits.maximum_file_bytes == 0
            || limits.maximum_total_bytes == 0
        {
            return Err(ArtifactError::InvalidLimits);
        }
        Ok(Self {
            limits,
            source,
            aggregate,
            cache,
        })
    }

    pub fn resolve(&self, manifest: &ArtifactManifest) -> Result<CachedArtifact, ArtifactError> {
        manifest.validate(&self.limits)?;

        let mut files = Vec::with_capacity(manifest.paths.len());
        let mut total_bytes = 0usize;
        for entry in &manifest.paths {
            let reader = self
                .source
                .open(&entry.path, &entry.sha256)
                .map_err(|source| ArtifactError::Source {
                    path: entry.path.clone(),
                    source,
                })?;
            let bytes =
                read_bounded(reader, self.limits.maximum_file_bytes).map_err(
                    |error| match error {
                        ReadBoundedError::Io(source) => ArtifactError::Read {
                            path: entry.path.clone(),
                            source,
                        },
                        ReadBoundedError::TooLarge { actual, maximum } => {
                            ArtifactError::FileTooLarge {
                                path: entry.path.clone(),
                                actual,
                                maximum,
                            }
                        }
                    },
                )?;
            total_bytes =
                total_bytes
                    .checked_add(bytes.len())
                    .ok_or(ArtifactError::TotalTooLarge {
                        actual: usize::MAX,
                        maximum: self.limits.maximum_total_bytes,
                    })?;
            if total_bytes > self.limits.maximum_total_bytes {
                return Err(ArtifactError::TotalTooLarge {
                    actual: total_bytes,
                    maximum: self.limits.maximum_total_bytes,
                });
            }
            let actual = Sha256Digest::of(&bytes);
            if actual != entry.sha256 {
                return Err(ArtifactError::PathHashMismatch {
                    path: entry.path.clone(),
                    expected: entry.sha256.clone(),
                    actual,
                });
            }
            files.push(VerifiedFile {
                path: Arc::from(entry.path.as_str()),
                digest: entry.sha256.clone(),
                bytes: Arc::from(bytes),
            });
        }

        let actual_aggregate = self.aggregate.compute(&files)?;
        if actual_aggregate != manifest.aggregate {
            return Err(ArtifactError::AggregateMismatch {
                expected: manifest.aggregate.clone(),
                actual: actual_aggregate,
            });
        }

        self.cache.commit(&manifest.aggregate, &files)
    }
}
