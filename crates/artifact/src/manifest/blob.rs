use std::{fmt, io::Read, sync::Arc};

use crate::{BlobSourceError, Sha256Digest};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlobFetchRequest {
    pub(super) logical_path: Arc<str>,
    pub(super) digest: Sha256Digest,
    pub(super) candidates: Arc<[Arc<str>]>,
    pub(super) maximum_bytes: usize,
}

impl BlobFetchRequest {
    pub fn logical_path(&self) -> &str {
        &self.logical_path
    }

    pub fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub fn candidate_urls(&self) -> impl ExactSizeIterator<Item = &str> {
        self.candidates.iter().map(AsRef::as_ref)
    }

    pub fn maximum_bytes(&self) -> usize {
        self.maximum_bytes
    }
}

pub struct BlobFetchResponse {
    source_url: String,
    pub(super) status: u16,
    pub(super) redirect_location: Option<String>,
    pub(super) body: Box<dyn Read + Send>,
}

impl fmt::Debug for BlobFetchResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BlobFetchResponse")
            .field("source_url", &self.source_url)
            .field("status", &self.status)
            .field("redirect_location", &self.redirect_location)
            .finish_non_exhaustive()
    }
}

impl BlobFetchResponse {
    pub fn ok(source_url: impl Into<String>, body: Box<dyn Read + Send>) -> Self {
        Self {
            source_url: source_url.into(),
            status: 200,
            redirect_location: None,
            body,
        }
    }

    pub fn status(source_url: impl Into<String>, status: u16, body: Box<dyn Read + Send>) -> Self {
        Self {
            source_url: source_url.into(),
            status,
            redirect_location: None,
            body,
        }
    }

    pub fn redirect(
        source_url: impl Into<String>,
        status: u16,
        location: impl Into<String>,
    ) -> Self {
        Self {
            source_url: source_url.into(),
            status,
            redirect_location: Some(location.into()),
            body: Box::new(std::io::empty()),
        }
    }
}

pub trait ManifestBlobSource: Send + Sync + fmt::Debug {
    fn fetch(&self, request: &BlobFetchRequest) -> Result<BlobFetchResponse, BlobSourceError>;
}
