use std::sync::Arc;

use super::ArtifactMode;
use crate::{ArtifactManifest, Sha256Digest};

#[derive(Clone, Debug)]
pub struct VerifiedManifest {
    pub(super) event_id: Sha256Digest,
    pub(super) author: Sha256Digest,
    pub(super) kind: u16,
    pub(super) d_tag: Option<Arc<str>>,
    pub(super) aggregate: Sha256Digest,
    pub(super) artifact: ArtifactManifest,
    pub(super) mode: ArtifactMode,
    pub(super) requirements: Arc<[Arc<str>]>,
    pub(super) servers: Arc<[Arc<str>]>,
    pub(super) title: Option<Arc<str>>,
    pub(super) description: Option<Arc<str>>,
    pub(super) source: Option<Arc<str>>,
}

impl VerifiedManifest {
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

    pub fn requirements(&self) -> impl ExactSizeIterator<Item = &str> {
        self.requirements.iter().map(AsRef::as_ref)
    }

    pub fn servers(&self) -> impl ExactSizeIterator<Item = &str> {
        self.servers.iter().map(AsRef::as_ref)
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}
