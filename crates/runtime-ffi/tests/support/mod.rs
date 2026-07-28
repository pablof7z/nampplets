//! Shared runtime-ffi integration fixture.
//!
//! The plain integration test and Cucumber runner both drive this exact rig:
//! bounded corpus loading, real artifact verification, public FFI controller
//! calls, and the Rust-owned snapshot projection.
// This rig is shared by every runtime-ffi test target, so each one sees the
// whole fixture and uses only its own slice of it. Unused items and re-exports
// here are expected, not drift.
#![allow(dead_code, unused_imports)]

mod receipt;

use std::{fmt, path::PathBuf, sync::Arc};

use nmp_native_artifact::{Sha256Digest, embedded_requirements};
use nmp_native_runtime_core::{Capability, GrantDecision, Principal};
use nmp_native_runtime_ffi::{
    ArtifactCoordinate, ArtifactFetchRequest, ArtifactFetchResponse, ArtifactSource, RuntimeConfig,
    RuntimeController, RuntimeExactBuildCoordinate, RuntimeExecutionProfile,
    RuntimePermissionBatchUpdate, RuntimePermissionDecisionBatch,
    RuntimePermissionDecisionSelection, RuntimePermissionReviewSnapshot, RuntimeSnapshot,
    RuntimeSnapshotProjection, VerifiedArtifact,
};
use nmp_native_runtime_store::{RuntimeStore, StoreLimits};
use nmp_native_test_harness::{FixtureLoader, FsFixtureLoader};
use tempfile::TempDir;

pub use receipt::ReceiptProjectionRig;

/// The relay-lane scenarios never resolve an artifact; admission happens
/// before anything is fetched.
pub struct NoArtifactSource;

impl ArtifactSource for NoArtifactSource {
    fn fetch(&self, request: ArtifactFetchRequest) -> ArtifactFetchResponse {
        ArtifactFetchResponse::Body {
            source_url: request.candidate_urls.first().cloned().unwrap_or_default(),
            http_status: 404,
            bytes: Vec::new(),
        }
    }
}

/// Every `operator-relay-refused` detail on the current snapshot, and the
/// durable copy that the bounded ring cannot evict.
pub fn relay_refusals(controller: &RuntimeController) -> (Vec<String>, Vec<String>) {
    let RuntimeSnapshotProjection::Snapshot { snapshot } = controller.snapshot() else {
        panic!("the relay scenarios never produce a refused snapshot");
    };
    let ringed = snapshot
        .boundary_refusals
        .iter()
        .filter(|refusal| refusal.code == "operator-relay-refused")
        .map(|refusal| refusal.detail.clone())
        .collect();
    let durable = snapshot
        .refused_operator_relays
        .iter()
        .map(|refusal| refusal.detail.clone())
        .collect();
    (ringed, durable)
}

const AUTHOR: &str = "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5";
const D_TAG: &str = "good-morning";
const MAXIMUM_FIXTURE_BYTES: usize = 512 * 1_024;

struct ExactFixtureSource {
    digest: String,
    index: Vec<u8>,
}

impl ArtifactSource for ExactFixtureSource {
    fn fetch(&self, request: ArtifactFetchRequest) -> ArtifactFetchResponse {
        let matches = request.expected_sha256 == self.digest;
        ArtifactFetchResponse::Body {
            source_url: request.candidate_urls.first().cloned().unwrap_or_default(),
            http_status: if matches { 200 } else { 404 },
            bytes: if matches {
                self.index.clone()
            } else {
                Vec::new()
            },
        }
    }
}

pub struct PermissionReviewRig {
    _temp: TempDir,
    controller: Arc<RuntimeController>,
    artifact: Arc<VerifiedArtifact>,
    embedded_domains: Vec<String>,
    coordinate: RuntimeExactBuildCoordinate,
    runtime_store_path: PathBuf,
}

impl fmt::Debug for PermissionReviewRig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PermissionReviewRig")
            .field("coordinate", &self.coordinate)
            .finish_non_exhaustive()
    }
}

