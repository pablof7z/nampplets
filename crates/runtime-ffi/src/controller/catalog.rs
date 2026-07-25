//! Catalog browse, review, confirmation, and installed-build reacquisition.

use std::sync::{Arc, atomic::Ordering};

use nmp_native_artifact::VerifiedArtifactHandle;
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

    /// Freezes an exact review from one entry in the most recent bounded page.
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

    /// Parses and freezes an exact public manifest coordinate entirely in
    /// Rust. Native presentation never interprets Nostr coordinate identity.
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

    /// Confirms one opaque frozen review and installs its immutable exact
    /// bytes. The pinned Good Morning demo profile receives the Rust-owned
    /// exact-build grant set immediately so the native Workbench can exercise
    /// the complete journey; other builds remain review-gated. This never
    /// launches the napplet.
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
    /// unfiltered persistent installation and returns the already-attached
    /// immutable handle only when its signed event, coordinate, aggregate, and
    /// capability inventory still match. After process restart this fails
    /// closed until the artifact owner exposes an exact persistent-cache reopen
    /// seam; this boundary never resolves a newer replaceable manifest as a
    /// substitute for the installed event.
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
        let Some(handle) = retained_handle else {
            return RuntimeCatalogConfirmationResult {
                confirmation: None,
                artifact: None,
                failure: Some(runtime_catalog_failure(
                    "artifact-handle-unavailable",
                    "the installed exact bytes are not retained in this runtime process",
                )),
            };
        };
        let artifact = match self.verified_installed_artifact(&installed, handle) {
            Ok(artifact) => artifact,
            Err(failure) => {
                return RuntimeCatalogConfirmationResult {
                    confirmation: None,
                    artifact: None,
                    failure: Some(failure),
                };
            }
        };
        RuntimeCatalogConfirmationResult {
            confirmation: Some(installed_confirmation(&artifact, &installed, Vec::new())),
            artifact: Some(artifact),
            failure: None,
        }
    }
}

impl RuntimeController {
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
