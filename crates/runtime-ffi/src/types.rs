//! Sealed verified-artifact handles and the leaf records they travel with.

use std::sync::Arc;

use nmp_native_artifact::{ArtifactMode, VerifiedArtifactHandle};
use nmp_native_providers::ProviderPushReport;
use nmp_native_runtime_core::Principal;

#[derive(Clone, Debug, uniffi::Enum)]
pub enum ArtifactCoordinate {
    Snapshot { event_id: String, author: String },
    Root { author: String },
    Named { author: String, d_tag: String },
}

#[derive(Debug, uniffi::Object)]
pub struct VerifiedArtifact {
    pub(crate) handle: Arc<VerifiedArtifactHandle>,
    pub(crate) principal: Option<Principal>,
}

#[uniffi::export]
impl VerifiedArtifact {
    pub fn author(&self) -> String {
        self.handle.index().author().as_str().to_owned()
    }

    pub fn d_tag(&self) -> Option<String> {
        self.handle.index().d_tag().map(str::to_owned)
    }

    pub fn aggregate_hash(&self) -> String {
        self.handle.index().aggregate().as_str().to_owned()
    }

    pub fn manifest_kind(&self) -> u16 {
        self.handle.index().kind()
    }

    pub fn mode(&self) -> ArtifactExecutionMode {
        match self.handle.index().mode() {
            ArtifactMode::SingleFile => ArtifactExecutionMode::SingleFile,
            ArtifactMode::ExternalAssets => ArtifactExecutionMode::ExternalAssets,
        }
    }

    pub fn logical_paths(&self) -> Vec<String> {
        self.handle
            .index()
            .entries()
            .map(|entry| entry.path().to_owned())
            .collect()
    }

    /// Verified manifest requirements. Native presentation may render these,
    /// but launch authority always derives them again from the sealed handle.
    pub fn requires(&self) -> Vec<String> {
        self.handle
            .manifest()
            .requirements()
            .map(str::to_owned)
            .collect()
    }
}

#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum ArtifactExecutionMode {
    SingleFile,
    ExternalAssets,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ArtifactVerification {
    pub artifact: Option<Arc<VerifiedArtifact>>,
    pub refusal: Option<RuntimeRefusal>,
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum VerifiedRead {
    Bytes {
        bytes: Vec<u8>,
        media_type: String,
        sha256: String,
    },
    Refused {
        refusal: RuntimeRefusal,
    },
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeRefusal {
    pub code: String,
    pub detail: String,
    pub occurred_at_millis: u64,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeProviderUpdate {
    pub accepted: bool,
    pub attempted: u64,
    pub delivered: u64,
    pub refused: u64,
    pub refusal: Option<RuntimeRefusal>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativeConfigCommit {
    pub manifest_author: String,
    pub d_tag: String,
    pub aggregate_hash: String,
    pub session_id: u64,
    pub values_json: String,
}

impl RuntimeProviderUpdate {
    pub(crate) fn accepted(report: ProviderPushReport) -> Self {
        Self {
            accepted: true,
            attempted: report.attempted as u64,
            delivered: report.delivered as u64,
            refused: report.refused as u64,
            refusal: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeAccountHandle {
    pub installation_id: u64,
    pub public_key: String,
    pub kind: RuntimeAccountKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeAccountKind {
    LocalSigner,
    ReadOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeAccountSnapshot {
    pub generation: u64,
    pub active_public_key: Option<String>,
    pub local_accounts: Vec<RuntimeAccountHandle>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeAccountFailure {
    Closed,
    InvalidSecretKey,
    InvalidPublicKey,
    Nip05ResolutionUnavailable,
    Capacity { limit: u64 },
    InstanceExhausted,
    StaleInstallation,
    Failed { reason: String },
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeAccountUpdate {
    pub accepted: bool,
    pub handle: Option<RuntimeAccountHandle>,
    pub snapshot: Option<RuntimeAccountSnapshot>,
    pub failure: Option<RuntimeAccountFailure>,
}