impl PermissionReviewRig {
    pub fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../conformance/napplet-corpus/published/good-morning");
        let loader = FsFixtureLoader::new(root, MAXIMUM_FIXTURE_BYTES);
        let event = loader
            .load("event.json")
            .expect("bounded signed event fixture");
        let index = loader
            .load("index.html")
            .expect("bounded entry document fixture");
        let digest = Sha256Digest::of(&index).as_str().to_owned();
        let embedded_domains = embedded_requirements(&index)
            .into_iter()
            .map(str::to_owned)
            .collect();
        let temp = TempDir::new().expect("temporary runtime directory");
        let runtime_store_path = temp.path().join("runtime.sqlite3");
        let controller = RuntimeController::open(
            RuntimeConfig {
                runtime_store_path: runtime_store_path.display().to_string(),
                nmp_store_path: None,
                artifact_cache_path: temp.path().join("artifacts").display().to_string(),
                ..RuntimeConfig::default()
            },
            Box::new(ExactFixtureSource { digest, index }),
        )
        .expect("runtime opens");
        let artifact = controller
            .verify_artifact(
                event,
                ArtifactCoordinate::Named {
                    author: AUTHOR.to_owned(),
                    d_tag: D_TAG.to_owned(),
                },
            )
            .artifact
            .expect("published artifact verifies");
        let coordinate = RuntimeExactBuildCoordinate {
            manifest_author: artifact.author(),
            d_tag: artifact.d_tag().expect("named fixture"),
            aggregate_hash: artifact.aggregate_hash(),
        };
        controller.install(Arc::clone(&artifact));
        Self {
            _temp: temp,
            controller,
            artifact,
            embedded_domains,
            coordinate,
            runtime_store_path,
        }
    }

    pub fn has_no_signed_requirements(&self) -> bool {
        self.artifact.requires().is_empty()
    }

    pub fn embedded_domains(&self) -> &[String] {
        &self.embedded_domains
    }

    pub fn coordinate(&self) -> &RuntimeExactBuildCoordinate {
        &self.coordinate
    }

    pub fn permission_review(&self) -> RuntimePermissionReviewSnapshot {
        self.controller
            .permission_review(self.coordinate.clone())
            .review
            .expect("installed exact build has a permission review")
    }

    pub fn set_host_policy(&self, domain: &str) {
        let store = RuntimeStore::open(&self.runtime_store_path, StoreLimits::default())
            .expect("test host policy opens the runtime store");
        let principal = Principal::new(
            &self.coordinate.manifest_author,
            &self.coordinate.d_tag,
            &self.coordinate.aggregate_hash,
        )
        .expect("fixture coordinate is a principal");
        let capability = Capability::new(domain).expect("scenario uses a valid capability");
        store
            .set_grant(&principal, &capability, GrantDecision::Managed)
            .expect("host policy persists");
    }

    pub fn apply_changes(
        &self,
        review_revision: String,
        decisions: Vec<RuntimePermissionDecisionSelection>,
    ) -> RuntimePermissionBatchUpdate {
        self.controller
            .apply_permission_decisions(RuntimePermissionDecisionBatch {
                coordinate: self.coordinate.clone(),
                review_revision,
                decisions,
            })
    }

    pub fn launch_without_grants(&self) {
        self.controller
            .launch(Arc::clone(&self.artifact), RuntimeExecutionProfile::Legacy);
    }

    pub fn snapshot(&self) -> RuntimeSnapshot {
        match self.controller.snapshot() {
            RuntimeSnapshotProjection::Snapshot { snapshot } => snapshot,
            RuntimeSnapshotProjection::Refused {
                revision,
                closed,
                refusal,
            } => panic!(
                "runtime snapshot revision {revision} (closed={closed}) was refused: {}: {}",
                refusal.code, refusal.detail
            ),
        }
    }
}

impl Drop for PermissionReviewRig {
    fn drop(&mut self) {
        self.controller.close();
    }
}
