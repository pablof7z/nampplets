use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    io::Read,
    sync::Arc,
};

use nmp::Event;
use serde::Serialize;
use thiserror::Error;
use url::{Host, Url};

use crate::{
    ArtifactError, ArtifactLimits, ArtifactManifest, ArtifactPath, ArtifactResolver, BlobSource,
    BlobSourceError, CachedArtifact, FileArtifactCache, INDEX_PATH, Nip5aPathTagsAggregate,
    Sha256Digest, nip5a_path_tags_aggregate, validate_artifact_path,
};

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

#[derive(Clone, Debug)]
pub struct ManifestEventVerifier {
    limits: ManifestEventLimits,
}

impl ManifestEventVerifier {
    pub fn new(limits: ManifestEventLimits) -> Result<Self, ManifestError> {
        if limits.maximum_event_bytes == 0
            || limits.maximum_tags == 0
            || limits.maximum_tag_fields == 0
            || limits.maximum_tag_string_bytes == 0
            || limits.maximum_requirements == 0
            || limits.maximum_sources == 0
        {
            return Err(ManifestError::InvalidLimits);
        }
        Ok(Self { limits })
    }

    pub fn pinned() -> Self {
        Self {
            limits: ManifestEventLimits::default(),
        }
    }

    pub fn verify_json(
        &self,
        bytes: &[u8],
        coordinate: &ManifestCoordinate,
    ) -> Result<VerifiedManifest, ManifestError> {
        if bytes.len() > self.limits.maximum_event_bytes {
            return Err(ManifestError::EventTooLarge {
                actual: bytes.len(),
                maximum: self.limits.maximum_event_bytes,
            });
        }
        let event: Event = serde_json::from_slice(bytes).map_err(ManifestError::EventJson)?;
        self.verify_event(&event, coordinate)
    }

