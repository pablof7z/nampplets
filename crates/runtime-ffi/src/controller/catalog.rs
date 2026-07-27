//! Catalog browse, review, confirmation, and installed-build reacquisition.

use std::sync::{Arc, atomic::Ordering};

use base64::Engine as _;
use nmp_native_artifact::{
    ManifestCoordinate, ManifestError, ManifestEventLimits, ManifestEventVerifier,
    VerifiedArtifactHandle, VerifiedManifest, reopen_verified_artifact,
};
use nmp_native_runtime_app::{ExecutableArtifact, PlatformCommand};
use nmp_native_runtime_core::Principal;
use nmp_native_runtime_store::InstalledBuild;

use super::{
    RuntimeController,
    support::{
        installation_capability_requests, installed_confirmation, installed_manifest_event_id,
    },
};
use crate::{
    RuntimeCatalogCancellationResult, RuntimeCatalogConfirmationResult, RuntimeCatalogFailure,
    RuntimeCatalogFeedSnapshot, RuntimeCatalogPageResult, RuntimeCatalogReviewResult,
    RuntimeExactBuildCoordinate, VerifiedArtifact,
    catalog::project_catalog_error,
    projection::{parse_catalog_coordinate, runtime_catalog_failure},
};

#[uniffi::export]
impl RuntimeController {
    /// Reads the latest replacement from the profile's permanent finite NMP
    /// manifest feed. A non-empty query filters that replacement locally; it
    /// never opens another relay subscription or claims NIP-50 completeness.
    pub fn catalog_browse(&self, query: String) -> RuntimeCatalogPageResult {
        if self.closed.load(Ordering::Acquire) {
            return RuntimeCatalogPageResult {
                page: None,
                failure: Some(runtime_catalog_failure("closed", "runtime is closed")),
            };
        }
        if query.len() > 256 || query.chars().any(char::is_control) {
            return RuntimeCatalogPageResult {
                page: None,
                failure: Some(runtime_catalog_failure(
                    "invalid-query",
                    "catalog query exceeds 256 UTF-8 bytes or contains control characters",
                )),
            };
        }
        match self.catalog.browse(Some(&query)) {
            Ok(page) => RuntimeCatalogPageResult {
                page: Some(page),
                failure: None,
            },
            Err(error) => RuntimeCatalogPageResult {
                page: None,
                failure: Some(project_catalog_error(error)),
            },
        }
    }

    /// Returns the latest unfiltered catalog feed replacement and revision.
    /// The underlying NMP subscription remains profile-owned and permanent.
    pub fn catalog_feed_snapshot(&self) -> RuntimeCatalogFeedSnapshot {
        self.catalog.feed_snapshot(None)
    }

    /// Resolves immutable hash-matching bytes and then freezes an exact review
    /// from one entry in the most recent bounded page.
    pub fn catalog_review_entry(&self, event_id: String) -> RuntimeCatalogReviewResult {
        if self.closed.load(Ordering::Acquire) {
            return RuntimeCatalogReviewResult {
                review: None,
                failure: Some(runtime_catalog_failure("closed", "runtime is closed")),
            };
        }
        match self.catalog.begin_review_for_entry(&event_id) {
            Ok(review) => RuntimeCatalogReviewResult {
                review: Some(review),
                failure: None,
            },
            Err(error) => RuntimeCatalogReviewResult {
                review: None,
                failure: Some(project_catalog_error(error)),
            },
        }
    }

    /// Parses, verifies, and freezes an exact public manifest coordinate
    /// entirely in Rust. Native presentation never interprets Nostr coordinate
    /// identity or reconstructs requirements.
    pub fn catalog_review_manual(&self, coordinate: String) -> RuntimeCatalogReviewResult {
        if self.closed.load(Ordering::Acquire) {
            return RuntimeCatalogReviewResult {
                review: None,
                failure: Some(runtime_catalog_failure("closed", "runtime is closed")),
            };
        }
        let coordinate = match parse_catalog_coordinate(&coordinate) {
            Ok(coordinate) => coordinate,
            Err(detail) => {
                return RuntimeCatalogReviewResult {
                    review: None,
                    failure: Some(runtime_catalog_failure("invalid-coordinate", detail)),
                };
            }
        };
        match self.catalog.begin_review(coordinate) {
            Ok(review) => RuntimeCatalogReviewResult {
                review: Some(review),
                failure: None,
            },
            Err(error) => RuntimeCatalogReviewResult {
                review: None,
                failure: Some(project_catalog_error(error)),
            },
        }
    }

