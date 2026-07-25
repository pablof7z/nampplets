//! Verified artifact acquisition and immutable content-addressed storage.
//!
//! No blob is committed until every path hash and the baseline-supplied
//! aggregate policy succeeds.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use parking_lot::Mutex;
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

mod manifest;

pub use manifest::{
    ArtifactMode, ArtifactSourcePolicy, BlobFetchRequest, BlobFetchResponse, ManifestBlobSource,
    ManifestCoordinate, ManifestError, ManifestEventLimits, ManifestEventVerifier,
    SignedArtifactResolver, VerifiedArtifactHandle, VerifiedArtifactIndex,
    VerifiedArtifactIndexEntry, VerifiedManifest,
};

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
    ) -> Result<Box<dyn Read + Send>, BlobSourceError>;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedArtifact {
    pub aggregate: Sha256Digest,
    pub files: usize,
    pub bytes: usize,
    root: PathBuf,
    index: Arc<BTreeMap<String, CachedBlob>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CachedBlob {
    digest: Sha256Digest,
    bytes: usize,
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

#[derive(Debug)]
pub struct FileArtifactCache {
    root: PathBuf,
    commit_lock: Mutex<()>,
}

impl FileArtifactCache {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, ArtifactError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(ArtifactError::CacheIo)?;
        Ok(Self {
            root,
            commit_lock: Mutex::new(()),
        })
    }
}

impl ArtifactCache for FileArtifactCache {
    fn commit(
        &self,
        aggregate: &Sha256Digest,
        files: &[VerifiedFile],
    ) -> Result<CachedArtifact, ArtifactError> {
        let _guard = self.commit_lock.lock();
        let destination = self.root.join(aggregate.as_str());
        if destination.is_dir() {
            return inspect_cached_artifact(&destination, aggregate, Some(files));
        }

        let index = cache_index_for(aggregate, files)?;
        let staging = tempfile::Builder::new()
            .prefix(".artifact-")
            .tempdir_in(&self.root)
            .map_err(ArtifactError::CacheIo)?;
        let blobs_directory = staging.path().join(CACHE_BLOBS_DIRECTORY);
        fs::create_dir(&blobs_directory).map_err(ArtifactError::CacheIo)?;
        for verified in files {
            let output = blobs_directory.join(verified.digest.as_str());
            if output.exists() {
                continue;
            }
            let mut file = File::create(&output).map_err(ArtifactError::CacheIo)?;
            file.write_all(&verified.bytes)
                .map_err(ArtifactError::CacheIo)?;
            file.sync_all().map_err(ArtifactError::CacheIo)?;
            let mut permissions = file
                .metadata()
                .map_err(ArtifactError::CacheIo)?
                .permissions();
            permissions.set_readonly(true);
            fs::set_permissions(&output, permissions).map_err(ArtifactError::CacheIo)?;
        }
        let index_bytes = serde_json::to_vec(&index).map_err(ArtifactError::CacheIndex)?;
        let index_path = staging.path().join(CACHE_INDEX_FILE);
        let mut index_file = File::create(&index_path).map_err(ArtifactError::CacheIo)?;
        index_file
            .write_all(&index_bytes)
            .map_err(ArtifactError::CacheIo)?;
        index_file.sync_all().map_err(ArtifactError::CacheIo)?;
        let mut permissions = index_file
            .metadata()
            .map_err(ArtifactError::CacheIo)?
            .permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&index_path, permissions).map_err(ArtifactError::CacheIo)?;
        File::open(staging.path())
            .and_then(|directory| directory.sync_all())
            .map_err(ArtifactError::CacheIo)?;
        let staging_path = staging.keep();
        fs::rename(&staging_path, &destination).map_err(ArtifactError::CacheIo)?;
        File::open(&self.root)
            .and_then(|directory| directory.sync_all())
            .map_err(ArtifactError::CacheIo)?;

        inspect_cached_artifact(&destination, aggregate, Some(files))
    }

    fn contains(&self, aggregate: &Sha256Digest) -> bool {
        inspect_cached_artifact(&self.root.join(aggregate.as_str()), aggregate, None).is_ok()
    }
}

const CACHE_INDEX_FILE: &str = "artifact-index.json";
const CACHE_BLOBS_DIRECTORY: &str = "blobs";

#[derive(Debug, Serialize, Deserialize)]
struct CacheIndex {
    aggregate: Sha256Digest,
    entries: BTreeMap<String, CachedBlob>,
}

fn cache_index_for(
    aggregate: &Sha256Digest,
    files: &[VerifiedFile],
) -> Result<CacheIndex, ArtifactError> {
    let mut entries = BTreeMap::new();
    for file in files {
        validate_artifact_path(&file.path)?;
        let actual = Sha256Digest::of(&file.bytes);
        if actual != file.digest {
            return Err(ArtifactError::PathHashMismatch {
                path: file.path.to_string(),
                expected: file.digest.clone(),
                actual,
            });
        }
        let entry = CachedBlob {
            digest: file.digest.clone(),
            bytes: file.bytes.len(),
        };
        if entries.insert(file.path.to_string(), entry).is_some() {
            return Err(ArtifactError::DuplicatePath(file.path.to_string()));
        }
    }
    Ok(CacheIndex {
        aggregate: aggregate.clone(),
        entries,
    })
}

