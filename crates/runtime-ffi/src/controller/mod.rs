//! The single Rust-owned controller exposed across the UniFFI boundary.

mod accounts;
mod catalog;
mod intent_restore;
mod library;
mod observation;
mod open;
mod permission_changes;
mod permissions;
mod preferences;
mod providers;
mod session;
mod snapshot;
pub(crate) mod support;
mod workspace;

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use nmp_native_artifact::{ArtifactLimits, FileArtifactCache, VerifiedArtifactHandle};
use nmp_native_nmp_adapter::NmpDataPlane;
use nmp_native_provider_link::IntentProvider;
use nmp_native_providers::{
    ConfigProvider, ShellEnvironment, ShellEnvironmentError, ShellEnvironmentLimits,
    ShellEnvironmentSource, ThemeProvider,
};
use nmp_native_runtime_app::{BoundedFacts, KernelClock, RuntimeApp};
use nmp_native_runtime_core::{Capability, Principal, SessionId};
use nmp_native_runtime_store::RuntimeStore;
use parking_lot::Mutex;
use tokio::sync::watch;

use crate::{
    RuntimeProfilePreferences, RuntimeRefusal, catalog::RuntimeCatalogService,
    diagnostics::RuntimeDiagnosticsService, native_capabilities::CallbackArtifactSource,
    native_capabilities::RuntimeThemeSource, slots::SlotHub, support::now_millis,
};

#[derive(Debug, Default)]
struct ProjectionFaultLatch {
    keys: BTreeSet<(String, String)>,
    overflow_reported: bool,
}

#[derive(uniffi::Object)]
pub struct RuntimeController {
    pub(crate) app: Arc<RuntimeApp>,
    data_plane: Arc<NmpDataPlane>,
    pub(crate) runtime_store: Arc<RuntimeStore>,
    artifact_cache: Arc<FileArtifactCache>,
    catalog: Arc<RuntimeCatalogService>,
    diagnostics: Arc<RuntimeDiagnosticsService>,
    artifact_source: CallbackArtifactSource,
    artifact_limits: ArtifactLimits,
    maximum_manifest_bytes: usize,
    maximum_verified_read_bytes: usize,
    maximum_blob_sources: usize,
    maximum_command_items: usize,
    maximum_command_string_bytes: usize,
    maximum_envelope_bytes: usize,
    theme_source: Option<Arc<RuntimeThemeSource>>,
    theme_provider: Option<Arc<ThemeProvider>>,
    config_provider: Option<Arc<ConfigProvider>>,
    pub(crate) intent_provider: Arc<IntentProvider>,
    pub(crate) artifacts: Arc<Mutex<BTreeMap<Principal, Arc<VerifiedArtifactHandle>>>>,
    boundary_refusals: Mutex<BoundedFacts<RuntimeRefusal>>,
    /// Relays refused at open, retained for the whole process lifetime.
    ///
    /// These also reach `boundary_refusals`, but that ring evicts: a busy
    /// session can push an open-time deployment fault out of it and
    /// re-silence exactly the thing this was added to stop hiding. The set is
    /// finite by construction -- it cannot exceed the configured lanes -- so
    /// retaining it costs nothing that needs bounding.
    refused_operator_relays: Vec<RuntimeRefusal>,
    projection_fault_latch: Mutex<ProjectionFaultLatch>,
    maximum_boundary_events: usize,
    signal: watch::Sender<u64>,
    observers: Arc<AtomicUsize>,
    slot_hub: Arc<SlotHub>,
    maximum_observers: usize,
    profile_preferences: Mutex<RuntimeProfilePreferences>,
    nmp_store_path: Option<PathBuf>,
    artifact_cache_path: PathBuf,
    closed: AtomicBool,
}

impl fmt::Debug for RuntimeController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeController")
            .field("snapshot_revision", &self.app.snapshot().revision)
            .field("retained_artifacts", &self.artifacts.lock().len())
            .field("active_observers", &self.observers.load(Ordering::Acquire))
            .field("maximum_observers", &self.maximum_observers)
            .field("closed", &self.closed.load(Ordering::Acquire))
            .finish()
    }
}

impl Drop for RuntimeController {
    fn drop(&mut self) {
        self.close();
    }
}

#[derive(Debug)]
struct ObserverPermit(Arc<AtomicUsize>);

impl Drop for ObserverPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug)]
struct SystemClock;

impl KernelClock for SystemClock {
    fn now_millis(&self) -> u64 {
        now_millis()
    }
}

#[derive(Debug)]
struct RuntimeShellEnvironment;

impl ShellEnvironmentSource for RuntimeShellEnvironment {
    fn environment(
        &self,
        _principal: &Principal,
        _session: SessionId,
        offered_domains: &BTreeSet<Capability>,
    ) -> Result<ShellEnvironment, ShellEnvironmentError> {
        ShellEnvironment::new(
            offered_domains.iter().cloned(),
            std::iter::empty::<Arc<str>>(),
            ShellEnvironmentLimits::default(),
        )
    }
}