    /// Cancels and discards one opaque exact review without side effects.
    pub fn catalog_cancel_review(&self, token: String) -> RuntimeCatalogCancellationResult {
        match self.catalog.cancel_review(&token) {
            Ok(()) => RuntimeCatalogCancellationResult {
                cancelled: true,
                failure: None,
            },
            Err(error) => RuntimeCatalogCancellationResult {
                cancelled: false,
                failure: Some(project_catalog_error(error)),
            },
        }
    }

    /// Cancels transient exact review/acquisition work. The profile-owned
    /// catalog feed stays subscribed until the profile closes.
    pub fn catalog_cancel_pending(&self) -> RuntimeCatalogCancellationResult {
        self.catalog.cancel_pending();
        RuntimeCatalogCancellationResult {
            cancelled: true,
            failure: None,
        }
    }

    /// Consumes one opaque review and installs the immutable exact bytes that
    /// were already verified before review. Every build goes through the
    /// same review-gated `install()` path here -- no author, d-tag, or
    /// aggregate hash is special-cased, and the capability inventory
    /// `installation_capability_requests` derives comes only from the
    /// artifact's own verified bytes (see that function's doc for the pin
    /// this replaced, and
    /// `crates/runtime-ffi/src/tests/permissions.rs`'s
    /// `no_published_build_receives_a_runtime_pinned_capability_profile` for
    /// the test that keeps it gone). This never launches the napplet.
    pub fn catalog_confirm_install(
        &self,
        token: String,
        expected_author: String,
        expected_d_tag: String,
        expected_aggregate_hash: String,
    ) -> RuntimeCatalogConfirmationResult {
        if self.closed.load(Ordering::Acquire) {
            return RuntimeCatalogConfirmationResult {
                confirmation: None,
                artifact: None,
                failure: Some(runtime_catalog_failure("closed", "runtime is closed")),
            };
        }
        let confirmed = match self.catalog.confirm_review(&token) {
            Ok(confirmed) => confirmed,
            Err(error) => {
                return RuntimeCatalogConfirmationResult {
                    confirmation: None,
                    artifact: None,
                    failure: Some(project_catalog_error(error)),
                };
            }
        };
        let confirmation = confirmed.confirmation.clone();
        if confirmation.manifest_author != expected_author
            || confirmation.d_tag.as_deref() != Some(expected_d_tag.as_str())
            || confirmation.aggregate_hash != expected_aggregate_hash
        {
            return RuntimeCatalogConfirmationResult {
                confirmation: None,
                artifact: None,
                failure: Some(runtime_catalog_failure(
                    "confirmation-mismatch",
                    "native confirmation did not match the frozen exact review",
                )),
            };
        }
        let principal = match Principal::new(
            confirmation.manifest_author.clone(),
            expected_d_tag,
            confirmation.aggregate_hash.clone(),
        ) {
            Ok(principal) => principal,
            Err(error) => {
                return RuntimeCatalogConfirmationResult {
                    confirmation: None,
                    artifact: None,
                    failure: Some(runtime_catalog_failure(
                        "unsupported-manifest-identity",
                        error.to_string(),
                    )),
                };
            }
        };
        let artifact = Arc::new(VerifiedArtifact {
            handle: Arc::new(confirmed.into_handle()),
            principal: Some(principal.clone()),
        });
        self.install(Arc::clone(&artifact));
        let installed = self
            .app
            .snapshot()
            .library
            .builds
            .iter()
            .any(|candidate| candidate.build.principal == principal);
        if !installed {
            return RuntimeCatalogConfirmationResult {
                confirmation: None,
                artifact: None,
                failure: Some(runtime_catalog_failure(
                    "install-refused",
                    "the verified exact build was not accepted by the runtime library",
                )),
            };
        }
        RuntimeCatalogConfirmationResult {
            confirmation: Some(confirmation),
            artifact: Some(artifact),
            failure: None,
        }
    }

