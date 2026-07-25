//! Private controller helpers: capability derivation and bounded refusals.

use nmp_native_runtime_core::{Capability, CapabilityRequest, CapabilityRequirement, Principal};
use nmp_native_runtime_store::InstalledBuild;

use super::RuntimeController;
use crate::{
    GOOD_MORNING_AGGREGATE_HASH, GOOD_MORNING_AUTHOR, GOOD_MORNING_CAPABILITY_PROFILE,
    GOOD_MORNING_D_TAG, MAXIMUM_PERMISSION_DECISIONS, RuntimeCatalogCapability,
    RuntimeCatalogConfirmation, RuntimeCatalogProvenance, RuntimeExactBuildCoordinate,
    RuntimePermissionRequirement, RuntimeProviderUpdate, RuntimeRefusal, VerifiedArtifact,
    support::{bump_signal, now_millis},
};

/// Derives the finite permission inventory exclusively from verified bytes and
/// Rust-owned compatibility policy.
///
/// Signed `requires` tags remain authoritative for general artifacts. The
/// published Good Morning fixture predates those tags, so its immutable exact
/// build receives the required/optional profile already pinned by the native
/// runtime compatibility corpus. Native callers cannot select this profile or
/// supply capability names.
pub(super) fn installation_capability_requests(
    artifact: &VerifiedArtifact,
) -> Result<Vec<CapabilityRequest>, String> {
    let mut requests = artifact
        .handle
        .manifest()
        .requirements()
        .map(|domain| {
            Capability::new(domain)
                .map(|capability| CapabilityRequest {
                    capability,
                    requirement: CapabilityRequirement::Required,
                })
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;

    let is_pinned_good_morning = artifact.handle.index().author().as_str() == GOOD_MORNING_AUTHOR
        && artifact.handle.index().d_tag() == Some(GOOD_MORNING_D_TAG)
        && artifact.handle.index().aggregate().as_str() == GOOD_MORNING_AGGREGATE_HASH;
    if is_pinned_good_morning {
        debug_assert!(requests.is_empty());
        for (domain, requirement) in GOOD_MORNING_CAPABILITY_PROFILE {
            requests.push(CapabilityRequest {
                capability: Capability::new(*domain).map_err(|error| error.to_string())?,
                requirement: *requirement,
            });
        }
    }
    if requests.len() > MAXIMUM_PERMISSION_DECISIONS {
        return Err(format!(
            "verified capability profile has {} domains; the maximum is {}",
            requests.len(),
            MAXIMUM_PERMISSION_DECISIONS
        ));
    }
    Ok(requests)
}

pub(super) fn installed_manifest_event_id(build: &InstalledBuild) -> Result<String, String> {
    let metadata: serde_json::Value = serde_json::from_str(build.manifest_metadata.as_str())
        .map_err(|error| format!("installed manifest metadata is invalid JSON: {error}"))?;
    let event_id = metadata
        .get("event_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "installed manifest metadata has no verified event_id".to_owned())?;
    nmp_native_artifact::Sha256Digest::parse(event_id)
        .map_err(|error| format!("installed manifest event_id is invalid: {error}"))?;
    Ok(event_id.to_owned())
}

pub(super) fn installed_confirmation(
    artifact: &VerifiedArtifact,
    build: &InstalledBuild,
    provenance: Vec<RuntimeCatalogProvenance>,
) -> RuntimeCatalogConfirmation {
    RuntimeCatalogConfirmation {
        event_id: artifact.handle.index().event_id().as_str().to_owned(),
        coordinate: format!(
            "35129:{}:{}",
            build.principal.manifest_author(),
            build.principal.d_tag()
        ),
        manifest_author: build.principal.manifest_author().to_owned(),
        d_tag: Some(build.principal.d_tag().to_owned()),
        title: Some(build.title.to_string()),
        aggregate_hash: build.principal.aggregate_hash().to_owned(),
        capabilities: build
            .capability_requests
            .iter()
            .map(|request| RuntimeCatalogCapability {
                domain: request.capability.as_str().to_owned(),
                requirement: match request.requirement {
                    CapabilityRequirement::Required => RuntimePermissionRequirement::Required,
                    CapabilityRequirement::Optional => RuntimePermissionRequirement::Optional,
                },
            })
            .collect(),
        provenance,
    }
}

impl RuntimeController {
    pub(super) fn refusal(
        &self,
        code: impl Into<String>,
        detail: impl Into<String>,
    ) -> RuntimeRefusal {
        RuntimeRefusal {
            code: code.into(),
            detail: detail.into(),
            occurred_at_millis: now_millis(),
        }
    }

    pub(super) fn library_principal(
        &self,
        coordinate: RuntimeExactBuildCoordinate,
    ) -> Option<Principal> {
        match Principal::new(
            coordinate.manifest_author,
            coordinate.d_tag,
            coordinate.aggregate_hash,
        ) {
            Ok(principal) => Some(principal),
            Err(error) => {
                self.record_refusal("invalid-exact-build-coordinate", error.to_string());
                None
            }
        }
    }

    pub(super) fn workspace_refusal(
        &self,
        code: impl Into<String>,
        detail: impl Into<String>,
    ) -> RuntimeRefusal {
        let refusal = self.refusal(code, detail);
        self.record_boundary_refusal(refusal.clone());
        refusal
    }

    pub(super) fn provider_refusal(
        &self,
        code: impl Into<String>,
        detail: impl Into<String>,
    ) -> RuntimeProviderUpdate {
        let refusal = self.refusal(code, detail);
        self.record_boundary_refusal(refusal.clone());
        RuntimeProviderUpdate {
            accepted: false,
            attempted: 0,
            delivered: 0,
            refused: 0,
            refusal: Some(refusal),
        }
    }

    pub(super) fn record_refusal(&self, code: impl Into<String>, detail: impl Into<String>) {
        self.record_boundary_refusal(self.refusal(code, detail));
    }

    /// The single bounded-append path for boundary refusals. Eviction past the
    /// cap is counted, never silent: `dropped_boundary_refusals` reports it.
    fn record_boundary_refusal(&self, refusal: RuntimeRefusal) {
        self.boundary_refusals
            .lock()
            .push(self.maximum_boundary_events, refusal);
        bump_signal(&self.signal);
    }
}
