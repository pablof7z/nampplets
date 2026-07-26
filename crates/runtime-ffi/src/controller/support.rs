//! Private controller helpers: capability derivation and bounded refusals.

use nmp_native_artifact::INDEX_PATH;
use nmp_native_runtime_core::{Capability, CapabilityRequest, CapabilityRequirement, Principal};
use nmp_native_runtime_store::InstalledBuild;

use super::RuntimeController;
use crate::{
    MAXIMUM_PERMISSION_DECISIONS, RuntimeCatalogCapability, RuntimeCatalogConfirmation,
    RuntimeCatalogProvenance, RuntimeExactBuildCoordinate, RuntimePermissionRequirement,
    RuntimeProviderUpdate, RuntimeRefusal, VerifiedArtifact,
    snapshot_integrity::MAXIMUM_REPORTED_PROJECTION_FAULTS,
    support::{bump_signal, now_millis},
};

/// Derives the finite permission inventory exclusively from the artifact's own
/// verified bytes. No build's identity is special-cased: what a napplet
/// declares is what it gets, and native callers cannot select a profile or
/// supply capability names.
///
/// Signed `requires` tags are authoritative. A build that declares none falls
/// back to the `napplet-requires` meta inside the verified entry document --
/// still the artifact's own bytes, pinned by the signed path digest and
/// aggregate, so the declaration is as authenticated as a tag. A build that
/// declares neither gets an empty inventory and launches with only the
/// foundational shell; if its content needs more, it says so itself rather
/// than the runtime guessing on its behalf.
///
/// `intent_dispatch::launch_handler` relies on this same derivation: an
/// intent-launched handler must be admitted with exactly the domains an
/// interactive launch would ask for, or it starts with capabilities its own
/// content requires silently missing.
pub(crate) fn installation_capability_requests(
    artifact: &VerifiedArtifact,
) -> Result<Vec<CapabilityRequest>, String> {
    let requests = artifact
        .handle
        .authenticated_requirements()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|domain| {
            Capability::new(&domain)
                .map(|capability| CapabilityRequest {
                    capability,
                    requirement: CapabilityRequirement::Required,
                })
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;

    if requests.len() > MAXIMUM_PERMISSION_DECISIONS {
        return Err(format!(
            "verified capability profile has {} domains; the maximum is {}",
            requests.len(),
            MAXIMUM_PERMISSION_DECISIONS
        ));
    }
    Ok(requests)
}

/// Reads the `napplet-config-schema` declaration out of the verified entry
/// document so the config provider can install it before untrusted code runs.
/// Napplets read their settings through `config.subscribe`, which answers
/// `no-schema` until some schema is registered, and the published SDK never
/// registers the manifest one itself.
///
/// A parse failure here must not be indistinguishable from "this napplet
/// declared no schema" — the caller reports `Err` as a boundary refusal, not
/// as a quiet `None`, so `config.subscribe` answering `no-schema` forever has
/// a recorded reason.
pub(super) fn declared_config_schema(
    artifact: &VerifiedArtifact,
) -> Result<Option<serde_json::Value>, String> {
    let Some(document) = verified_index_document(artifact)? else {
        return Ok(None);
    };
    let Some(schema) = nmp_native_artifact::embedded_config_schema(&document) else {
        return Ok(None);
    };
    serde_json::from_str(&schema)
        .map(Some)
        .map_err(|error| format!("napplet-config-schema is not valid JSON: {error}"))
}

/// Reads the verified entry document, or `None` when the artifact has no
/// `/index.html` entry to read.
fn verified_index_document(artifact: &VerifiedArtifact) -> Result<Option<Vec<u8>>, String> {
    let Some(entry) = artifact
        .handle
        .index()
        .entries()
        .find(|entry| entry.path() == INDEX_PATH)
    else {
        return Ok(None);
    };
    artifact
        .handle
        .read_verified(INDEX_PATH, entry.bytes())
        .map(Some)
        .map_err(|error| error.to_string())
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
    pub(crate) fn refusal(
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

    /// Retains bounded evidence for a projection refusal without making the
    /// evidence ring part of the fail-closed delivery mechanism.
    pub(crate) fn report_projection_fault(&self, refusal: RuntimeRefusal) {
        enum Evidence {
            Exact,
            Capacity,
            None,
        }

        let key = (refusal.code.clone(), refusal.detail.clone());
        let evidence = {
            let mut latch = self.projection_fault_latch.lock();
            if latch.keys.contains(&key) {
                Evidence::None
            } else if latch.keys.len() < MAXIMUM_REPORTED_PROJECTION_FAULTS {
                latch.keys.insert(key);
                Evidence::Exact
            } else if !latch.overflow_reported {
                latch.overflow_reported = true;
                Evidence::Capacity
            } else {
                Evidence::None
            }
        };
        match evidence {
            Evidence::Exact => self.record_boundary_refusal(refusal),
            Evidence::Capacity => self.record_boundary_refusal(self.refusal(
                "projection-fault-latch-capacity",
                format!(
                    "projection fault evidence reached its capacity of \
                     {MAXIMUM_REPORTED_PROJECTION_FAULTS} distinct keys"
                ),
            )),
            Evidence::None => {}
        }
    }

    pub(crate) fn record_refusal(&self, code: impl Into<String>, detail: impl Into<String>) {
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