    /// Reopens one installed exact build from its retained verifier handle.
    ///
    /// Native supplies only the exact library coordinate. Rust checks the
    /// unfiltered persistent installation and returns a handle only when its
    /// signed event, coordinate, aggregate, and capability inventory still
    /// match. If this process already holds the verified handle from a
    /// prior install or reopen, that handle is reused directly. Otherwise
    /// (typically: first reopen after a process restart) this reconstructs
    /// it entirely from local state -- the exact signed manifest event bytes
    /// retained at original install time, re-verified, and the sealed
    /// artifact bytes already committed to the local artifact cache. No
    /// network access, and this never resolves a newer replaceable manifest
    /// as a substitute for the installed event.
    ///
    /// This call is blocking and must be invoked away from a native UI thread.
    pub fn reacquire_installed_artifact(
        &self,
        coordinate: RuntimeExactBuildCoordinate,
    ) -> RuntimeCatalogConfirmationResult {
        if self.closed.load(Ordering::Acquire) {
            return RuntimeCatalogConfirmationResult {
                confirmation: None,
                artifact: None,
                failure: Some(runtime_catalog_failure("closed", "runtime is closed")),
            };
        }
        let principal = match Principal::new(
            coordinate.manifest_author,
            coordinate.d_tag,
            coordinate.aggregate_hash,
        ) {
            Ok(principal) => principal,
            Err(error) => {
                return RuntimeCatalogConfirmationResult {
                    confirmation: None,
                    artifact: None,
                    failure: Some(runtime_catalog_failure(
                        "invalid-exact-build-coordinate",
                        error.to_string(),
                    )),
                };
            }
        };
        let installed = match self.runtime_store.installed_builds() {
            Ok(installed) => installed
                .into_iter()
                .find(|candidate| candidate.principal == principal),
            Err(error) => {
                return RuntimeCatalogConfirmationResult {
                    confirmation: None,
                    artifact: None,
                    failure: Some(runtime_catalog_failure(
                        "installed-library-unavailable",
                        error.to_string(),
                    )),
                };
            }
        };
        let Some(installed) = installed else {
            return RuntimeCatalogConfirmationResult {
                confirmation: None,
                artifact: None,
                failure: Some(runtime_catalog_failure(
                    "not-installed",
                    "the exact build is not present in the runtime library",
                )),
            };
        };
        let retained_handle = { self.artifacts.lock().get(&principal).cloned() };
        // A handle that had to be reopened from the sealed cache is not yet
        // trusted: nothing has checked that the bytes it reconstructed still
        // carry the capability inventory the user approved at install time.
        // Attaching it, dispatching `InstallVerified`, or registering it as
        // an intent handler before `verified_installed_artifact` says so
        // would publish an artifact the runtime is about to refuse -- and a
        // registered intent handler is reachable by other napplets, so the
        // window is not merely cosmetic. Validate first, publish second.
        let reopened = retained_handle.is_none();
        let handle = match retained_handle {
            Some(handle) => handle,
            None => match self.reopen_sealed_artifact(&principal, &installed) {
                Ok(handle) => handle,
                Err(failure) => {
                    return RuntimeCatalogConfirmationResult {
                        confirmation: None,
                        artifact: None,
                        failure: Some(failure),
                    };
                }
            },
        };
        let artifact = match self.verified_installed_artifact(&installed, Arc::clone(&handle)) {
            Ok(artifact) => artifact,
            Err(failure) => {
                return RuntimeCatalogConfirmationResult {
                    confirmation: None,
                    artifact: None,
                    failure: Some(failure),
                };
            }
        };
        if reopened {
            self.artifacts
                .lock()
                .insert(principal.clone(), Arc::clone(&handle));
            let executable: Arc<dyn ExecutableArtifact> = handle.clone();
            self.app.dispatch(PlatformCommand::InstallVerified {
                build: installed.clone(),
                artifact: executable,
            });
            self.register_intent_handler(&principal, &handle);
        }
        RuntimeCatalogConfirmationResult {
            confirmation: Some(installed_confirmation(&artifact, &installed, Vec::new())),
            artifact: Some(artifact),
            failure: None,
        }
    }
}

impl RuntimeController {
    /// Reconstructs one installed exact build entirely from local state: the
    /// exact signed manifest event bytes retained in `installed`'s metadata
    /// at original install time, and the sealed artifact bytes already
    /// committed to `self.artifact_cache`. No network access. Re-verifies
    /// the event signature exactly as a fresh install would, so a corrupted
    /// or substituted retained event is refused the same way any other
    /// invalid manifest would be.
    pub(super) fn reopen_sealed_artifact(
        &self,
        principal: &Principal,
        installed: &InstalledBuild,
    ) -> Result<Arc<VerifiedArtifactHandle>, RuntimeCatalogFailure> {
        let event_json = retained_manifest_event(installed)?;
        let coordinate = named_manifest_coordinate(principal)?;
        let verifier = self.manifest_event_verifier()?;
        let handle =
            reopen_verified_artifact(&verifier, &event_json, &coordinate, &self.artifact_cache)
                .map_err(|error| match error {
                    ManifestError::Artifact(_) => {
                        runtime_catalog_failure("sealed-bytes-unavailable", error.to_string())
                    }
                    _ => runtime_catalog_failure(
                        "installed-manifest-event-unavailable",
                        error.to_string(),
                    ),
                })?;
        Ok(Arc::new(handle))
    }

