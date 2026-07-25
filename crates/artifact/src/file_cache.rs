use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{
    ArtifactCache, ArtifactError, Sha256Digest, VerifiedFile,
    resolver::{CachedArtifact, CachedBlob},
    validate_artifact_path,
};

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

pub(crate) const CACHE_INDEX_FILE: &str = "artifact-index.json";
pub(crate) const CACHE_BLOBS_DIRECTORY: &str = "blobs";

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

pub(crate) enum ReadBoundedError {
    Io(io::Error),
    TooLarge { actual: usize, maximum: usize },
}

pub(crate) fn read_bounded(
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
