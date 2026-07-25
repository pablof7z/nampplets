//! Referential integrity of one projected `RuntimeSnapshot`.
//!
//! Every platform binding needs the same fail-closed guarantee on a malformed
//! runtime snapshot. Re-deriving these checks inside each binding risks a
//! differently shaped failure on each platform, so they are asserted once here,
//! on the producer side, where a projection is a single consistent cut of one
//! `AppSnapshot`: the boundary either satisfies them by construction or the
//! controller records a typed `RuntimeRefusal` naming the exact violation.
//!
//! Only snapshot-internal invariants live here. Configured ceilings are policy
//! and are enforced where that policy is applied, not re-checked here.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use nmp_native_runtime_core::SessionState;

use crate::{RuntimeExactBuildCoordinate, RuntimeSnapshot};

/// Distinct projection faults one controller will surface as boundary
/// refusals. The latch behind it exists so a persistent fault cannot evict the
/// refusal ring; the cap keeps the latch itself finite.
pub(crate) const MAXIMUM_REPORTED_PROJECTION_FAULTS: usize = 64;

/// The lifecycle states a session may carry across the boundary.
///
/// `RuntimeSessionSnapshot.state` is the lowercased `Debug` spelling of
/// `SessionState`, and only sessions that reached `Running` are ever published
/// (`runtime-app` inserts an entry after the transition and removes it before
/// a terminal one).
pub(crate) const PUBLISHED_SESSION_STATES: [&str; 2] = ["running", "suspended"];

/// Compile-time guard for the list above: adding a `SessionState` variant stops
/// this crate from compiling until the new lifecycle state is reviewed against
/// `PUBLISHED_SESSION_STATES` and every binding's typed session projection.
const _: fn(SessionState) = |state| match state {
    SessionState::Launching
    | SessionState::Running
    | SessionState::Suspended
    | SessionState::Crashed
    | SessionState::Stopped => {}
};

/// One violated snapshot invariant, carrying the exact offending identity so
/// the recorded refusal never degrades into "something was wrong".
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
    /// Stable refusal code, one per invariant, so a binding or a log reader can
    /// tell the violations apart without parsing the detail text.
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

/// Exact-build identity as one orderable key, so membership stays logarithmic
/// on snapshots holding the full installed library.
type BuildKey<'a> = (&'a str, &'a str, &'a str);

pub(crate) fn build_key(coordinate: &RuntimeExactBuildCoordinate) -> BuildKey<'_> {
    (
        coordinate.manifest_author.as_str(),
        coordinate.d_tag.as_str(),
        coordinate.aggregate_hash.as_str(),
    )
}

/// All three coordinate fields stay present in every refusal; none of them ever
/// names a publisher/dTag pair without the verified aggregate.
pub(crate) fn key_label(key: BuildKey<'_>) -> String {
    format!("{}:{}:{}", key.0, key.1, key.2)
}

/// Checks one projected snapshot against every invariant a platform binding
/// would otherwise have to re-derive. Returns the first violation found, in a
/// deterministic order, so a repeated fault produces a repeatable refusal.
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