    /// Re-verifies the retained signed manifest event alone, without opening
    /// the sealed artifact bytes.
    ///
    /// This is the cheap half of `reopen_sealed_artifact`: enough to read
    /// anything the manifest itself authenticates -- archetypes, title,
    /// signed `requires` tags -- and not enough to execute the build. The
    /// startup intent-handler restore uses it to decide which installations
    /// are worth the full reopen, which re-reads and re-hashes every sealed
    /// byte of every file the build declares.
    pub(super) fn retained_manifest(
        &self,
        principal: &Principal,
        installed: &InstalledBuild,
    ) -> Result<VerifiedManifest, RuntimeCatalogFailure> {
        let event_json = retained_manifest_event(installed)?;
        let coordinate = named_manifest_coordinate(principal)?;
        self.manifest_event_verifier()?
            .verify_json(&event_json, &coordinate)
            .map_err(|error| {
                runtime_catalog_failure("installed-manifest-event-unavailable", error.to_string())
            })
    }

    fn manifest_event_verifier(&self) -> Result<ManifestEventVerifier, RuntimeCatalogFailure> {
        ManifestEventVerifier::new(ManifestEventLimits {
            maximum_event_bytes: self.maximum_manifest_bytes,
            ..ManifestEventLimits::default()
        })
        .map_err(|error| runtime_catalog_failure("invalid-limits", error.to_string()))
    }

    pub(crate) fn verified_installed_artifact(
        &self,
        build: &InstalledBuild,
        handle: Arc<VerifiedArtifactHandle>,
    ) -> Result<Arc<VerifiedArtifact>, RuntimeCatalogFailure> {
        let expected_event_id = installed_manifest_event_id(build)
            .map_err(|detail| runtime_catalog_failure("installed-metadata-invalid", detail))?;
        let index = handle.index();
        if index.kind() != 35_129
            || index.event_id().as_str() != expected_event_id
            || index.author().as_str() != build.principal.manifest_author()
            || index.d_tag() != Some(build.principal.d_tag())
            || index.aggregate().as_str() != build.principal.aggregate_hash()
        {
            return Err(runtime_catalog_failure(
                "installed-artifact-mismatch",
                "the verifier handle does not match the persisted exact signed manifest",
            ));
        }
        let artifact = Arc::new(VerifiedArtifact {
            handle,
            principal: Some(build.principal.clone()),
        });
        let requests = installation_capability_requests(&artifact)
            .map_err(|detail| runtime_catalog_failure("installed-capability-mismatch", detail))?;
        if requests != build.capability_requests {
            return Err(runtime_catalog_failure(
                "installed-capability-mismatch",
                "the verified manifest capability inventory differs from the persisted installation",
            ));
        }
        Ok(artifact)
    }
}

/// Recovers the exact signed manifest event bytes retained in an
/// installation's metadata at install time. Decoding only: the caller still
/// re-verifies the signature before trusting anything the event claims.
fn retained_manifest_event(installed: &InstalledBuild) -> Result<Vec<u8>, RuntimeCatalogFailure> {
    let metadata: serde_json::Value = serde_json::from_str(installed.manifest_metadata.as_str())
        .map_err(|error| {
            runtime_catalog_failure(
                "installed-manifest-event-unavailable",
                format!("installed manifest metadata is invalid JSON: {error}"),
            )
        })?;
    let signed_event_b64 = metadata
        .get("signed_event_b64")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            runtime_catalog_failure(
                "installed-manifest-event-unavailable",
                "this build was installed before offline reopen was supported; reinstall it once to enable reopening after a restart",
            )
        })?;
    base64::engine::general_purpose::STANDARD
        .decode(signed_event_b64)
        .map_err(|error| {
            runtime_catalog_failure(
                "installed-manifest-event-unavailable",
                format!("retained signed event is not valid base64: {error}"),
            )
        })
}

fn named_manifest_coordinate(
    principal: &Principal,
) -> Result<ManifestCoordinate, RuntimeCatalogFailure> {
    ManifestCoordinate::named(principal.manifest_author(), principal.d_tag()).map_err(|error| {
        runtime_catalog_failure("invalid-exact-build-coordinate", error.to_string())
    })
}
