use std::sync::Arc;

use serde::Serialize;

use crate::Sha256Digest;

mod blob;
mod error;
mod index;
mod policy;
mod resolver;
mod verified;
mod verifier;

#[cfg(test)]
mod tests;

pub use blob::{BlobFetchRequest, BlobFetchResponse, ManifestBlobSource};
pub use error::ManifestError;
pub use index::{VerifiedArtifactHandle, VerifiedArtifactIndex, VerifiedArtifactIndexEntry};
pub use policy::ArtifactSourcePolicy;
pub use resolver::SignedArtifactResolver;
pub use verified::VerifiedManifest;
pub use verifier::ManifestEventVerifier;

use verifier::validate_d_tag;

const NAPPLET_KIND_SNAPSHOT: u16 = 5_129;
const NAPPLET_KIND_ROOT: u16 = 15_129;
const NAPPLET_KIND_NAMED: u16 = 35_129;
const KNOWN_REQUIREMENTS: &[&str] = &[
    "relay", "identity", "storage", "inc", "theme", "keys", "media", "notify", "config",
    "resource", "cvm", "outbox", "upload", "intent", "ble", "webrtc", "link", "count", "lists",
    "serial", "common", "dm",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactMode {
    SingleFile,
    ExternalAssets,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestCoordinate {
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

impl ManifestCoordinate {
    pub fn snapshot(event_id: &str, author: &str) -> Result<Self, ManifestError> {
        Ok(Self::Snapshot {
            event_id: Sha256Digest::parse(event_id).map_err(ManifestError::Artifact)?,
            author: Sha256Digest::parse(author).map_err(ManifestError::Artifact)?,
        })
    }

    pub fn root(author: &str) -> Result<Self, ManifestError> {
        Ok(Self::Root {
            author: Sha256Digest::parse(author).map_err(ManifestError::Artifact)?,
        })
    }

    pub fn named(author: &str, d_tag: &str) -> Result<Self, ManifestError> {
        validate_d_tag(d_tag, 4_096)?;
        Ok(Self::Named {
            author: Sha256Digest::parse(author).map_err(ManifestError::Artifact)?,
            d_tag: Arc::from(d_tag),
        })
    }

    fn expected_kind(&self) -> u16 {
        match self {
            Self::Snapshot { .. } => NAPPLET_KIND_SNAPSHOT,
            Self::Root { .. } => NAPPLET_KIND_ROOT,
            Self::Named { .. } => NAPPLET_KIND_NAMED,
        }
    }

    fn expected_author(&self) -> &Sha256Digest {
        match self {
            Self::Snapshot { author, .. } | Self::Root { author } | Self::Named { author, .. } => {
                author
            }
        }
    }

    fn expected_d_tag(&self) -> Option<&str> {
        match self {
            Self::Named { d_tag, .. } => Some(d_tag),
            Self::Snapshot { .. } | Self::Root { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManifestEventLimits {
    pub maximum_event_bytes: usize,
    pub maximum_tags: usize,
    pub maximum_tag_fields: usize,
    pub maximum_tag_string_bytes: usize,
    pub maximum_requirements: usize,
    pub maximum_sources: usize,
}

impl Default for ManifestEventLimits {
    fn default() -> Self {
        Self {
            maximum_event_bytes: 256 * 1_024,
            maximum_tags: 1_024,
            maximum_tag_fields: 64,
            maximum_tag_string_bytes: 16 * 1_024,
            maximum_requirements: KNOWN_REQUIREMENTS.len(),
            maximum_sources: 32,
        }
    }
}