    pub fn verify_event(
        &self,
        event: &Event,
        coordinate: &ManifestCoordinate,
    ) -> Result<VerifiedManifest, ManifestError> {
        if !event.verify_id() {
            return Err(ManifestError::InvalidEventId);
        }
        if !event.verify_signature() {
            return Err(ManifestError::InvalidEventSignature);
        }

        let kind = event.kind.as_u16();
        if ![NAPPLET_KIND_SNAPSHOT, NAPPLET_KIND_ROOT, NAPPLET_KIND_NAMED].contains(&kind) {
            return Err(ManifestError::UnsupportedKind(kind));
        }
        if kind != coordinate.expected_kind() {
            return Err(ManifestError::CoordinateKind {
                expected: coordinate.expected_kind(),
                actual: kind,
            });
        }

        let author = event.pubkey.to_hex();
        if author != coordinate.expected_author().as_str() {
            return Err(ManifestError::CoordinateAuthor);
        }
        if let ManifestCoordinate::Snapshot { event_id, .. } = coordinate {
            if event.id.to_hex() != event_id.as_str() {
                return Err(ManifestError::CoordinateEventId);
            }
        }

        if event.tags.len() > self.limits.maximum_tags {
            return Err(ManifestError::TagCount {
                actual: event.tags.len(),
                maximum: self.limits.maximum_tags,
            });
        }

        let mut paths = Vec::new();
        let mut path_names = BTreeSet::new();
        let mut aggregate = None;
        let mut d_tag = None;
        let mut requirements = Vec::new();
        let mut requirement_names = BTreeSet::new();
        let mut servers = Vec::new();
        let mut server_names = BTreeSet::new();
        let mut title = None;
        let mut description = None;
        let mut source = None;

        for tag in event.tags.iter() {
            let fields = tag.as_slice();
            if fields.len() > self.limits.maximum_tag_fields {
                return Err(ManifestError::TagFieldCount {
                    name: fields.first().cloned().unwrap_or_default(),
                    actual: fields.len(),
                    maximum: self.limits.maximum_tag_fields,
                });
            }
            for field in fields {
                if field.len() > self.limits.maximum_tag_string_bytes {
                    return Err(ManifestError::TagStringTooLarge {
                        name: fields.first().cloned().unwrap_or_default(),
                        actual: field.len(),
                        maximum: self.limits.maximum_tag_string_bytes,
                    });
                }
            }

            match fields[0].as_str() {
                "path" => {
                    require_exact_fields(fields, 3)?;
                    validate_artifact_path(&fields[1]).map_err(ManifestError::Artifact)?;
                    if !path_names.insert(fields[1].as_str()) {
                        return Err(ManifestError::DuplicateCriticalTag(format!(
                            "path:{}",
                            fields[1]
                        )));
                    }
                    paths.push(ArtifactPath {
                        path: fields[1].clone(),
                        sha256: Sha256Digest::parse(&fields[2]).map_err(ManifestError::Artifact)?,
                    });
                }
                "x" => {
                    require_exact_fields(fields, 3)?;
                    if fields[2] != "aggregate" || aggregate.is_some() {
                        return Err(ManifestError::DuplicateOrInvalidAggregate);
                    }
                    aggregate =
                        Some(Sha256Digest::parse(&fields[1]).map_err(ManifestError::Artifact)?);
                }
                "d" => {
                    require_exact_fields(fields, 2)?;
                    if d_tag.is_some() {
                        return Err(ManifestError::DuplicateCriticalTag("d".to_owned()));
                    }
                    validate_d_tag(&fields[1], self.limits.maximum_tag_string_bytes)?;
                    d_tag = Some(Arc::<str>::from(fields[1].as_str()));
                }
                "requires" => {
                    require_exact_fields(fields, 2)?;
                    validate_requirement(&fields[1])?;
                    if !requirement_names.insert(fields[1].as_str()) {
                        return Err(ManifestError::DuplicateCriticalTag(format!(
                            "requires:{}",
                            fields[1]
                        )));
                    }
                    requirements.push(Arc::<str>::from(fields[1].as_str()));
                    if requirements.len() > self.limits.maximum_requirements {
                        return Err(ManifestError::RequirementCount {
                            actual: requirements.len(),
                            maximum: self.limits.maximum_requirements,
                        });
                    }
                }
                "server" => {
                    require_exact_fields(fields, 2)?;
                    let normalized = normalize_server(&fields[1])?;
                    if !server_names.insert(normalized.clone()) {
                        return Err(ManifestError::DuplicateCriticalTag(format!(
                            "server:{normalized}"
                        )));
                    }
                    servers.push(Arc::<str>::from(normalized));
                    if servers.len() > self.limits.maximum_sources {
                        return Err(ManifestError::SourceCount {
                            actual: servers.len(),
                            maximum: self.limits.maximum_sources,
                        });
                    }
                }
                "title" => {
                    title = Some(single_metadata("title", fields, title.is_some())?);
                }
                "description" => {
                    description = Some(single_metadata(
                        "description",
                        fields,
                        description.is_some(),
                    )?);
                }
                "source" => {
                    let value = single_metadata("source", fields, source.is_some())?;
                    validate_source_url(&value)?;
                    source = Some(value);
                }
                _ => {}
            }
        }

        let expected_d_tag = coordinate.expected_d_tag();
        match (kind, d_tag.as_deref(), expected_d_tag) {
            (NAPPLET_KIND_NAMED, Some(actual), Some(expected)) if actual == expected => {}
            (NAPPLET_KIND_NAMED, Some(_), Some(_)) => {
                return Err(ManifestError::CoordinateDTag);
            }
            (NAPPLET_KIND_NAMED, None, _) => return Err(ManifestError::MissingDTag),
            (NAPPLET_KIND_ROOT | NAPPLET_KIND_SNAPSHOT, None, None) => {}
            (NAPPLET_KIND_ROOT | NAPPLET_KIND_SNAPSHOT, Some(_), _) => {
                return Err(ManifestError::UnexpectedDTag);
            }
            _ => return Err(ManifestError::CoordinateDTag),
        }

        let aggregate = aggregate.ok_or(ManifestError::MissingAggregate)?;
        let manifest = ArtifactManifest {
            aggregate: aggregate.clone(),
            paths,
        };
        manifest
            .validate(&ArtifactLimits {
                maximum_files: self.limits.maximum_tags,
                ..ArtifactLimits::default()
            })
            .map_err(ManifestError::Artifact)?;
        let recomputed = nip5a_path_tags_aggregate(
            manifest
                .paths
                .iter()
                .map(|path| (path.path.as_str(), &path.sha256)),
        )
        .map_err(ManifestError::Artifact)?;
        if recomputed != aggregate {
            return Err(ManifestError::Artifact(ArtifactError::AggregateMismatch {
                expected: aggregate,
                actual: recomputed,
            }));
        }
        let mode = if manifest.paths.len() == 1 && manifest.paths[0].path == INDEX_PATH {
            ArtifactMode::SingleFile
        } else {
            ArtifactMode::ExternalAssets
        };

        Ok(VerifiedManifest {
            event_id: Sha256Digest::parse(event.id.to_hex()).map_err(ManifestError::Artifact)?,
            author: Sha256Digest::parse(author).map_err(ManifestError::Artifact)?,
            kind,
            d_tag,
            aggregate: manifest.aggregate.clone(),
            artifact: manifest,
            mode,
            requirements: requirements.into(),
            servers: servers.into(),
            title,
            description,
            source,
        })
    }
}

fn require_exact_fields(fields: &[String], expected: usize) -> Result<(), ManifestError> {
    if fields.len() != expected {
        return Err(ManifestError::MalformedCriticalTag {
            name: fields[0].clone(),
            expected,
            actual: fields.len(),
        });
    }
    Ok(())
}

fn single_metadata(
    name: &'static str,
    fields: &[String],
    duplicate: bool,
) -> Result<Arc<str>, ManifestError> {
    require_exact_fields(fields, 2)?;
    if duplicate {
        return Err(ManifestError::DuplicateCriticalTag(name.to_owned()));
    }
    Ok(Arc::from(fields[1].as_str()))
}

fn validate_d_tag(value: &str, maximum: usize) -> Result<(), ManifestError> {
    if value.is_empty()
        || value.len() > maximum
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(ManifestError::InvalidDTag);
    }
    Ok(())
}

