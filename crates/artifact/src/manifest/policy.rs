use std::{collections::BTreeSet, sync::Arc};

use url::{Host, Url};

use super::{ManifestError, verified::VerifiedManifest};

#[derive(Clone, Debug)]
pub struct ArtifactSourcePolicy {
    accept_manifest_https: bool,
    accept_manifest_loopback_http: bool,
    configured_servers: Arc<[Arc<str>]>,
    allowed_origins: Arc<BTreeSet<String>>,
    maximum_sources: usize,
}

impl ArtifactSourcePolicy {
    pub fn manifest_https_only(maximum_sources: usize) -> Result<Self, ManifestError> {
        Self::new(
            true,
            false,
            std::iter::empty::<&str>(),
            std::iter::empty::<&str>(),
            maximum_sources,
        )
    }

    pub fn allowlisted<I, S>(
        allowed_origins: I,
        configured_servers: I,
        maximum_sources: usize,
    ) -> Result<Self, ManifestError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::new(
            false,
            false,
            allowed_origins,
            configured_servers,
            maximum_sources,
        )
    }

    pub fn new<OI, OS, CI, CS>(
        accept_manifest_https: bool,
        accept_manifest_loopback_http: bool,
        allowed_origins: OI,
        configured_servers: CI,
        maximum_sources: usize,
    ) -> Result<Self, ManifestError>
    where
        OI: IntoIterator<Item = OS>,
        OS: AsRef<str>,
        CI: IntoIterator<Item = CS>,
        CS: AsRef<str>,
    {
        if maximum_sources == 0 {
            return Err(ManifestError::InvalidLimits);
        }
        let mut origins = BTreeSet::new();
        for origin in allowed_origins {
            origins.insert(normalize_origin(origin.as_ref())?);
        }
        let mut configured = Vec::new();
        let mut seen = BTreeSet::new();
        for server in configured_servers {
            let normalized = normalize_server(server.as_ref())?;
            if seen.insert(normalized.clone()) {
                configured.push(Arc::<str>::from(normalized));
            }
        }
        if configured.len() > maximum_sources {
            return Err(ManifestError::SourceCount {
                actual: configured.len(),
                maximum: maximum_sources,
            });
        }
        Ok(Self {
            accept_manifest_https,
            accept_manifest_loopback_http,
            configured_servers: configured.into(),
            allowed_origins: Arc::new(origins),
            maximum_sources,
        })
    }

    pub(super) fn approved_servers(
        &self,
        manifest: &VerifiedManifest,
    ) -> Result<Vec<Arc<str>>, ManifestError> {
        let mut approved = Vec::new();
        let mut seen = BTreeSet::new();
        for configured in self.configured_servers.iter() {
            if seen.insert(configured.to_string()) {
                approved.push(Arc::clone(configured));
            }
        }
        for hint in manifest.servers.iter() {
            let url = Url::parse(hint).map_err(|_| ManifestError::InvalidBlobServer)?;
            let origin = normalized_origin(&url)?;
            let allowed = self.allowed_origins.contains(&origin)
                || (self.accept_manifest_https && url.scheme() == "https")
                || (self.accept_manifest_loopback_http
                    && url.scheme() == "http"
                    && is_loopback_host(&url));
            if allowed && seen.insert(hint.to_string()) {
                approved.push(Arc::clone(hint));
            }
        }
        if approved.len() > self.maximum_sources {
            return Err(ManifestError::SourceCount {
                actual: approved.len(),
                maximum: self.maximum_sources,
            });
        }
        if approved.is_empty() {
            return Err(ManifestError::NoApprovedBlobSource);
        }
        Ok(approved)
    }
}

fn normalize_origin(value: &str) -> Result<String, ManifestError> {
    let url = Url::parse(value).map_err(|_| ManifestError::InvalidBlobServer)?;
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(ManifestError::InvalidBlobServer);
    }
    normalized_origin(&url)
}

fn normalized_origin(url: &Url) -> Result<String, ManifestError> {
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ManifestError::InvalidBlobServer);
    }
    Ok(url.origin().ascii_serialization())
}

pub(super) fn normalize_server(value: &str) -> Result<String, ManifestError> {
    let mut url = Url::parse(value).map_err(|_| ManifestError::InvalidBlobServer)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ManifestError::InvalidBlobServer);
    }
    if !url.path().ends_with('/') {
        let mut path = url.path().to_owned();
        path.push('/');
        url.set_path(&path);
    }
    Ok(url.to_string())
}

fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}
