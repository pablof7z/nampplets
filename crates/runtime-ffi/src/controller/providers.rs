//! Native-sourced provider pushes: appearance changes and settings commits.

use std::sync::atomic::Ordering;

use nmp_native_providers::ConfigProviderLimits;
use nmp_native_runtime_core::{Principal, SessionId, SessionState};

use super::RuntimeController;
use crate::{
    NativeAppearanceSnapshot, NativeConfigCommit, RuntimeProviderUpdate,
    projection::theme_from_appearance,
};

#[uniffi::export]
impl RuntimeController {
    /// Applies one event-driven native appearance change. The source is a
    /// single latest value; provider delivery uses finite conflating lanes.
    pub fn update_appearance(&self, appearance: NativeAppearanceSnapshot) -> RuntimeProviderUpdate {
        if self.closed.load(Ordering::Acquire) {
            return self.provider_refusal("closed", "runtime is closed");
        }
        let (Some(source), Some(provider)) = (&self.theme_source, &self.theme_provider) else {
            return self.provider_refusal(
                "theme-unavailable",
                "no native appearance source was registered",
            );
        };
        let snapshot = match theme_from_appearance(appearance) {
            Ok(snapshot) => snapshot,
            Err(detail) => return self.provider_refusal("invalid-appearance", detail),
        };
        source.replace(snapshot.clone());
        match provider.publish_changed(&snapshot) {
            Ok(report) => RuntimeProviderUpdate::accepted(report),
            Err(error) => self.provider_refusal("theme-push-refused", error.to_string()),
        }
    }

    /// Trusted native settings commit. Rust rechecks the exact active session,
    /// exact-build principal, schema, values, and persistence before pushing.
    pub fn commit_config_values(&self, commit: NativeConfigCommit) -> RuntimeProviderUpdate {
        if self.closed.load(Ordering::Acquire) {
            return self.provider_refusal("closed", "runtime is closed");
        }
        let Some(provider) = &self.config_provider else {
            return self.provider_refusal(
                "config-unavailable",
                "no native settings executor was registered",
            );
        };
        if commit.values_json.len() > ConfigProviderLimits::default().maximum_values_bytes {
            return self.provider_refusal(
                "config-values-too-large",
                "native settings values exceed the configured byte limit",
            );
        }
        let principal =
            match Principal::new(commit.manifest_author, commit.d_tag, commit.aggregate_hash) {
                Ok(principal) => principal,
                Err(error) => return self.provider_refusal("invalid-principal", error.to_string()),
            };
        let session = SessionId(commit.session_id);
        let active = self.app.snapshot().sessions.iter().any(|candidate| {
            candidate.id == session
                && candidate.principal == principal
                && candidate.state == SessionState::Running
        });
        if !active {
            return self.provider_refusal(
                "settings-session-closed",
                "the exact settings session is no longer running",
            );
        }
        let values = match serde_json::from_str(&commit.values_json) {
            Ok(values) => values,
            Err(_) => {
                return self.provider_refusal(
                    "invalid-config-values",
                    "native settings returned invalid JSON",
                );
            }
        };
        match provider.commit_values(&principal, &values) {
            Ok(report) => RuntimeProviderUpdate::accepted(report),
            Err(error) => self.provider_refusal("config-commit-refused", error.to_string()),
        }
    }
}