fn validate_requirement(value: &str) -> Result<(), ManifestError> {
    let valid_syntax = !value.is_empty()
        && value.len() <= 64
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase() || (index > 0 && (byte.is_ascii_digit() || byte == b'-'))
        });
    if !valid_syntax || value.starts_with("nap:") || value.starts_with("NAP-") {
        return Err(ManifestError::InvalidRequirement(value.to_owned()));
    }
    if !KNOWN_REQUIREMENTS.contains(&value) {
        return Err(ManifestError::UnknownRequirement(value.to_owned()));
    }
    Ok(())
}

fn validate_source_url(value: &str) -> Result<(), ManifestError> {
    let url = Url::parse(value).map_err(|_| ManifestError::InvalidSourceUrl)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ManifestError::InvalidSourceUrl);
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct VerifiedManifest {
    event_id: Sha256Digest,
    author: Sha256Digest,
    kind: u16,
    d_tag: Option<Arc<str>>,
    aggregate: Sha256Digest,
    artifact: ArtifactManifest,
    mode: ArtifactMode,
    requirements: Arc<[Arc<str>]>,
    servers: Arc<[Arc<str>]>,
    title: Option<Arc<str>>,
    description: Option<Arc<str>>,
    source: Option<Arc<str>>,
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

    fn approved_servers(
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

fn normalize_server(value: &str) -> Result<String, ManifestError> {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlobFetchRequest {
    logical_path: Arc<str>,
    digest: Sha256Digest,
    candidates: Arc<[Arc<str>]>,
    maximum_bytes: usize,
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
    status: u16,
    redirect_location: Option<String>,
    body: Box<dyn Read + Send>,
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
    fn new(manifest: VerifiedManifest, cached: CachedArtifact) -> Result<Self, ArtifactError> {
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

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("manifest verifier limits must be finite and non-zero")]
    InvalidLimits,
    #[error("manifest event is {actual} bytes; the maximum is {maximum}")]
    EventTooLarge { actual: usize, maximum: usize },
    #[error("manifest event is not valid Nostr event JSON: {0}")]
    EventJson(#[source] serde_json::Error),
    #[error("manifest event id does not match its canonical NIP-01 serialization")]
    InvalidEventId,
    #[error("manifest event Schnorr signature is invalid")]
    InvalidEventSignature,
    #[error("event kind {0} is not a pinned NIP-5D manifest kind")]
    UnsupportedKind(u16),
    #[error(
        "resolved event kind differs from the requested coordinate: expected {expected}, got {actual}"
    )]
    CoordinateKind { expected: u16, actual: u16 },
    #[error("resolved event author differs from the requested coordinate")]
    CoordinateAuthor,
    #[error("resolved snapshot event id differs from the requested coordinate")]
    CoordinateEventId,
    #[error("resolved named-manifest d tag differs from the requested coordinate")]
    CoordinateDTag,
    #[error("named manifest is missing its d tag")]
    MissingDTag,
    #[error("root or snapshot manifest contains an unexpected d tag")]
    UnexpectedDTag,
    #[error("manifest d tag is empty, normalized, over-limit, or contains control characters")]
    InvalidDTag,
    #[error("manifest has {actual} tags; the maximum is {maximum}")]
    TagCount { actual: usize, maximum: usize },
    #[error("manifest tag {name:?} has {actual} fields; the maximum is {maximum}")]
    TagFieldCount {
        name: String,
        actual: usize,
        maximum: usize,
    },
    #[error("manifest tag {name:?} contains a {actual}-byte field; the maximum is {maximum}")]
    TagStringTooLarge {
        name: String,
        actual: usize,
        maximum: usize,
    },
    #[error("critical manifest tag {name:?} must have {expected} fields, not {actual}")]
    MalformedCriticalTag {
        name: String,
        expected: usize,
        actual: usize,
    },
    #[error("duplicate or ambiguous critical manifest tag {0}")]
    DuplicateCriticalTag(String),
    #[error("manifest must contain exactly one [\"x\", hash, \"aggregate\"] tag")]
    DuplicateOrInvalidAggregate,
    #[error("manifest has no aggregate x tag")]
    MissingAggregate,
    #[error("invalid requires domain {0:?}")]
    InvalidRequirement(String),
    #[error("requires domain {0:?} is outside the pinned compatibility inventory")]
    UnknownRequirement(String),
    #[error("manifest declares {actual} requirements; the maximum is {maximum}")]
    RequirementCount { actual: usize, maximum: usize },
    #[error("manifest declares {actual} blob sources; the maximum is {maximum}")]
    SourceCount { actual: usize, maximum: usize },
    #[error("manifest source metadata is not an absolute credential-free HTTP(S) URL")]
    InvalidSourceUrl,
    #[error("blob server URL violates source policy")]
    InvalidBlobServer,
    #[error("no policy-approved blob source is available")]
    NoApprovedBlobSource,
    #[error(transparent)]
    Artifact(#[from] ArtifactError),
}

#[cfg(test)]
mod tests;
