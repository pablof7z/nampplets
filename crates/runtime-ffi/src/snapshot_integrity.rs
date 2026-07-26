//! Referential integrity of one projected `RuntimeSnapshot`.
//!
//! Every platform binding needs the same fail-closed guarantee on a malformed
//! runtime snapshot. These checks therefore run once on the producer side,
//! against the exact record that would otherwise cross the FFI boundary.
//! Configured ceilings remain policy and are enforced where they are applied.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use nmp_native_runtime_core::SessionState;

use crate::{RuntimeExactBuildCoordinate, RuntimeSnapshot};

pub(crate) const MAXIMUM_REPORTED_PROJECTION_FAULTS: usize = 64;

/// The lifecycle states a session may carry across the boundary.
pub(crate) const PUBLISHED_SESSION_STATES: [&str; 2] = ["running", "suspended"];

/// Adding a kernel lifecycle state requires an explicit boundary review.
const _: fn(SessionState) = |state| match state {
    SessionState::Launching
    | SessionState::Running
    | SessionState::Suspended
    | SessionState::Crashed
    | SessionState::Stopped => {}
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SnapshotIntegrityViolation {
    TotalInstalledBelowVisible {
        total_installed: u64,
        visible_builds: usize,
    },
    DuplicateSession {
        session_id: u64,
    },
    UnsupportedSessionState {
        session_id: u64,
        raw_value: String,
    },
    DuplicateWorkspace {
        workspace_id: String,
    },
    DuplicateBuild {
        build: String,
    },
    DuplicateBuildSession {
        build: String,
        session_id: u64,
    },
    MissingBuildSession {
        build: String,
        session_id: u64,
    },
    MismatchedBuildSession {
        build: String,
        session_id: u64,
        session_build: String,
    },
    DuplicateWorkspaceAssignment {
        build: String,
        workspace_id: String,
    },
    MissingWorkspaceAssignment {
        build: String,
        workspace_id: String,
    },
}

impl SnapshotIntegrityViolation {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::TotalInstalledBelowVisible { .. } => "snapshot-integrity-total-installed",
            Self::DuplicateSession { .. } => "snapshot-integrity-duplicate-session",
            Self::UnsupportedSessionState { .. } => "snapshot-integrity-session-state",
            Self::DuplicateWorkspace { .. } => "snapshot-integrity-duplicate-workspace",
            Self::DuplicateBuild { .. } => "snapshot-integrity-duplicate-build",
            Self::DuplicateBuildSession { .. } => "snapshot-integrity-duplicate-build-session",
            Self::MissingBuildSession { .. } => "snapshot-integrity-missing-build-session",
            Self::MismatchedBuildSession { .. } => "snapshot-integrity-mismatched-build-session",
            Self::DuplicateWorkspaceAssignment { .. } => {
                "snapshot-integrity-duplicate-workspace-assignment"
            }
            Self::MissingWorkspaceAssignment { .. } => {
                "snapshot-integrity-missing-workspace-assignment"
            }
        }
    }
}

impl fmt::Display for SnapshotIntegrityViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TotalInstalledBelowVisible {
                total_installed,
                visible_builds,
            } => write!(
                formatter,
                "the snapshot reports {total_installed} total installs but projects \
                 {visible_builds} visible builds"
            ),
            Self::DuplicateSession { session_id } => {
                write!(formatter, "session {session_id} appears more than once")
            }
            Self::UnsupportedSessionState {
                session_id,
                raw_value,
            } => write!(
                formatter,
                "session {session_id} carries unpublishable state {raw_value}"
            ),
            Self::DuplicateWorkspace { workspace_id } => {
                write!(formatter, "workspace {workspace_id} appears more than once")
            }
            Self::DuplicateBuild { build } => {
                write!(formatter, "exact build {build} appears more than once")
            }
            Self::DuplicateBuildSession { build, session_id } => write!(
                formatter,
                "exact build {build} references session {session_id} more than once"
            ),
            Self::MissingBuildSession { build, session_id } => write!(
                formatter,
                "exact build {build} references missing session {session_id}"
            ),
            Self::MismatchedBuildSession {
                build,
                session_id,
                session_build,
            } => write!(
                formatter,
                "session {session_id} belongs to {session_build}, not {build}"
            ),
            Self::DuplicateWorkspaceAssignment {
                build,
                workspace_id,
            } => write!(
                formatter,
                "exact build {build} repeats workspace {workspace_id}"
            ),
            Self::MissingWorkspaceAssignment {
                build,
                workspace_id,
            } => write!(
                formatter,
                "exact build {build} references missing workspace {workspace_id}"
            ),
        }
    }
}

