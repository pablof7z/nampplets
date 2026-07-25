//! Controller construction: every native capability wiring lives here.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize},
};

use nmp::EngineConfig;
use nmp_native_artifact::FileArtifactCache;
use nmp_native_nap_bridge::{BridgeLimits, Provider};
use nmp_native_nmp_adapter::{NapNostrProviderLimits, NapNostrProviderSet, NmpDataPlane};
use nmp_native_provider_identity::{
    IdentityDataPlane, IdentityProvider, IdentityProviderLimits, NoopIdentityDiagnostics,
};
use nmp_native_provider_inc::{AllowAllIncAcl, IncProvider, IncProviderLimits, NoopIncActivity};
use nmp_native_providers::{
    ConfigProvider, ConfigProviderLimits, ShellProvider, ShellProviderLimits, StorageProvider,
    StorageProviderLimits, ThemeProvider, ThemeProviderLimits,
};
use nmp_native_runtime_app::{AppLimits, BoundedFacts, RuntimeApp, RuntimeAppConfig};
use nmp_native_runtime_core::{GrantLimits, ResourceLimits};
use nmp_native_runtime_store::{RuntimeStore, StoreLimits};
use nmp_native_surface::BindingLimits;
use parking_lot::Mutex;
use std::collections::BTreeMap;
use tokio::sync::watch;

use super::{RuntimeController, RuntimeShellEnvironment, SystemClock};
use crate::{
    ArtifactSource, NativeAppearanceSource, NativeIncActionExecutor, NativeSettingsExecutor,
    RuntimeConfig, RuntimeOpenError,
    catalog::RuntimeCatalogService,
    diagnostics::RuntimeDiagnosticsService,
    native_capabilities::{
        CallbackArtifactSource, CallbackIncNativeActions, CallbackSettingsExecutor,
        RuntimeThemeSource,
    },
    projection::theme_from_appearance,
};

#[uniffi::export]
impl RuntimeController {
    #[uniffi::constructor]
    pub fn open(
        config: RuntimeConfig,
        artifact_source: Box<dyn ArtifactSource>,
    ) -> Result<Arc<Self>, RuntimeOpenError> {
        open_runtime_controller(config, artifact_source, None, None, None)
    }

    #[uniffi::constructor]
    pub fn open_with_appearance(
        config: RuntimeConfig,
        artifact_source: Box<dyn ArtifactSource>,
        appearance_source: Box<dyn NativeAppearanceSource>,
    ) -> Result<Arc<Self>, RuntimeOpenError> {
        open_runtime_controller(
            config,
            artifact_source,
            Some(Arc::from(appearance_source)),
            None,
            None,
        )
    }

    #[uniffi::constructor]
    pub fn open_with_settings(
        config: RuntimeConfig,
        artifact_source: Box<dyn ArtifactSource>,
        settings_executor: Box<dyn NativeSettingsExecutor>,
    ) -> Result<Arc<Self>, RuntimeOpenError> {
        open_runtime_controller(
            config,
            artifact_source,
            None,
            Some(Arc::from(settings_executor)),
            None,
        )
    }

    #[uniffi::constructor]
    pub fn open_with_native_capabilities(
        config: RuntimeConfig,
        artifact_source: Box<dyn ArtifactSource>,
        appearance_source: Box<dyn NativeAppearanceSource>,
        settings_executor: Box<dyn NativeSettingsExecutor>,
    ) -> Result<Arc<Self>, RuntimeOpenError> {
        open_runtime_controller(
            config,
            artifact_source,
            Some(Arc::from(appearance_source)),
            Some(Arc::from(settings_executor)),
            None,
        )
    }

    #[uniffi::constructor]
    pub fn open_with_all_native_capabilities(
        config: RuntimeConfig,
        artifact_source: Box<dyn ArtifactSource>,
        appearance_source: Box<dyn NativeAppearanceSource>,
        settings_executor: Box<dyn NativeSettingsExecutor>,
        inc_action_executor: Box<dyn NativeIncActionExecutor>,
    ) -> Result<Arc<Self>, RuntimeOpenError> {
        open_runtime_controller(
            config,
            artifact_source,
            Some(Arc::from(appearance_source)),
            Some(Arc::from(settings_executor)),
            Some(Arc::from(inc_action_executor)),
        )
    }
}

