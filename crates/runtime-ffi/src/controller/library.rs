//! Artifact verification, installation, and installed-library commands.

use std::sync::{Arc, atomic::Ordering};

use nmp_native_artifact::{
    ArtifactMode, ArtifactSourcePolicy, ManifestEventLimits, ManifestEventVerifier,
    SignedArtifactResolver,
};
use nmp_native_runtime_app::{ExecutableArtifact, PermissionDecision, PlatformCommand};
use nmp_native_runtime_core::{BoundedJson, GrantDecision, Principal};
use nmp_native_runtime_store::{InstalledBuild, UninstallCleanupPolicy};

use super::{RuntimeController, support::installation_capability_requests};
use crate::{
    ArtifactCoordinate, ArtifactVerification, RuntimeExactBuildCoordinate, RuntimePermissionMode,
    VerifiedArtifact, projection::map_coordinate, support::bump_signal,
    workspace::validate_workspace_name,
};

#[uniffi::export]
impl RuntimeController {
    pub fn verify_artifact(
        &self,
        event_json: Vec<u8>,
        coordinate: ArtifactCoordinate,
    ) -> ArtifactVerification {
        if self.closed.load(Ordering::Acquire) {
            return ArtifactVerification {
                artifact: None,
                refusal: Some(self.refusal("closed", "runtime is closed")),
            };
        }
        if event_json.len() > self.maximum_manifest_bytes {
            return ArtifactVerification {
                artifact: None,
                refusal: Some(self.refusal(
                    "manifest-too-large",
                    format!(
                        "manifest has {} bytes; the maximum is {}",
                        event_json.len(),
                        self.maximum_manifest_bytes
                    ),
                )),
            };
        }
        let coordinate = match map_coordinate(coordinate) {
            Ok(coordinate) => coordinate,
            Err(detail) => {
                return ArtifactVerification {
                    artifact: None,
                    refusal: Some(self.refusal("invalid-coordinate", detail)),
                };
            }
        };
        let verifier = match ManifestEventVerifier::new(ManifestEventLimits {
            maximum_event_bytes: self.maximum_manifest_bytes,
            ..ManifestEventLimits::default()
        }) {
            Ok(verifier) => verifier,
            Err(error) => {
                return ArtifactVerification {
                    artifact: None,
                    refusal: Some(self.refusal("invalid-limits", error.to_string())),
                };
            }
        };
        let policy = match ArtifactSourcePolicy::manifest_https_only(self.maximum_blob_sources) {
            Ok(policy) => policy,
            Err(error) => {
                return ArtifactVerification {
                    artifact: None,
                    refusal: Some(self.refusal("invalid-source-policy", error.to_string())),
                };
            }
        };
        let resolver = match SignedArtifactResolver::new(
            verifier,
            self.artifact_limits,
            policy,
            &self.artifact_source,
            &self.artifact_cache,
        ) {
            Ok(resolver) => resolver,
            Err(error) => {
                return ArtifactVerification {
                    artifact: None,
                    refusal: Some(self.refusal("invalid-resolver", error.to_string())),
                };
            }
        };
        match resolver.resolve_json(&event_json, &coordinate) {
            Ok(handle) => {
                let handle = Arc::new(handle);
                let principal = handle.index().d_tag().and_then(|d_tag| {
                    Principal::new(
                        handle.index().author().as_str(),
                        d_tag,
                        handle.index().aggregate().as_str(),
                    )
                    .ok()
                });
                ArtifactVerification {
                    artifact: Some(Arc::new(VerifiedArtifact { handle, principal })),
                    refusal: None,
                }
            }
            Err(error) => ArtifactVerification {
                artifact: None,
                refusal: Some(self.refusal("artifact-verification", error.to_string())),
            },
        }
    }

    pub fn install(&self, artifact: Arc<VerifiedArtifact>) {
        let Some(principal) = artifact.principal.clone() else {
            self.record_refusal(
                "unsupported-manifest-identity",
                "only verified named manifests currently mint exact-build principals",
            );
            return;
        };
        let metadata = serde_json::json!({
            "event_id": artifact.handle.index().event_id().as_str(),
            "kind": artifact.handle.index().kind(),
            "mode": match artifact.handle.index().mode() {
                ArtifactMode::SingleFile => "single-file",
                ArtifactMode::ExternalAssets => "external-assets",
            },
            "paths": artifact.handle.index().entries().len(),
        });
        let metadata = match BoundedJson::from_value(&metadata, 256 * 1_024) {
            Ok(metadata) => metadata,
            Err(error) => {
                self.record_refusal("manifest-metadata", error.to_string());
                return;
            }
        };
        let title: Arc<str> = Arc::from(
            artifact
                .handle
                .manifest()
                .title()
                .unwrap_or("Untitled napplet"),
        );
        let capability_requests = match installation_capability_requests(&artifact) {
            Ok(requests) => requests,
            Err(error) => {
                self.record_refusal("invalid-capability-request", error);
                return;
            }
        };
        self.artifacts
            .lock()
            .insert(principal.clone(), Arc::clone(&artifact.handle));
        let executable: Arc<dyn ExecutableArtifact> = artifact.handle.clone();
        self.app.dispatch(PlatformCommand::InstallVerified {
            build: InstalledBuild {
                principal: principal.clone(),
                title,
                manifest_metadata: metadata,
                capability_requests,
            },
            artifact: executable,
        });
        self.grant_demo_permissions(
            principal.manifest_author(),
            principal.d_tag(),
            principal.aggregate_hash(),
        );
        bump_signal(&self.signal);
    }