type BuildKey<'a> = (&'a str, &'a str, &'a str);

pub(crate) fn build_key(coordinate: &RuntimeExactBuildCoordinate) -> BuildKey<'_> {
    (
        coordinate.manifest_author.as_str(),
        coordinate.d_tag.as_str(),
        coordinate.aggregate_hash.as_str(),
    )
}

pub(crate) fn key_label(key: BuildKey<'_>) -> String {
    format!("{}:{}:{}", key.0, key.1, key.2)
}

/// Returns the first violation in deterministic projection order.
pub(crate) fn check_snapshot_integrity(
    snapshot: &RuntimeSnapshot,
) -> Result<(), SnapshotIntegrityViolation> {
    let library = &snapshot.installed_library;
    if library.total_installed < library.builds.len() as u64 {
        return Err(SnapshotIntegrityViolation::TotalInstalledBelowVisible {
            total_installed: library.total_installed,
            visible_builds: library.builds.len(),
        });
    }

    let mut sessions: BTreeMap<u64, BuildKey<'_>> = BTreeMap::new();
    for session in &snapshot.sessions {
        if !PUBLISHED_SESSION_STATES.contains(&session.state.as_str()) {
            return Err(SnapshotIntegrityViolation::UnsupportedSessionState {
                session_id: session.id,
                raw_value: session.state.clone(),
            });
        }
        let identity = (
            session.author.as_str(),
            session.d_tag.as_str(),
            session.aggregate_hash.as_str(),
        );
        if sessions.insert(session.id, identity).is_some() {
            return Err(SnapshotIntegrityViolation::DuplicateSession {
                session_id: session.id,
            });
        }
    }

    let mut workspace_ids: BTreeSet<&str> = BTreeSet::new();
    for workspace in &snapshot.workspaces {
        if !workspace_ids.insert(workspace.workspace_id.as_str()) {
            return Err(SnapshotIntegrityViolation::DuplicateWorkspace {
                workspace_id: workspace.workspace_id.clone(),
            });
        }
    }

    let mut coordinates: BTreeSet<BuildKey<'_>> = BTreeSet::new();
    for build in &library.builds {
        let key = build_key(&build.coordinate);
        if !coordinates.insert(key) {
            return Err(SnapshotIntegrityViolation::DuplicateBuild {
                build: key_label(key),
            });
        }

        let mut seen_sessions: BTreeSet<u64> = BTreeSet::new();
        for session_id in build.active_session_ids.iter().copied() {
            if !seen_sessions.insert(session_id) {
                return Err(SnapshotIntegrityViolation::DuplicateBuildSession {
                    build: key_label(key),
                    session_id,
                });
            }
            let Some(session_key) = sessions.get(&session_id) else {
                return Err(SnapshotIntegrityViolation::MissingBuildSession {
                    build: key_label(key),
                    session_id,
                });
            };
            if *session_key != key {
                return Err(SnapshotIntegrityViolation::MismatchedBuildSession {
                    build: key_label(key),
                    session_id,
                    session_build: key_label(*session_key),
                });
            }
        }

        let mut seen_workspaces: BTreeSet<&str> = BTreeSet::new();
        for workspace_id in &build.assigned_workspace_ids {
            if !seen_workspaces.insert(workspace_id.as_str()) {
                return Err(SnapshotIntegrityViolation::DuplicateWorkspaceAssignment {
                    build: key_label(key),
                    workspace_id: workspace_id.clone(),
                });
            }
            if !workspace_ids.contains(workspace_id.as_str()) {
                return Err(SnapshotIntegrityViolation::MissingWorkspaceAssignment {
                    build: key_label(key),
                    workspace_id: workspace_id.clone(),
                });
            }
        }
    }

    Ok(())
}
