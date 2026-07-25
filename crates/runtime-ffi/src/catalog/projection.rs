//! Projections from NMP catalog facts onto the bounded screen records.

use std::{sync::Arc, time::Duration};

use nmp::WindowLoad;
use nmp_native_artifact::ManifestCoordinate;
use nmp_native_catalog_resolver::{CoordinateLookupFact, CoordinateLookupState, ResolveError};
use nmp_native_nmp_adapter::catalog::{
    CatalogAccessContext, CatalogBrowseFrame, CatalogManifestCandidate, CatalogShortfall,
    CatalogSourceEvidence, CatalogSourceStatus, ManifestCatalogError,
};
use nmp_native_runtime_core::CapabilityRequirement;

use super::types::{
    RuntimeCatalogCapability, RuntimeCatalogEntry, RuntimeCatalogError, RuntimeCatalogFailure,
    RuntimeCatalogLookupState, RuntimeCatalogPage, RuntimeCatalogProvenance,
    RuntimeCatalogShortfall, RuntimeCatalogSource, RuntimeCatalogSourceAccess,
    RuntimeCatalogSourceState, RuntimeCatalogWindowState,
};
use crate::{
    GOOD_MORNING_AGGREGATE_HASH, GOOD_MORNING_AUTHOR, GOOD_MORNING_CAPABILITY_PROFILE,
    GOOD_MORNING_D_TAG, RuntimePermissionRequirement,
};

pub(super) fn candidate_coordinate(
    candidate: &CatalogManifestCandidate,
) -> Option<ManifestCoordinate> {
    let author = nmp_native_artifact::Sha256Digest::parse(candidate.author.to_string()).ok()?;
    match candidate.kind {
        5_129 => Some(ManifestCoordinate::Snapshot {
            event_id: nmp_native_artifact::Sha256Digest::parse(candidate.event_id.to_string())
                .ok()?,
            author,
        }),
        15_129 => Some(ManifestCoordinate::Root { author }),
        35_129 => Some(ManifestCoordinate::Named {
            author,
            d_tag: Arc::clone(candidate.d_tag.as_ref()?),
        }),
        _ => None,
    }
}

pub(super) fn project_page(
    frame: &CatalogBrowseFrame,
    query_was_local_filter: bool,
) -> RuntimeCatalogPage {
    RuntimeCatalogPage {
        entries: frame.candidates.iter().map(project_entry).collect(),
        query_was_local_filter,
        locally_filtered_rows: usize_to_u64(frame.locally_filtered_rows),
        projection_limited_rows: usize_to_u64(frame.projection_limit_rows),
        refused_rows: usize_to_u64(frame.refused.len()),
        has_more: frame.projection_limit_rows > 0,
        window: match frame.window_load {
            WindowLoad::Idle => RuntimeCatalogWindowState::Idle,
            WindowLoad::Requesting => RuntimeCatalogWindowState::Requesting,
            WindowLoad::Returned { added } => RuntimeCatalogWindowState::Returned {
                added: usize_to_u64(added),
            },
            WindowLoad::AtBound { max } => RuntimeCatalogWindowState::AtBound {
                maximum: usize_to_u64(max),
            },
            _ => RuntimeCatalogWindowState::Unknown,
        },
        sources: frame.source_evidence.iter().map(project_source).collect(),
        shortfalls: frame
            .shortfalls
            .iter()
            .map(|shortfall| match shortfall {
                CatalogShortfall::NoPlannedSource => RuntimeCatalogShortfall::NoPlannedSource,
                CatalogShortfall::NoResolvedDemand => RuntimeCatalogShortfall::NoResolvedDemand,
                CatalogShortfall::LocalLimit => RuntimeCatalogShortfall::LocalLimit,
            })
            .collect(),
    }
}

pub(super) fn project_entry(candidate: &CatalogManifestCandidate) -> RuntimeCatalogEntry {
    RuntimeCatalogEntry {
        event_id: candidate.event_id.to_string(),
        coordinate: candidate_coordinate(candidate)
            .as_ref()
            .map(catalog_coordinate_string),
        manifest_author: candidate.author.to_string(),
        kind: candidate.kind,
        created_at: candidate.created_at,
        d_tag: candidate.d_tag.as_deref().map(str::to_owned),
        title: candidate.title.as_deref().map(str::to_owned),
        description: candidate.description.as_deref().map(str::to_owned),
        aggregate_hash: candidate.aggregate.as_deref().map(str::to_owned),
        observed_sources: candidate
            .observed_sources
            .iter()
            .map(ToString::to_string)
            .collect(),
    }
}

pub(super) fn project_source(source: &CatalogSourceEvidence) -> RuntimeCatalogSource {
    RuntimeCatalogSource {
        relay: source.relay.to_string(),
        access: match &source.access {
            CatalogAccessContext::Public => RuntimeCatalogSourceAccess::Public,
            CatalogAccessContext::Nip42 { public_key } => RuntimeCatalogSourceAccess::Nip42 {
                public_key: public_key.to_string(),
            },
        },
        reconciled_through: source.reconciled_through,
        state: match source.status {
            CatalogSourceStatus::Requesting => RuntimeCatalogSourceState::Requesting,
            CatalogSourceStatus::Connecting => RuntimeCatalogSourceState::Connecting,
            CatalogSourceStatus::Disconnected => RuntimeCatalogSourceState::Disconnected,
            CatalogSourceStatus::AwaitingAuth => RuntimeCatalogSourceState::AwaitingAuth,
            CatalogSourceStatus::AuthDenied => RuntimeCatalogSourceState::AuthDenied,
            CatalogSourceStatus::Error => RuntimeCatalogSourceState::Error,
        },
    }
}

