use std::sync::Arc;

use serde::Serialize;

use super::{ArtifactMode, verified::VerifiedManifest};
use crate::{ArtifactError, CachedArtifact, Sha256Digest};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VerifiedArtifactIndexEntry {
    path: Arc<str>,
    sha256: Sha256Digest,
    bytes: usize,
}

impl VerifiedArtifactIndexEntry {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn sha256(&self) -> &Sha256Digest {
        &self.sha256
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VerifiedArtifactIndex {
    event_id: Sha256Digest,
    author: Sha256Digest,
    kind: u16,
    d_tag: Option<Arc<str>>,
    aggregate: Sha256Digest,
    mode: ArtifactMode,
    entries: Arc<[VerifiedArtifactIndexEntry]>,
}

impl VerifiedArtifactIndex {
    pub fn event_id(&self) -> &Sha256Digest {
        &self.event_id
    }

    pub fn author(&self) -> &Sha256Digest {
        &self.author
    }

    pub fn kind(&self) -> u16 {
        self.kind
    }

    pub fn d_tag(&self) -> Option<&str> {
        self.d_tag.as_deref()
    }

    pub fn aggregate(&self) -> &Sha256Digest {
        &self.aggregate
    }

    pub fn mode(&self) -> ArtifactMode {
        self.mode
    }

    pub fn entries(&self) -> impl ExactSizeIterator<Item = &VerifiedArtifactIndexEntry> {
        self.entries.iter()
    }
}

#[derive(Clone, Debug)]
pub struct VerifiedArtifactHandle {
    manifest: VerifiedManifest,
    cached: CachedArtifact,
    index: VerifiedArtifactIndex,
}

impl VerifiedArtifactHandle {
    pub(super) fn new(
        manifest: VerifiedManifest,
        cached: CachedArtifact,
    ) -> Result<Self, ArtifactError> {
        let mut entries = Vec::with_capacity(manifest.artifact.paths.len());
        for path in &manifest.artifact.paths {
            let bytes = cached
                .index
                .get(&path.path)
                .ok_or_else(|| ArtifactError::MissingCachedPath(path.path.clone()))?
                .bytes;
            entries.push(VerifiedArtifactIndexEntry {
                path: Arc::from(path.path.as_str()),
                sha256: path.sha256.clone(),
                bytes,
            });
        }
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        let index = VerifiedArtifactIndex {
            event_id: manifest.event_id.clone(),
            author: manifest.author.clone(),
            kind: manifest.kind,
            d_tag: manifest.d_tag.clone(),
            aggregate: manifest.aggregate.clone(),
            mode: manifest.mode,
            entries: entries.into(),
        };
        Ok(Self {
            manifest,
            cached,
            index,
        })
    }

    pub fn manifest(&self) -> &VerifiedManifest {
        &self.manifest
    }

    pub fn index(&self) -> &VerifiedArtifactIndex {
        &self.index
    }

    pub fn read_verified(
        &self,
        logical_path: &str,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, ArtifactError> {
        self.cached.read_verified(logical_path, maximum_bytes)
    }
}
