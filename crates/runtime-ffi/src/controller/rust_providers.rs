//! Platform-neutral runtime construction with Rust-native providers.

use std::sync::Arc;

use nmp_native_nap_bridge::Provider;

use super::{RuntimeController, open::open_runtime_controller};
use crate::{ArtifactSource, NativeSettingsExecutor, RuntimeConfig, RuntimeOpenError};

impl RuntimeController {
    /// Opens the runtime with additional Rust-native NAP providers.
    ///
    /// This is the platform-neutral composition seam for providers whose raw
    /// OS capabilities are implemented by a Rust host. Provider descriptors,
    /// duplicate domains, dependencies, and registry capacity remain
    /// validated by the ordinary runtime registry. The supplied providers do
    /// not bypass exact-build permission review or session lifecycle.
    pub fn open_with_rust_providers(
        config: RuntimeConfig,
        artifact_source: Box<dyn ArtifactSource>,
        providers: Vec<Arc<dyn Provider>>,
    ) -> Result<Arc<Self>, RuntimeOpenError> {
        open_runtime_controller(config, artifact_source, None, None, None, None, providers)
    }

    /// Opens the runtime with native settings and additional Rust-native NAP providers.
    ///
    /// This preserves the ordinary `config` provider while allowing a Rust
    /// platform host to compose providers such as `resource`. Every provider
    /// still passes through the same descriptor, permission, and lifecycle
    /// checks as the built-in providers.
    pub fn open_with_settings_and_rust_providers(
        config: RuntimeConfig,
        artifact_source: Box<dyn ArtifactSource>,
        settings_executor: Box<dyn NativeSettingsExecutor>,
        providers: Vec<Arc<dyn Provider>>,
    ) -> Result<Arc<Self>, RuntimeOpenError> {
        open_runtime_controller(
            config,
            artifact_source,
            None,
            Some(Arc::from(settings_executor)),
            None,
            None,
            providers,
        )
    }
}
