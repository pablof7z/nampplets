//! Installation, uninstallation, and installed-library projection.

use std::{collections::BTreeMap, sync::Arc, thread::JoinHandle};

use nmp_native_runtime_core::{Principal, SessionId, SessionState};
use nmp_native_runtime_store::{InstalledBuild, UninstallCleanupPolicy};

use super::{AppState, RuntimeApp, SessionEntry};
use crate::{
    commands::PlatformEvent,
    limits::ExecutableArtifact,
    views::{AppErrorCode, InstalledBuildAvailability, InstalledBuildView, InstalledLibraryView},
};

impl RuntimeApp {
    pub(super) fn install_verified(
        &self,
        state: &mut AppState,
        build: InstalledBuild,
        artifact: Arc<dyn ExecutableArtifact>,
        now: u64,
    ) {
        if artifact.manifest_kind() != 35_129 || artifact.d_tag().is_none() {
            let detail = match artifact.manifest_kind() {
                5_129 => {
                    "verified kind 5129 snapshot has no d tag; the pinned baseline does not yet define its exact-build runtime principal mapping"
                }
                15_129 => {
                    "verified kind 15129 root has no d tag; the pinned baseline does not yet define its exact-build runtime principal mapping"
                }
                _ => {
                    "verified artifact kind has no supported exact-build runtime principal mapping"
                }
            };
            self.refuse(
                state,
                AppErrorCode::UnsupportedManifestIdentity,
                Some(build.principal),
                None,
                detail,
                now,
            );
            return;
        }
        if artifact.manifest_author() != build.principal.manifest_author()
            || artifact.d_tag() != Some(build.principal.d_tag())
            || artifact.aggregate_hash() != build.principal.aggregate_hash()
        {
            self.refuse(
                state,
                AppErrorCode::ArtifactIdentityMismatch,
                Some(build.principal),
                None,
                "verified artifact aggregate does not match the exact principal",
                now,
            );
            return;
        }
        if !artifact.contains_logical_path(nmp_native_artifact::INDEX_PATH) {
            self.refuse(
                state,
                AppErrorCode::MissingIndex,
                Some(build.principal),
                None,
                "verified artifact has no /index.html",
                now,
            );
            return;
        }
        if !state.installed.contains_key(&build.principal)
            && state.installed.len() >= self.limits.maximum_installed_artifacts
        {
            self.refuse(
                state,
                AppErrorCode::Capacity,
                Some(build.principal),
                None,
                "installed artifact handle capacity is full",
                now,
            );
            return;
        }
        if let Err(error) = self.store.install(&build) {
            self.refuse_store(state, Some(build.principal), None, error, now);
            return;
        }
        let principal = build.principal.clone();
        state.installed.insert(principal.clone(), build);
        state.artifacts.insert(principal.clone(), artifact);
        self.record_activity(state, &principal, "install", "verified", "completed", now);
        self.push_event(state, PlatformEvent::Installed { principal });
    }

    pub(super) fn set_library_filter(&self, state: &mut AppState, query: Arc<str>, now: u64) {
        if query.len() > self.limits.maximum_library_query_bytes {
            self.refuse(
                state,
                AppErrorCode::Capacity,
                None,
                None,
                format!(
                    "library query is {} bytes; the maximum is {}",
                    query.len(),
                    self.limits.maximum_library_query_bytes
                ),
                now,
            );
            return;
        }
        if let Err(error) = self
            .store
            .search_installed_builds(&query, self.limits.maximum_installed_artifacts)
        {
            self.refuse_store(state, None, None, error, now);
            return;
        }
        state.library_query = Arc::clone(&query);
        self.push_event(state, PlatformEvent::LibraryFilterChanged { query });
    }

    pub(super) fn uninstall(
        &self,
        state: &mut AppState,
        principal: Principal,
        cleanup: UninstallCleanupPolicy,
        now: u64,
    ) -> Vec<JoinHandle<()>> {
        if !state.installed.contains_key(&principal) {
            self.refuse(
                state,
                AppErrorCode::NotInstalled,
                Some(principal),
                None,
                "uninstall target is not an installed exact build",
                now,
            );
            return Vec::new();
        }

        let sessions = state
            .sessions
            .iter()
            .filter_map(|(id, entry)| (entry.context.principal == principal).then_some(*id))
            .collect::<Vec<_>>();
        let mut joins = Vec::with_capacity(sessions.len());
        for session in sessions {
            if let Some(join) = self.end_session(state, session, SessionState::Stopped, None, now) {
                joins.push(join);
            }
        }

        let report = match self.store.uninstall_exact_build(&principal, cleanup) {
            Ok(report) => report,
            Err(error) => {
                self.refuse_store(state, Some(principal), None, error, now);
                return joins;
            }
        };

        for domain in self.bridge.advertised_domains() {
            self.bridge.revoke(&principal, &domain);
        }
        state.artifacts.remove(&principal);
        state.installed.remove(&principal);
        for assignments in state.workspace_assignments.values_mut() {
            assignments.remove(&principal);
        }
        self.record_activity(
            state,
            &principal,
            "install",
            "uninstall",
            "runtime-state-removed",
            now,
        );
        self.push_event(
            state,
            PlatformEvent::Uninstalled {
                principal,
                cleanup: report,
            },
        );
        joins
    }
}

pub(super) fn installed_library_view(
    installed: &BTreeMap<Principal, InstalledBuild>,
    artifacts: &BTreeMap<Principal, Arc<dyn ExecutableArtifact>>,
    sessions: &BTreeMap<SessionId, SessionEntry>,
    query: &str,
) -> InstalledLibraryView {
    let builds = installed
        .values()
        .filter(|build| {
            query.is_empty()
                || [
                    build.title.as_ref(),
                    build.principal.manifest_author(),
                    build.principal.d_tag(),
                    build.principal.aggregate_hash(),
                ]
                .iter()
                .any(|value| contains_library_search(value, query))
        })
        .map(|build| InstalledBuildView {
            build: build.clone(),
            availability: if artifacts.contains_key(&build.principal) {
                InstalledBuildAvailability::SealedExactBytesReady
            } else {
                InstalledBuildAvailability::MetadataOnly
            },
            active_sessions: sessions
                .iter()
                .filter_map(|(id, entry)| {
                    (entry.context.principal == build.principal).then_some(*id)
                })
                .collect(),
        })
        .collect();
    InstalledLibraryView {
        query: Arc::from(query),
        total_installed: installed.len(),
        builds,
    }
}

pub(super) fn contains_library_search(value: &str, query: &str) -> bool {
    query.is_empty()
        || value
            .as_bytes()
            .windows(query.len())
            .any(|window| window.eq_ignore_ascii_case(query.as_bytes()))
}
