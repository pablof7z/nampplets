use std::{collections::BTreeMap, io::Read, sync::Arc};

use url::Url;

use super::{
    ManifestCoordinate, ManifestError,
    blob::{BlobFetchRequest, ManifestBlobSource},
    index::VerifiedArtifactHandle,
    policy::ArtifactSourcePolicy,
    verified::VerifiedManifest,
    verifier::ManifestEventVerifier,
};
use crate::{
    ArtifactError, ArtifactLimits, ArtifactManifest, ArtifactResolver, BlobSource, BlobSourceError,
    FileArtifactCache, Nip5aPathTagsAggregate, Sha256Digest,
};

#[derive(Debug)]
pub struct SignedArtifactResolver<'a> {
    event_verifier: ManifestEventVerifier,
    artifact_limits: ArtifactLimits,
    source_policy: ArtifactSourcePolicy,
    source: &'a dyn ManifestBlobSource,
    cache: &'a FileArtifactCache,
}

impl<'a> SignedArtifactResolver<'a> {
    pub fn new(
        event_verifier: ManifestEventVerifier,
        artifact_limits: ArtifactLimits,
        source_policy: ArtifactSourcePolicy,
        source: &'a dyn ManifestBlobSource,
        cache: &'a FileArtifactCache,
    ) -> Result<Self, ManifestError> {
        if artifact_limits.maximum_files == 0
            || artifact_limits.maximum_file_bytes == 0
            || artifact_limits.maximum_total_bytes == 0
        {
            return Err(ManifestError::Artifact(ArtifactError::InvalidLimits));
        }
        Ok(Self {
            event_verifier,
            artifact_limits,
            source_policy,
            source,
            cache,
        })
    }

    pub fn resolve_json(
        &self,
        event_json: &[u8],
        coordinate: &ManifestCoordinate,
    ) -> Result<VerifiedArtifactHandle, ManifestError> {
        let manifest = self.event_verifier.verify_json(event_json, coordinate)?;
        self.resolve_verified(manifest)
    }

    pub fn resolve_verified(
        &self,
        manifest: VerifiedManifest,
    ) -> Result<VerifiedArtifactHandle, ManifestError> {
        manifest
            .artifact
            .validate(&self.artifact_limits)
            .map_err(ManifestError::Artifact)?;
        let servers = self.source_policy.approved_servers(&manifest)?;
        let source = PolicyCheckedBlobSource::new(
            self.source,
            &manifest.artifact,
            &servers,
            self.artifact_limits.maximum_file_bytes,
        )?;
        let aggregate = Nip5aPathTagsAggregate;
        let resolver = ArtifactResolver::new(self.artifact_limits, &source, &aggregate, self.cache)
            .map_err(ManifestError::Artifact)?;
        let cached = resolver
            .resolve(&manifest.artifact)
            .map_err(ManifestError::Artifact)?;
        VerifiedArtifactHandle::new(manifest, cached).map_err(ManifestError::Artifact)
    }
}

#[derive(Debug)]
struct PolicyCheckedBlobSource<'a> {
    source: &'a dyn ManifestBlobSource,
    requests: BTreeMap<String, BlobFetchRequest>,
}

impl<'a> PolicyCheckedBlobSource<'a> {
    fn new(
        source: &'a dyn ManifestBlobSource,
        manifest: &ArtifactManifest,
        servers: &[Arc<str>],
        maximum_bytes: usize,
    ) -> Result<Self, ManifestError> {
        let mut requests = BTreeMap::new();
        for path in &manifest.paths {
            let mut candidates = Vec::with_capacity(servers.len());
            for server in servers {
                let base = Url::parse(server).map_err(|_| ManifestError::InvalidBlobServer)?;
                let url = base
                    .join(path.sha256.as_str())
                    .map_err(|_| ManifestError::InvalidBlobServer)?;
                candidates.push(Arc::<str>::from(url.to_string()));
            }
            requests.insert(
                path.path.clone(),
                BlobFetchRequest {
                    logical_path: Arc::from(path.path.as_str()),
                    digest: path.sha256.clone(),
                    candidates: candidates.into(),
                    maximum_bytes,
                },
            );
        }
        Ok(Self { source, requests })
    }
}

impl BlobSource for PolicyCheckedBlobSource<'_> {
    fn open(
        &self,
        path: &str,
        expected: &Sha256Digest,
    ) -> Result<Box<dyn Read + Send>, BlobSourceError> {
        let request = self.requests.get(path).ok_or_else(|| BlobSourceError {
            reason: "path is absent from the verified manifest".to_owned(),
        })?;
        if request.digest != *expected {
            return Err(BlobSourceError {
                reason: "fetch request digest differs from the verified manifest".to_owned(),
            });
        }
        // The blob source (`SafeManifestBlobSource` for the Rust HTTPS
        // transport) owns provenance: it is the layer that actually resolves
        // DNS, connects, and follows any redirect, and it revalidates every
        // hop against the HTTPS-only / credential-free / public-address
        // policy before this call ever sees a response. This boundary does
        // not re-pin `source_url` to the original candidate list, because
        // content correctness is independent of which approved-policy URL
        // produced the bytes: every file is hash-verified against the
        // manifest-pinned digest below, and the full artifact is
        // aggregate-verified afterward.
        let response = self.source.fetch(request)?;
        if response.redirect_location.is_some() || (300..400).contains(&response.status) {
            return Err(BlobSourceError {
                reason: "redirect refused by artifact source policy".to_owned(),
            });
        }
        if response.status != 200 {
            return Err(BlobSourceError {
                reason: format!("blob source returned HTTP {}", response.status),
            });
        }
        Ok(response.body)
    }
}