    /// The explicit demo mode is intentionally permissive so a locally
    /// verified network napplet can be rendered and exercised end-to-end.
    /// Interactive production profiles still require the normal exact-build
    /// permission review.
    pub(super) fn grant_demo_permissions(&self, author: &str, d_tag: &str, aggregate_hash: &str) {
        if self.permission_mode != RuntimePermissionMode::DemoPinnedGoodMorning {
            return;
        }
        let Ok(principal) = Principal::new(author, d_tag, aggregate_hash) else {
            return;
        };
        let Ok(review) = self.app.permission_review(&principal) else {
            return;
        };
        let decisions = review
            .capabilities
            .into_iter()
            .map(|capability| PermissionDecision {
                capability: capability.capability,
                decision: capability
                    .decision_options
                    .into_iter()
                    .find(|option| {
                        option.valid && option.decision == GrantDecision::AllowExactBuild
                    })
                    .map_or(GrantDecision::Denied, |option| option.decision),
            })
            .collect::<Vec<_>>();
        if !decisions.is_empty() {
            self.app.dispatch(PlatformCommand::ApplyPermissionBatch {
                principal: principal.clone(),
                decisions,
            });
        }
    }

    pub(super) fn grant_demo_permissions_for_installed_builds(&self) {
        if self.permission_mode != RuntimePermissionMode::DemoPinnedGoodMorning {
            return;
        }
        let principals = self
            .app
            .snapshot()
            .library
            .builds
            .iter()
            .map(|view| view.build.principal.clone())
            .collect::<Vec<_>>();
        for principal in principals {
            self.grant_demo_permissions(
                principal.manifest_author(),
                principal.d_tag(),
                principal.aggregate_hash(),
            );
        }
    }

    /// Applies the Rust-owned, finite installed-library filter. The resulting
    /// bounded view is emitted in `RuntimeSnapshot.installed_library`.
    pub fn set_library_filter(&self, query: String) {
        self.app.dispatch(PlatformCommand::SetLibraryFilter {
            query: Arc::from(query),
        });
        bump_signal(&self.signal);
    }

    /// Removes only runtime-owned state for one exact build. NMP canonical
    /// facts and durable receipts are unreachable from this command, and
    /// artifact-cache bytes remain until the artifact owner exposes a safe
    /// exact-build deletion API.
    pub fn uninstall_build(&self, coordinate: RuntimeExactBuildCoordinate) {
        let Some(principal) = self.library_principal(coordinate) else {
            return;
        };
        self.app.dispatch(PlatformCommand::Uninstall {
            principal: principal.clone(),
            cleanup: UninstallCleanupPolicy::RuntimeOwnedExactBuildState,
        });
        let remains_installed = self
            .app
            .snapshot()
            .library
            .builds
            .iter()
            .any(|candidate| candidate.build.principal == principal);
        if !remains_installed {
            self.artifacts.lock().remove(&principal);
        }
        bump_signal(&self.signal);
    }

    /// Assigns one installed exact build to an existing durable workspace.
    /// The runtime store validates both sides and enforces assignment bounds.
    pub fn assign_build_to_workspace(
        &self,
        workspace_id: String,
        coordinate: RuntimeExactBuildCoordinate,
    ) {
        if let Err(detail) = validate_workspace_name("workspace_id", &workspace_id) {
            self.record_refusal("invalid-workspace-assignment", detail);
            return;
        }
        let Some(principal) = self.library_principal(coordinate) else {
            return;
        };
        self.app.dispatch(PlatformCommand::AssignWorkspaceBuild {
            workspace_id: Arc::from(workspace_id),
            principal,
        });
        bump_signal(&self.signal);
    }

    /// Clears one exact build assignment without deleting the workspace,
    /// installation, artifact bytes, NMP facts, or retained receipt ids.
    pub fn clear_build_from_workspace(
        &self,
        workspace_id: String,
        coordinate: RuntimeExactBuildCoordinate,
    ) {
        if let Err(detail) = validate_workspace_name("workspace_id", &workspace_id) {
            self.record_refusal("invalid-workspace-assignment", detail);
            return;
        }
        let Some(principal) = self.library_principal(coordinate) else {
            return;
        };
        self.app.dispatch(PlatformCommand::RemoveWorkspaceBuild {
            workspace_id: Arc::from(workspace_id),
            principal,
        });
        bump_signal(&self.signal);
    }
}