fn inspect_cached_artifact(
    destination: &Path,
    aggregate: &Sha256Digest,
    expected_files: Option<&[VerifiedFile]>,
) -> Result<CachedArtifact, ArtifactError> {
    let index_bytes = fs::read(destination.join(CACHE_INDEX_FILE)).map_err(|source| {
        ArtifactError::CorruptCacheIndex {
            aggregate: aggregate.clone(),
            source,
        }
    })?;
    let index: CacheIndex =
        serde_json::from_slice(&index_bytes).map_err(ArtifactError::CacheIndex)?;
    if index.aggregate != *aggregate {
        return Err(ArtifactError::CorruptCache {
            aggregate: aggregate.clone(),
            path: CACHE_INDEX_FILE.to_owned(),
        });
    }

    if let Some(files) = expected_files {
        let expected = cache_index_for(aggregate, files)?;
        if index.entries != expected.entries {
            return Err(ArtifactError::CorruptCache {
                aggregate: aggregate.clone(),
                path: CACHE_INDEX_FILE.to_owned(),
            });
        }
    }

    let mut total_bytes = 0usize;
    for (logical_path, entry) in &index.entries {
        validate_artifact_path(logical_path)?;
        total_bytes =
            total_bytes
                .checked_add(entry.bytes)
                .ok_or_else(|| ArtifactError::CorruptCache {
                    aggregate: aggregate.clone(),
                    path: logical_path.clone(),
                })?;
        let blob = fs::read(
            destination
                .join(CACHE_BLOBS_DIRECTORY)
                .join(entry.digest.as_str()),
        )
        .map_err(|_| ArtifactError::CorruptCache {
            aggregate: aggregate.clone(),
            path: logical_path.clone(),
        })?;
        if blob.len() != entry.bytes || Sha256Digest::of(&blob) != entry.digest {
            return Err(ArtifactError::CorruptCache {
                aggregate: aggregate.clone(),
                path: logical_path.clone(),
            });
        }
    }

    Ok(CachedArtifact {
        aggregate: aggregate.clone(),
        files: index.entries.len(),
        bytes: total_bytes,
        root: destination.to_owned(),
        index: Arc::new(index.entries),
    })
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

enum ReadBoundedError {
    Io(io::Error),
    TooLarge { actual: usize, maximum: usize },
}

fn read_bounded(
    mut reader: Box<dyn Read + Send>,
    maximum: usize,
) -> Result<Vec<u8>, ReadBoundedError> {
    let mut bytes = Vec::with_capacity(maximum.min(64 * 1024));
    let mut limited = reader.by_ref().take((maximum as u64).saturating_add(1));
    limited
        .read_to_end(&mut bytes)
        .map_err(ReadBoundedError::Io)?;
    if bytes.len() > maximum {
        return Err(ReadBoundedError::TooLarge {
            actual: bytes.len(),
            maximum,
        });
    }
    Ok(bytes)
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

/// Exact aggregate algorithm emitted by the pinned
/// `napplet/web@b335c40c77f55547f23af81d6d999e2e4e3a3623` toolchain.
///
/// Each verified `path` tag becomes `"<sha256> <absolute-path>\n"`, lines are
/// sorted bytewise, concatenated as UTF-8, and SHA-256 hashed to lowercase
/// hexadecimal. Only path-tag pairs participate.
#[derive(Debug)]
pub struct Nip5aPathTagsAggregate;

impl AggregateVerifier for Nip5aPathTagsAggregate {
    fn compute(&self, files: &[VerifiedFile]) -> Result<Sha256Digest, ArtifactError> {
        nip5a_path_tags_aggregate(files.iter().map(|file| (file.path.as_ref(), &file.digest)))
    }
}

pub(crate) fn nip5a_path_tags_aggregate<'a>(
    pairs: impl IntoIterator<Item = (&'a str, &'a Sha256Digest)>,
) -> Result<Sha256Digest, ArtifactError> {
    let mut lines = Vec::new();
    for (path, digest) in pairs {
        validate_artifact_path(path)?;
        lines.push(format!("{} {path}\n", digest.as_str()));
    }
    if lines.is_empty() {
        return Err(ArtifactError::AggregatePolicy(
            "at least one path tag is required".to_owned(),
        ));
    }
    lines.sort_unstable();
    let mut hasher = Sha256::new();
    for line in lines {
        hasher.update(line.as_bytes());
    }
    Ok(Sha256Digest(hex::encode(hasher.finalize())))
}

/// Deterministic aggregate used by the test harness only. Production
/// compatibility code must instantiate the policy selected by the lock.
#[derive(Debug)]
pub struct FramedSha256Aggregate;

impl AggregateVerifier for FramedSha256Aggregate {
    fn compute(&self, files: &[VerifiedFile]) -> Result<Sha256Digest, ArtifactError> {
        let mut ordered = files.iter().collect::<Vec<_>>();
        ordered.sort_by(|left, right| left.path.cmp(&right.path));
        let mut hasher = Sha256::new();
        for file in ordered {
            hasher.update((file.path.len() as u64).to_be_bytes());
            hasher.update(file.path.as_bytes());
            hasher.update((file.bytes.len() as u64).to_be_bytes());
            hasher.update(&file.bytes);
        }
        Ok(Sha256Digest(hex::encode(hasher.finalize())))
    }
}

#[derive(Debug, Default)]
pub struct MemoryBlobSource {
    blobs: BTreeMap<String, Arc<[u8]>>,
}

impl MemoryBlobSource {
    pub fn new(blobs: impl IntoIterator<Item = (String, Vec<u8>)>) -> Self {
        Self {
            blobs: blobs
                .into_iter()
                .map(|(path, bytes)| (path, Arc::from(bytes)))
                .collect(),
        }
    }
}

impl BlobSource for MemoryBlobSource {
    fn open(
        &self,
        path: &str,
        _expected: &Sha256Digest,
    ) -> Result<Box<dyn Read + Send>, BlobSourceError> {
        let bytes = self.blobs.get(path).ok_or_else(|| BlobSourceError {
            reason: "blob not found".to_owned(),
        })?;
        Ok(Box::new(io::Cursor::new(Arc::clone(bytes))))
    }
}

#[cfg(test)]
mod tests;