pub(super) fn review_capabilities(
    summary: &nmp_native_catalog_resolver::ArtifactReviewSummary,
) -> Vec<RuntimeCatalogCapability> {
    let (author, d_tag) = coordinate_identity(summary.coordinate());
    if author == GOOD_MORNING_AUTHOR
        && d_tag.as_deref() == Some(GOOD_MORNING_D_TAG)
        && summary.aggregate().as_str() == GOOD_MORNING_AGGREGATE_HASH
    {
        return GOOD_MORNING_CAPABILITY_PROFILE
            .iter()
            .map(|(domain, requirement)| RuntimeCatalogCapability {
                domain: (*domain).to_owned(),
                requirement: match requirement {
                    CapabilityRequirement::Required => RuntimePermissionRequirement::Required,
                    CapabilityRequirement::Optional => RuntimePermissionRequirement::Optional,
                },
            })
            .collect();
    }
    summary
        .requirements()
        .map(|domain| RuntimeCatalogCapability {
            domain: domain.to_owned(),
            requirement: RuntimePermissionRequirement::Required,
        })
        .collect()
}

pub(super) fn coordinate_identity(coordinate: &ManifestCoordinate) -> (String, Option<String>) {
    match coordinate {
        ManifestCoordinate::Snapshot { author, .. } | ManifestCoordinate::Root { author } => {
            (author.as_str().to_owned(), None)
        }
        ManifestCoordinate::Named { author, d_tag } => {
            (author.as_str().to_owned(), Some(d_tag.to_string()))
        }
    }
}

pub(super) fn catalog_coordinate_string(coordinate: &ManifestCoordinate) -> String {
    match coordinate {
        ManifestCoordinate::Snapshot { event_id, author } => {
            format!("5129:{}:{}", event_id.as_str(), author.as_str())
        }
        ManifestCoordinate::Root { author } => format!("15129:{}", author.as_str()),
        ManifestCoordinate::Named { author, d_tag } => {
            format!("35129:{}:{d_tag}", author.as_str())
        }
    }
}

pub(super) fn project_lookup_facts(
    facts: &[CoordinateLookupFact],
) -> Vec<RuntimeCatalogProvenance> {
    facts
        .iter()
        .map(|fact| RuntimeCatalogProvenance {
            source: fact.source().to_owned(),
            state: match fact.state() {
                CoordinateLookupState::Observed { rows } => RuntimeCatalogLookupState::Observed {
                    rows: usize_to_u64(*rows),
                },
                CoordinateLookupState::Shortfall { reason } => {
                    RuntimeCatalogLookupState::Shortfall {
                        reason: reason.to_string(),
                    }
                }
                CoordinateLookupState::Selected { event_id } => {
                    RuntimeCatalogLookupState::Selected {
                        event_id: event_id.to_string(),
                    }
                }
            },
        })
        .collect()
}

pub(super) fn map_browse_error(error: ManifestCatalogError) -> RuntimeCatalogError {
    match error {
        ManifestCatalogError::BrowseCapacity { maximum }
        | ManifestCatalogError::LookupCapacity { maximum } => RuntimeCatalogError::Busy {
            maximum: maximum as u64,
        },
        other => RuntimeCatalogError::Browse {
            reason: other.to_string(),
        },
    }
}

pub(super) fn map_resolve_error(error: ResolveError) -> RuntimeCatalogError {
    match error {
        ResolveError::Cancelled => RuntimeCatalogError::Cancelled,
        ResolveError::Saturated { maximum } => RuntimeCatalogError::Busy {
            maximum: maximum as u64,
        },
        ResolveError::ReviewSaturated { maximum } => RuntimeCatalogError::ReviewCapacity {
            maximum: maximum as u64,
        },
        ResolveError::ReviewStale | ResolveError::ReviewForeign => RuntimeCatalogError::StaleReview,
        ResolveError::NotFound { facts } => RuntimeCatalogError::NotFound {
            provenance: project_lookup_facts(&facts),
        },
        other => RuntimeCatalogError::Resolve {
            reason: other.to_string(),
        },
    }
}

pub fn project_catalog_error(error: RuntimeCatalogError) -> RuntimeCatalogFailure {
    let provenance = match &error {
        RuntimeCatalogError::NotFound { provenance } => provenance.clone(),
        _ => Vec::new(),
    };
    let code = match &error {
        RuntimeCatalogError::InvalidConfiguration { .. } => "invalid-configuration",
        RuntimeCatalogError::Busy { .. } => "busy",
        RuntimeCatalogError::Deadline { .. } => "deadline",
        RuntimeCatalogError::WorkerUnavailable { .. } => "worker-unavailable",
        RuntimeCatalogError::Browse { .. } => "browse-refused",
        RuntimeCatalogError::InvalidCoordinate { .. } => "invalid-coordinate",
        RuntimeCatalogError::NotFound { .. } => "not-found",
        RuntimeCatalogError::ReviewCapacity { .. } => "review-capacity",
        RuntimeCatalogError::StaleReview => "stale-review",
        RuntimeCatalogError::Cancelled => "cancelled",
        RuntimeCatalogError::Resolve { .. } => "resolve-refused",
    };
    RuntimeCatalogFailure {
        code: code.to_owned(),
        detail: error.to_string(),
        provenance,
    }
}

pub(super) fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

pub(super) fn usize_to_u64(value: usize) -> u64 {
    value.try_into().unwrap_or(u64::MAX)
}
