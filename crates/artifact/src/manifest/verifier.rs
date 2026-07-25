use std::{collections::BTreeSet, sync::Arc};

use nmp::Event;
use url::Url;

use super::{
    ArtifactMode, KNOWN_REQUIREMENTS, ManifestCoordinate, ManifestError, ManifestEventLimits,
    NAPPLET_KIND_NAMED, NAPPLET_KIND_ROOT, NAPPLET_KIND_SNAPSHOT, policy::normalize_server,
    verified::VerifiedManifest,
};
use crate::{
    ArtifactError, ArtifactLimits, ArtifactManifest, ArtifactPath, INDEX_PATH, Sha256Digest,
    nip5a_path_tags_aggregate, validate_artifact_path,
};

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

pub(super) fn validate_d_tag(value: &str, maximum: usize) -> Result<(), ManifestError> {
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
