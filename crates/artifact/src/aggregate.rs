use std::{collections::BTreeMap, io, sync::Arc};

use sha2::{Digest as _, Sha256};

use crate::{
    AggregateVerifier, ArtifactError, BlobSource, BlobSourceError, Sha256Digest, VerifiedFile,
    validate_artifact_path,
};

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
    ) -> Result<Box<dyn io::Read + Send>, BlobSourceError> {
        let bytes = self.blobs.get(path).ok_or_else(|| BlobSourceError {
            reason: "blob not found".to_owned(),
        })?;
        Ok(Box::new(io::Cursor::new(Arc::clone(bytes))))
    }
}
