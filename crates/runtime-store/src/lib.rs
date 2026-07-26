//! Persistent runtime metadata separate from NMP canonical state.
//!
//! This schema intentionally has no Nostr event, replacement, deletion,
//! routing, pending-row, or receipt-fact tables. Receipt identifiers are kept
//! only as workspace recovery references for NMP reattachment.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use nmp_native_runtime_core::{BoundedJson, CapabilityRequest, Principal, WriteReceiptId};
use parking_lot::Mutex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

mod activity;
mod components;
mod error;
mod grants;
mod installs;
mod preferences;
#[cfg(test)]
mod preferences_tests;
mod schema;
#[cfg(test)]
mod tests;
mod validate;
mod workspaces;

pub use error::StoreError;
pub use preferences::{
    MAXIMUM_PROFILE_RELAY_URL_BYTES, MAXIMUM_PROFILE_RELAYS_PER_LANE, PermissionDefaultPreference,
    ProfilePreferences,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoreLimits {
    pub maximum_installs: usize,
    pub maximum_install_title_bytes: usize,
    pub maximum_install_search_query_bytes: usize,
    pub maximum_grants_per_principal: usize,
    pub maximum_kv_keys_per_scope: usize,
    pub maximum_kv_bytes_per_scope: usize,
    pub maximum_value_bytes: usize,
    pub maximum_workspaces: usize,
    pub maximum_workspace_bytes: usize,
    pub maximum_workspace_assignments: usize,
    pub maximum_retained_receipts_per_workspace: usize,
    pub maximum_retained_receipt_bytes_per_workspace: usize,
    pub maximum_activity_facts: usize,
    pub maximum_activity_string_bytes: usize,
    pub maximum_activity_record_bytes: usize,
    pub maximum_activity_total_bytes: usize,
}

impl Default for StoreLimits {
    fn default() -> Self {
        Self {
            maximum_installs: 512,
            maximum_install_title_bytes: 512,
            maximum_install_search_query_bytes: 256,
            maximum_grants_per_principal: 64,
            maximum_kv_keys_per_scope: 1_024,
            maximum_kv_bytes_per_scope: 8 * 1024 * 1024,
            maximum_value_bytes: 512 * 1024,
            maximum_workspaces: 64,
            maximum_workspace_bytes: 512 * 1024,
            maximum_workspace_assignments: 512,
            maximum_retained_receipts_per_workspace: 256,
            maximum_retained_receipt_bytes_per_workspace: 64 * 1024,
            maximum_activity_facts: 10_000,
            maximum_activity_string_bytes: 512,
            maximum_activity_record_bytes: 1_024,
            maximum_activity_total_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Debug)]
pub struct RuntimeStore {
    path: PathBuf,
    limits: StoreLimits,
    connection: Mutex<Connection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledBuild {
    pub principal: Principal,
    pub title: Arc<str>,
    pub manifest_metadata: BoundedJson,
    pub capability_requests: Vec<CapabilityRequest>,
}

/// The exact runtime-owned state removed when one installed build is
/// uninstalled.
///
/// This policy deliberately excludes activity evidence, workspace definitions
/// and retained NMP receipt identifiers. It also cannot delete sealed artifact
/// bytes: those belong to the artifact resolver/cache, which must expose its
/// own exact-build deletion API before the application kernel can coordinate
/// that cleanup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UninstallCleanupPolicy {
    RuntimeOwnedExactBuildState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UninstallReport {
    pub installation_removed: bool,
    pub grants_removed: usize,
    pub component_values_removed: usize,
    pub workspace_assignments_removed: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRecord {
    pub id: Arc<str>,
    pub definition: BoundedJson,
    pub retained_receipts: Vec<WriteReceiptId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityRecord {
    pub principal: Principal,
    pub category: Arc<str>,
    pub operation: Arc<str>,
    pub outcome: Arc<str>,
    pub occurred_at_millis: u64,
}

impl RuntimeStore {
    pub fn open(path: impl AsRef<Path>, limits: StoreLimits) -> Result<Self, StoreError> {
        validate::validate_limits(limits)?;
        let path = path.as_ref().to_path_buf();
        let connection = Connection::open(&path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        schema::migrate(&connection)?;
        Ok(Self {
            path,
            limits,
            connection: Mutex::new(connection),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn table_names(&self) -> Result<Vec<String>, StoreError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }
}
