//! Kernel-owned application limits, injected time, artifact handles, and the
//! composition-root configuration used to open a [`RuntimeApp`].

use std::{fmt, sync::Arc};

use nmp_native_artifact::VerifiedArtifactHandle;
use nmp_native_nap_bridge::{BridgeError, BridgeLimits, Provider};
use nmp_native_providers::ShellProvider;
use nmp_native_runtime_core::{
    GrantError, GrantLimits, HostDataPlane, ResourceLimits, ResourceRefusal,
};
use nmp_native_runtime_store::{PermissionDefaultPreference, RuntimeStore, StoreError};
use nmp_native_surface::BindingLimits;
use thiserror::Error;

/// Kernel-owned limits. Every collection crossing the platform boundary is
/// bounded by one of these values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppLimits {
    pub maximum_installed_artifacts: usize,
    pub maximum_library_query_bytes: usize,
    pub maximum_sessions: usize,
    pub maximum_bindings: usize,
    pub maximum_receipts: usize,
    pub maximum_provider_operations: usize,
    pub maximum_activity_facts: usize,
    pub maximum_error_facts: usize,
    pub maximum_platform_events: usize,
    pub maximum_provider_push_batch: usize,
    pub maximum_receipt_frame_bytes: usize,
    pub maximum_envelope_bytes: usize,
}

impl Default for AppLimits {
    fn default() -> Self {
        Self {
            maximum_installed_artifacts: 512,
            maximum_library_query_bytes: 256,
            maximum_sessions: 16,
            maximum_bindings: 64,
            maximum_receipts: 256,
            maximum_provider_operations: 128,
            maximum_activity_facts: 1_024,
            maximum_error_facts: 256,
            maximum_platform_events: 1_024,
            maximum_provider_push_batch: 64,
            maximum_receipt_frame_bytes: 256 * 1024,
            maximum_envelope_bytes: 256 * 1024,
        }
    }
}

impl AppLimits {
    pub(crate) fn validate(self) -> Result<Self, OpenError> {
        if [
            self.maximum_installed_artifacts,
            self.maximum_library_query_bytes,
            self.maximum_sessions,
            self.maximum_bindings,
            self.maximum_receipts,
            self.maximum_provider_operations,
            self.maximum_activity_facts,
            self.maximum_error_facts,
            self.maximum_platform_events,
            self.maximum_provider_push_batch,
            self.maximum_receipt_frame_bytes,
            self.maximum_envelope_bytes,
        ]
        .contains(&0)
        {
            return Err(OpenError::InvalidLimits);
        }
        Ok(self)
    }
}

/// Time is an explicit nondeterministic input owned by the Rust kernel.
pub trait KernelClock: Send + Sync + fmt::Debug {
    fn now_millis(&self) -> u64;
}

/// Adaptable immutable executable handle implemented by the trusted Rust
/// artifact-resolution boundary. Platform and untrusted component code never
/// construct implementations of this interface.
pub trait ExecutableArtifact: Send + Sync + fmt::Debug {
    fn manifest_kind(&self) -> u16;
    fn manifest_author(&self) -> &str;
    fn d_tag(&self) -> Option<&str>;
    fn aggregate_hash(&self) -> &str;
    fn contains_logical_path(&self, logical_path: &str) -> bool;
}

impl ExecutableArtifact for VerifiedArtifactHandle {
    fn manifest_kind(&self) -> u16 {
        self.index().kind()
    }

    fn manifest_author(&self) -> &str {
        self.index().author().as_str()
    }

    fn d_tag(&self) -> Option<&str> {
        self.index().d_tag()
    }

    fn aggregate_hash(&self) -> &str {
        self.index().aggregate().as_str()
    }

    fn contains_logical_path(&self, logical_path: &str) -> bool {
        self.index()
            .entries()
            .any(|entry| entry.path() == logical_path)
    }
}

#[derive(Debug)]
pub struct RuntimeAppConfig {
    pub limits: AppLimits,
    pub resource_limits: ResourceLimits,
    pub grant_limits: GrantLimits,
    pub bridge_limits: BridgeLimits,
    pub binding_limits: BindingLimits,
    pub store: Arc<RuntimeStore>,
    pub data_plane: Arc<dyn HostDataPlane>,
    pub clock: Arc<dyn KernelClock>,
    /// Default selection for a capability with no existing decision.
    /// Permission review remains mandatory; this never applies a grant.
    pub permission_default: PermissionDefaultPreference,
    /// The mandatory NAP-SHELL provider. It is registered exactly once by the
    /// kernel and retained as the session-establishment authority.
    pub shell_provider: Arc<ShellProvider>,
    /// Fully conformant non-shell providers only.
    pub providers: Vec<Arc<dyn Provider>>,
}

#[derive(Debug, Error)]
pub enum OpenError {
    #[error("application limits must all be finite and non-zero")]
    InvalidLimits,
    #[error(transparent)]
    Resource(#[from] ResourceRefusal),
    #[error(transparent)]
    Grant(#[from] GrantError),
    #[error(transparent)]
    Bridge(#[from] BridgeError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("persistent library has {actual} builds; the application maximum is {maximum}")]
    InstalledLibraryCapacity { actual: usize, maximum: usize },
}
