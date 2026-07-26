//! Profile preferences, honest storage totals, and coordinated NMP cache reset.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

use nmp::{Engine, EngineError};

use super::RuntimeController;
use crate::{
    RuntimeProfilePreferences, RuntimeProfilePreferencesUpdate, RuntimeStorageResetResult,
    RuntimeStorageSnapshot,
    profile_preferences::{project_profile_preferences, validate_profile_preferences},
};

const MAXIMUM_STORAGE_ENTRIES: usize = 1_024;

#[uniffi::export]
impl RuntimeController {
    pub fn profile_preferences(&self) -> RuntimeProfilePreferences {
        self.profile_preferences.lock().clone()
    }

    pub fn update_profile_preferences(
        &self,
        preferences: RuntimeProfilePreferences,
    ) -> RuntimeProfilePreferencesUpdate {
        if self.closed.load(Ordering::Acquire) {
            return RuntimeProfilePreferencesUpdate {
                applied: false,
                restart_required: false,
                preferences: None,
                refusal: Some(self.refusal("closed", "the app profile is closed")),
            };
        }
        let validated = match validate_profile_preferences(preferences) {
            Ok(preferences) => preferences,
            Err(detail) => {
                return RuntimeProfilePreferencesUpdate {
                    applied: false,
                    restart_required: false,
                    preferences: None,
                    refusal: Some(self.refusal("invalid-preferences", detail)),
                };
            }
        };
        let projected = project_profile_preferences(&validated);
        let restart_required = *self.profile_preferences.lock() != projected;
        if let Err(error) = self.runtime_store.save_profile_preferences(&validated) {
            return RuntimeProfilePreferencesUpdate {
                applied: false,
                restart_required: false,
                preferences: None,
                refusal: Some(self.refusal("preferences-store", error.to_string())),
            };
        }
        *self.profile_preferences.lock() = projected.clone();
        RuntimeProfilePreferencesUpdate {
            applied: true,
            restart_required,
            preferences: Some(projected),
            refusal: None,
        }
    }

    /// Returns bounded filesystem facts, never a claim that another process or
    /// future file is represented. `incomplete` is true when enumeration was
    /// refused, failed, or hit its finite entry ceiling.
    pub fn storage_snapshot(&self) -> RuntimeStorageSnapshot {
        let (nmp_cache_bytes, nmp_incomplete) = self
            .nmp_store_path
            .as_deref()
            .map(file_bytes)
            .unwrap_or((0, false));
        let runtime_path = self.runtime_store.path();
        let (runtime_bytes, runtime_incomplete) = sqlite_family_bytes(runtime_path);
        let (artifact_bytes, artifact_incomplete) = tree_bytes(&self.artifact_cache_path);
        let app_data_bytes = runtime_bytes.saturating_add(artifact_bytes);
        RuntimeStorageSnapshot {
            nmp_cache_bytes,
            app_data_bytes,
            total_bytes: nmp_cache_bytes.saturating_add(app_data_bytes),
            incomplete: nmp_incomplete || runtime_incomplete || artifact_incomplete,
        }
    }

    /// Closes every runtime session and the NMP engine before asking NMP's
    /// supported facade to remove its own persistent store.
    pub fn reset_nmp_cache(&self) -> RuntimeStorageResetResult {
        let Some(path) = self.nmp_store_path.as_deref() else {
            return RuntimeStorageResetResult {
                reset: false,
                refusal: Some(self.refusal(
                    "nmp-cache-unavailable",
                    "this app profile does not use a persistent network cache",
                )),
            };
        };
        self.close();
        match Engine::reset_persistent_store(path) {
            Ok(()) => RuntimeStorageResetResult {
                reset: true,
                refusal: None,
            },
            Err(error) => RuntimeStorageResetResult {
                reset: false,
                refusal: Some(self.refusal(reset_error_code(&error), error.to_string())),
            },
        }
    }
}

fn reset_error_code(error: &EngineError) -> &'static str {
    match error {
        EngineError::StoreStillOpen { .. } => "nmp-cache-still-open",
        EngineError::StoreResetFailed { .. } => "nmp-cache-reset",
        _ => "nmp-cache-reset",
    }
}

fn sqlite_family_bytes(path: &Path) -> (u64, bool) {
    let mut total = 0_u64;
    let mut incomplete = false;
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ] {
        let (bytes, failed) = file_bytes(&candidate);
        total = total.saturating_add(bytes);
        incomplete |= failed;
    }
    (total, incomplete)
}

fn file_bytes(path: &Path) -> (u64, bool) {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => (metadata.len(), false),
        Ok(metadata) if metadata.file_type().is_symlink() => (0, true),
        Ok(_) => (0, false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (0, false),
        Err(_) => (0, true),
    }
}

fn tree_bytes(root: &Path) -> (u64, bool) {
    let mut pending = vec![root.to_path_buf()];
    let mut visited = 0_usize;
    let mut total = 0_u64;
    let mut incomplete = false;
    while let Some(path) = pending.pop() {
        if visited >= MAXIMUM_STORAGE_ENTRIES {
            incomplete = true;
            break;
        }
        visited += 1;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                incomplete = true;
                continue;
            }
        };
        if metadata.file_type().is_symlink() {
            incomplete = true;
        } else if metadata.is_file() {
            total = total.saturating_add(metadata.len());
        } else if metadata.is_dir() {
            match fs::read_dir(&path) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(entry) => pending.push(entry.path()),
                            Err(_) => incomplete = true,
                        }
                    }
                }
                Err(_) => incomplete = true,
            }
        }
    }
    (total, incomplete)
}
