//! Native-supplied runtime configuration and its validated Rust-owned form.

use nmp_native_artifact::ArtifactLimits;

use crate::{
    DEFAULT_MAXIMUM_ARTIFACT_READ_BYTES, DEFAULT_MAXIMUM_BOUNDARY_EVENTS,
    DEFAULT_MAXIMUM_CONFIG_ITEMS, DEFAULT_MAXIMUM_CONFIG_STRING_BYTES,
    DEFAULT_MAXIMUM_MANIFEST_BYTES, DEFAULT_MAXIMUM_OBSERVERS,
    relay_lane::{DroppedRelay, admit_lane},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimePermissionDefault {
    AskEveryTime,
    AllowSession,
    AllowExactBuild,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeConfig {
    pub runtime_store_path: String,
    pub nmp_store_path: Option<String>,
    pub artifact_cache_path: String,
    pub indexer_relays: Vec<String>,
    pub app_relays: Vec<String>,
    pub fallback_relays: Vec<String>,
    pub allowed_local_relay_hosts: Vec<String>,
    pub maximum_nmp_relays: u64,
    pub maximum_bridge_workers: u64,
    pub maximum_observers: u64,
    pub maximum_boundary_events: u64,
    pub maximum_config_items: u64,
    pub maximum_config_string_bytes: u64,
    pub maximum_manifest_bytes: u64,
    pub maximum_artifact_files: u64,
    pub maximum_artifact_file_bytes: u64,
    pub maximum_artifact_total_bytes: u64,
    pub maximum_verified_read_bytes: u64,
    pub maximum_blob_sources: u64,
    pub permission_default: RuntimePermissionDefault,
}

impl RuntimeConfig {
    pub(crate) fn validated(self) -> Result<ValidatedConfig, RuntimeOpenError> {
        let maximum_config_items =
            nonzero_usize(self.maximum_config_items, "maximum_config_items")?;
        let maximum_config_string_bytes = nonzero_usize(
            self.maximum_config_string_bytes,
            "maximum_config_string_bytes",
        )?;
        let maximum_observers = nonzero_usize(self.maximum_observers, "maximum_observers")?;
        let maximum_boundary_events =
            nonzero_usize(self.maximum_boundary_events, "maximum_boundary_events")?;
        let maximum_manifest_bytes =
            nonzero_usize(self.maximum_manifest_bytes, "maximum_manifest_bytes")?;
        let maximum_verified_read_bytes = nonzero_usize(
            self.maximum_verified_read_bytes,
            "maximum_verified_read_bytes",
        )?;
        let maximum_bridge_workers =
            nonzero_usize(self.maximum_bridge_workers, "maximum_bridge_workers")?;
        let maximum_blob_sources =
            nonzero_usize(self.maximum_blob_sources, "maximum_blob_sources")?;
        let artifact_limits = ArtifactLimits {
            maximum_files: nonzero_usize(self.maximum_artifact_files, "maximum_artifact_files")?,
            maximum_file_bytes: nonzero_usize(
                self.maximum_artifact_file_bytes,
                "maximum_artifact_file_bytes",
            )?,
            maximum_total_bytes: nonzero_usize(
                self.maximum_artifact_total_bytes,
                "maximum_artifact_total_bytes",
            )?,
        };
        let maximum_nmp_relays = nonzero_usize(self.maximum_nmp_relays, "maximum_nmp_relays")?;
        validate_string(
            "runtime_store_path",
            &self.runtime_store_path,
            maximum_config_string_bytes,
        )?;
        validate_string(
            "artifact_cache_path",
            &self.artifact_cache_path,
            maximum_config_string_bytes,
        )?;
        if let Some(path) = &self.nmp_store_path {
            validate_string("nmp_store_path", path, maximum_config_string_bytes)?;
        }
        for (name, values) in [
            ("indexer_relays", &self.indexer_relays),
            ("app_relays", &self.app_relays),
            ("fallback_relays", &self.fallback_relays),
            ("allowed_local_relay_hosts", &self.allowed_local_relay_hosts),
        ] {
            if values.len() > maximum_config_items {
                return Err(RuntimeOpenError::InvalidConfig {
                    detail: format!(
                        "{name} has {} items; the configured maximum is {maximum_config_items}",
                        values.len()
                    ),
                });
            }
            for value in values {
                validate_string(name, value, maximum_config_string_bytes)?;
            }
        }

        // Operator relay lanes are judged here, once, in Rust. They used to
        // be filtered by each host before they ever arrived, which meant the
        // scheme, credential and duplicate rules lived in the shell and a
        // second host had to reproduce them exactly to route the same way.
        let (indexer_relays, mut dropped_relays) =
            admit_lane("indexer", &self.indexer_relays, maximum_nmp_relays);
        let (app_relays, dropped_app) = admit_lane("app", &self.app_relays, maximum_nmp_relays);
        let (fallback_relays, dropped_fallback) =
            admit_lane("fallback", &self.fallback_relays, maximum_nmp_relays);
        dropped_relays.extend(dropped_app);
        dropped_relays.extend(dropped_fallback);
        // Degrading a lane is one thing; emptying it is another. A lane that
        // was configured and survives with nothing left is a runtime routing
        // through no relays at all while every other signal reads healthy --
        // so it refuses instead, naming what it could not admit.
        for (lane, configured, admitted) in [
            ("indexer", &self.indexer_relays, &indexer_relays),
            ("app", &self.app_relays, &app_relays),
            ("fallback", &self.fallback_relays, &fallback_relays),
        ] {
            if !configured.is_empty() && admitted.is_empty() {
                let reasons = dropped_relays
                    .iter()
                    .filter(|dropped| dropped.lane == lane)
                    .map(DroppedRelay::detail)
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(RuntimeOpenError::InvalidConfig {
                    detail: format!(
                        "every configured {lane} relay was refused, leaving that \
                         lane empty: {reasons}"
                    ),
                });
            }
        }

        Ok(ValidatedConfig {
            runtime_store_path: self.runtime_store_path,
            nmp_store_path: self.nmp_store_path,
            artifact_cache_path: self.artifact_cache_path,
            indexer_relays,
            app_relays,
            fallback_relays,
            dropped_relays,
            allowed_local_relay_hosts: self.allowed_local_relay_hosts,
            maximum_nmp_relays,
            maximum_bridge_workers,
            maximum_observers,
            maximum_boundary_events,
            maximum_manifest_bytes,
            artifact_limits,
            maximum_verified_read_bytes,
            maximum_blob_sources,
            maximum_command_items: maximum_config_items,
            maximum_command_string_bytes: maximum_config_string_bytes,
            permission_default: self.permission_default,
        })
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            runtime_store_path: "runtime.sqlite3".to_owned(),
            nmp_store_path: Some("nmp.redb".to_owned()),
            artifact_cache_path: "artifacts".to_owned(),
            indexer_relays: Vec::new(),
            app_relays: Vec::new(),
            fallback_relays: Vec::new(),
            allowed_local_relay_hosts: Vec::new(),
            maximum_nmp_relays: 64,
            maximum_bridge_workers: 12,
            maximum_observers: DEFAULT_MAXIMUM_OBSERVERS,
            maximum_boundary_events: DEFAULT_MAXIMUM_BOUNDARY_EVENTS,
            maximum_config_items: DEFAULT_MAXIMUM_CONFIG_ITEMS,
            maximum_config_string_bytes: DEFAULT_MAXIMUM_CONFIG_STRING_BYTES,
            maximum_manifest_bytes: DEFAULT_MAXIMUM_MANIFEST_BYTES,
            maximum_artifact_files: 256,
            maximum_artifact_file_bytes: DEFAULT_MAXIMUM_ARTIFACT_READ_BYTES,
            maximum_artifact_total_bytes: 32 * 1_024 * 1_024,
            maximum_verified_read_bytes: DEFAULT_MAXIMUM_ARTIFACT_READ_BYTES,
            maximum_blob_sources: 8,
            permission_default: RuntimePermissionDefault::AskEveryTime,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ValidatedConfig {
    pub(crate) runtime_store_path: String,
    pub(crate) nmp_store_path: Option<String>,
    pub(crate) artifact_cache_path: String,
    pub(crate) indexer_relays: Vec<String>,
    pub(crate) app_relays: Vec<String>,
    pub(crate) fallback_relays: Vec<String>,
    /// Relays the runtime would not admit, each with its reason. Carried so
    /// open can record them as evidence rather than let them disappear.
    pub(crate) dropped_relays: Vec<DroppedRelay>,
    pub(crate) allowed_local_relay_hosts: Vec<String>,
    pub(crate) maximum_nmp_relays: usize,
    pub(crate) maximum_bridge_workers: usize,
    pub(crate) maximum_observers: usize,
    pub(crate) maximum_boundary_events: usize,
    pub(crate) maximum_manifest_bytes: usize,
    pub(crate) artifact_limits: ArtifactLimits,
    pub(crate) maximum_verified_read_bytes: usize,
    pub(crate) maximum_blob_sources: usize,
    pub(crate) maximum_command_items: usize,
    pub(crate) maximum_command_string_bytes: usize,
    pub(crate) permission_default: RuntimePermissionDefault,
}

#[derive(Clone, Debug, thiserror::Error, uniffi::Error)]
pub enum RuntimeOpenError {
    #[error("invalid runtime configuration: {detail}")]
    InvalidConfig { detail: String },
    #[error("runtime storage could not be opened: {detail}")]
    RuntimeStore { detail: String },
    #[error("artifact cache could not be opened: {detail}")]
    ArtifactCache { detail: String },
    #[error("NMP data plane could not be opened: {detail}")]
    Nmp { detail: String },
    #[error("runtime kernel could not be opened: {detail}")]
    Runtime { detail: String },
}

fn validate_string(name: &str, value: &str, maximum: usize) -> Result<(), RuntimeOpenError> {
    if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        return Err(RuntimeOpenError::InvalidConfig {
            detail: format!("{name} must be non-empty, control-free, and at most {maximum} bytes"),
        });
    }
    Ok(())
}

fn nonzero_usize(value: u64, name: &str) -> Result<usize, RuntimeOpenError> {
    usize::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| RuntimeOpenError::InvalidConfig {
            detail: format!("{name} must fit usize and be non-zero"),
        })
}