pub(super) fn open_runtime_controller(
    config: RuntimeConfig,
    artifact_source: Box<dyn ArtifactSource>,
    appearance_source: Option<Arc<dyn NativeAppearanceSource>>,
    settings_executor: Option<Arc<dyn NativeSettingsExecutor>>,
    inc_action_executor: Option<Arc<dyn NativeIncActionExecutor>>,
) -> Result<Arc<RuntimeController>, RuntimeOpenError> {
    let config = config.validated()?;
    let runtime_store = Arc::new(
        RuntimeStore::open(&config.runtime_store_path, StoreLimits::default()).map_err(
            |error| RuntimeOpenError::RuntimeStore {
                detail: error.to_string(),
            },
        )?,
    );
    let artifact_cache = Arc::new(
        FileArtifactCache::open(&config.artifact_cache_path).map_err(|error| {
            RuntimeOpenError::ArtifactCache {
                detail: error.to_string(),
            }
        })?,
    );
    let data_plane = Arc::new(
        NmpDataPlane::open(
            EngineConfig {
                store_path: config.nmp_store_path,
                indexer_relays: config.indexer_relays,
                app_relays: config.app_relays,
                fallback_relays: config.fallback_relays,
                allowed_local_relay_hosts: config.allowed_local_relay_hosts,
                max_relays: config.maximum_nmp_relays,
                ..EngineConfig::default()
            },
            config.maximum_bridge_workers,
        )
        .map_err(|error| RuntimeOpenError::Nmp {
            detail: error.to_string(),
        })?,
    );
    let catalog = Arc::new(
        RuntimeCatalogService::new(
            Arc::clone(&data_plane),
            Arc::clone(&artifact_cache),
            config.artifact_limits,
            config.maximum_manifest_bytes,
            config.maximum_blob_sources,
        )
        .map_err(|error| RuntimeOpenError::Runtime {
            detail: format!("catalog: {error}"),
        })?,
    );
    let diagnostics = Arc::new(RuntimeDiagnosticsService::new(&data_plane));
    let shell_provider = Arc::new(
        ShellProvider::new(
            Arc::new(RuntimeShellEnvironment),
            ShellProviderLimits::default(),
        )
        .map_err(|error| RuntimeOpenError::Runtime {
            detail: error.to_string(),
        })?,
    );
    let storage_provider: Arc<dyn Provider> = Arc::new(
        StorageProvider::new(Arc::clone(&runtime_store), StorageProviderLimits::default())
            .map_err(|error| RuntimeOpenError::Runtime {
                detail: error.to_string(),
            })?,
    );
    let identity_source: Arc<dyn IdentityDataPlane> = data_plane.clone();
    let identity_provider: Arc<dyn Provider> = IdentityProvider::connect(
        identity_source,
        Arc::new(NoopIdentityDiagnostics),
        IdentityProviderLimits::default(),
    )
    .map_err(|error| RuntimeOpenError::Runtime {
        detail: error.to_string(),
    })?;
    let inc_provider: Arc<dyn Provider> = match inc_action_executor {
        Some(callback) => Arc::new(
            IncProvider::with_native_actions(
                Arc::new(AllowAllIncAcl),
                Arc::new(NoopIncActivity),
                Arc::new(CallbackIncNativeActions { callback }),
                IncProviderLimits::default(),
            )
            .map_err(|error| RuntimeOpenError::Runtime {
                detail: error.to_string(),
            })?,
        ),
        None => Arc::new(
            IncProvider::new(
                Arc::new(AllowAllIncAcl),
                Arc::new(NoopIncActivity),
                IncProviderLimits::default(),
            )
            .map_err(|error| RuntimeOpenError::Runtime {
                detail: error.to_string(),
            })?,
        ),
    };
    let nostr_providers =
        NapNostrProviderSet::new(data_plane.clone(), NapNostrProviderLimits::default()).map_err(
            |error| RuntimeOpenError::Runtime {
                detail: error.to_string(),
            },
        )?;
    let outbox_provider: Arc<dyn Provider> = nostr_providers.outbox;
    let relay_provider: Arc<dyn Provider> = nostr_providers.relay;
    let (theme_source, theme_provider) = match appearance_source.and_then(|source| source.current())
    {
        Some(appearance) => {
            let snapshot =
                theme_from_appearance(appearance).map_err(|detail| RuntimeOpenError::Runtime {
                    detail: format!("native appearance source was invalid: {detail}"),
                })?;
            let source = Arc::new(RuntimeThemeSource::new(snapshot));
            let provider = Arc::new(
                ThemeProvider::new(source.clone(), ThemeProviderLimits::default()).map_err(
                    |error| RuntimeOpenError::Runtime {
                        detail: error.to_string(),
                    },
                )?,
            );
            (Some(source), Some(provider))
        }
        None => (None, None),
    };
    let config_provider = settings_executor
        .map(|callback| {
            ConfigProvider::new(
                Arc::clone(&runtime_store),
                Arc::new(CallbackSettingsExecutor { callback }),
                ConfigProviderLimits::default(),
            )
            .map(Arc::new)
            .map_err(|error| RuntimeOpenError::Runtime {
                detail: error.to_string(),
            })
        })
        .transpose()?;
    let mut providers = vec![
        storage_provider,
        identity_provider,
        inc_provider,
        outbox_provider,
        relay_provider,
    ];
    if let Some(provider) = &theme_provider {
        let provider: Arc<dyn Provider> = provider.clone();
        providers.push(provider);
    }
    if let Some(provider) = &config_provider {
        let provider: Arc<dyn Provider> = provider.clone();
        providers.push(provider);
    }
    let app_limits = AppLimits::default();
    let maximum_envelope_bytes = app_limits.maximum_envelope_bytes;
    let app = RuntimeApp::open(RuntimeAppConfig {
        limits: app_limits,
        resource_limits: ResourceLimits::default(),
        grant_limits: GrantLimits::default(),
        bridge_limits: BridgeLimits::default(),
        binding_limits: BindingLimits::default(),
        store: runtime_store.clone(),
        data_plane: data_plane.clone(),
        clock: Arc::new(SystemClock),
        shell_provider,
        providers,
    })
    .map_err(|error| RuntimeOpenError::Runtime {
        detail: error.to_string(),
    })?;
    let (signal, _) = watch::channel(0_u64);
    let controller = Arc::new(RuntimeController {
        app,
        data_plane,
        runtime_store,
        artifact_cache,
        catalog,
        diagnostics,
        artifact_source: CallbackArtifactSource {
            callback: Arc::from(artifact_source),
        },
        artifact_limits: config.artifact_limits,
        maximum_manifest_bytes: config.maximum_manifest_bytes,
        maximum_verified_read_bytes: config.maximum_verified_read_bytes,
        maximum_blob_sources: config.maximum_blob_sources,
        maximum_command_items: config.maximum_command_items,
        maximum_command_string_bytes: config.maximum_command_string_bytes,
        maximum_envelope_bytes,
        theme_source,
        theme_provider,
        config_provider,
        artifacts: Mutex::new(BTreeMap::new()),
        boundary_refusals: Mutex::new(BoundedFacts::with_capacity(config.maximum_boundary_events)),
        maximum_boundary_events: config.maximum_boundary_events,
        signal,
        observers: Arc::new(AtomicUsize::new(0)),
        maximum_observers: config.maximum_observers,
        permission_mode: config.permission_mode,
        closed: AtomicBool::new(false),
    });
    // Demo profiles are deliberately permissive for local end-to-end demos.
    // Re-apply that explicit policy to metadata restored from a prior process
    // too; otherwise a first run under interactive review can leave a denied
    // exact-build grant persisted forever, making NAP-OUTBOX appear absent on
    // the next demo launch. The helper remains a no-op for interactive
    // profiles and still binds every decision to the restored exact principal.
    controller.grant_demo_permissions_for_installed_builds();
    Ok(controller)
}
