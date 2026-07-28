//! Producer-side referential integrity for projected snapshots.

use super::*;
use crate::snapshot_integrity::{SnapshotIntegrityViolation, check_snapshot_integrity};

fn coordinate(d_tag: &str) -> RuntimeExactBuildCoordinate {
    RuntimeExactBuildCoordinate {
        manifest_author: AUTHOR.to_owned(),
        d_tag: d_tag.to_owned(),
        aggregate_hash: "c".repeat(64),
    }
}

fn build(coordinate: RuntimeExactBuildCoordinate) -> RuntimeInstalledBuildSnapshot {
    RuntimeInstalledBuildSnapshot {
        coordinate,
        title: "Good Morning".to_owned(),
        manifest_metadata_json: "{}".to_owned(),
        availability: RuntimeInstalledBuildAvailability::MetadataOnly,
        active_session_ids: Vec::new(),
        assigned_workspace_ids: Vec::new(),
    }
}

fn session(
    id: u64,
    coordinate: &RuntimeExactBuildCoordinate,
    state: &str,
) -> RuntimeSessionSnapshot {
    RuntimeSessionSnapshot {
        id,
        author: coordinate.manifest_author.clone(),
        d_tag: coordinate.d_tag.clone(),
        aggregate_hash: coordinate.aggregate_hash.clone(),
        profile: RuntimeExecutionProfile::Legacy,
        state: state.to_owned(),
        domains: Vec::new(),
        unavailable_domains: Vec::new(),
    }
}

fn snapshot(
    builds: Vec<RuntimeInstalledBuildSnapshot>,
    sessions: Vec<RuntimeSessionSnapshot>,
    workspaces: Vec<RuntimeWorkspaceDefinition>,
) -> RuntimeSnapshot {
    RuntimeSnapshot {
        revision: 1,
        closed: false,
        installed_library: RuntimeInstalledLibrarySnapshot {
            query: String::new(),
            total_installed: builds.len() as u64,
            builds,
        },
        sessions,
        bindings: Vec::new(),
        pending_writes: Vec::new(),
        receipts: Vec::new(),
        workspaces,
        recent_activity: Vec::new(),
        dropped_activity: 0,
        recent_errors: Vec::new(),
        dropped_errors: 0,
        boundary_refusals: Vec::new(),
        refused_operator_relays: Vec::new(),
        dropped_boundary_refusals: 0,
        active_resources: 0,
        resource_high_watermark: 0,
        resource_refusal_count: 0,
    }
}

#[test]
fn a_consistent_snapshot_satisfies_every_invariant() {
    let first = coordinate("good-morning");
    let second = coordinate("second");
    let mut running = build(first.clone());
    running.active_session_ids = vec![7, 8];
    running.assigned_workspace_ids = vec!["primary".to_owned()];
    let projection = snapshot(
        vec![running, build(second.clone())],
        vec![
            session(7, &first, "running"),
            session(8, &first, "suspended"),
            session(9, &second, "running"),
        ],
        vec![workspace_definition("primary")],
    );

    assert_eq!(check_snapshot_integrity(&projection), Ok(()));
}

#[test]
fn global_duplicate_and_state_invariants_name_the_offending_row() {
    let first = coordinate("good-morning");
    let second = coordinate("second");
    let mut below = snapshot(vec![build(first.clone())], Vec::new(), Vec::new());
    below.installed_library.total_installed = 0;
    assert_eq!(
        check_snapshot_integrity(&below),
        Err(SnapshotIntegrityViolation::TotalInstalledBelowVisible {
            total_installed: 0,
            visible_builds: 1,
        })
    );
    assert_eq!(
        check_snapshot_integrity(&snapshot(
            Vec::new(),
            vec![
                session(7, &first, "running"),
                session(7, &second, "running")
            ],
            Vec::new(),
        )),
        Err(SnapshotIntegrityViolation::DuplicateSession { session_id: 7 })
    );
    assert_eq!(
        check_snapshot_integrity(&snapshot(
            Vec::new(),
            vec![session(7, &first, "launching")],
            Vec::new(),
        )),
        Err(SnapshotIntegrityViolation::UnsupportedSessionState {
            session_id: 7,
            raw_value: "launching".to_owned(),
        })
    );
    assert_eq!(
        check_snapshot_integrity(&snapshot(
            Vec::new(),
            Vec::new(),
            vec![
                workspace_definition("primary"),
                workspace_definition("primary")
            ],
        )),
        Err(SnapshotIntegrityViolation::DuplicateWorkspace {
            workspace_id: "primary".to_owned(),
        })
    );
}

#[test]
fn build_cross_reference_invariants_name_exact_builds() {
    let first = coordinate("good-morning");
    let second = coordinate("second");
    let label = format!(
        "{}:{}:{}",
        first.manifest_author, first.d_tag, first.aggregate_hash
    );
    assert_eq!(
        check_snapshot_integrity(&snapshot(
            vec![build(first.clone()), build(first.clone())],
            Vec::new(),
            Vec::new(),
        )),
        Err(SnapshotIntegrityViolation::DuplicateBuild {
            build: label.clone(),
        })
    );

    let mut repeated = build(first.clone());
    repeated.active_session_ids = vec![7, 7];
    assert_eq!(
        check_snapshot_integrity(&snapshot(
            vec![repeated],
            vec![session(7, &first, "running")],
            Vec::new(),
        )),
        Err(SnapshotIntegrityViolation::DuplicateBuildSession {
            build: label.clone(),
            session_id: 7,
        })
    );

    let mut dangling = build(first.clone());
    dangling.active_session_ids = vec![7];
    assert_eq!(
        check_snapshot_integrity(&snapshot(vec![dangling], Vec::new(), Vec::new())),
        Err(SnapshotIntegrityViolation::MissingBuildSession {
            build: label.clone(),
            session_id: 7,
        })
    );

    let mut borrowed = build(first.clone());
    borrowed.active_session_ids = vec![7];
    assert_eq!(
        check_snapshot_integrity(&snapshot(
            vec![borrowed],
            vec![session(7, &second, "running")],
            Vec::new(),
        )),
        Err(SnapshotIntegrityViolation::MismatchedBuildSession {
            build: label.clone(),
            session_id: 7,
            session_build: format!(
                "{}:{}:{}",
                second.manifest_author, second.d_tag, second.aggregate_hash
            ),
        })
    );

    let mut repeated_workspace = build(first.clone());
    repeated_workspace.assigned_workspace_ids = vec!["primary".to_owned(), "primary".to_owned()];
    assert_eq!(
        check_snapshot_integrity(&snapshot(
            vec![repeated_workspace],
            Vec::new(),
            vec![workspace_definition("primary")],
        )),
        Err(SnapshotIntegrityViolation::DuplicateWorkspaceAssignment {
            build: label.clone(),
            workspace_id: "primary".to_owned(),
        })
    );

    let mut unknown_workspace = build(first);
    unknown_workspace.assigned_workspace_ids = vec!["primary".to_owned()];
    assert_eq!(
        check_snapshot_integrity(&snapshot(vec![unknown_workspace], Vec::new(), Vec::new())),
        Err(SnapshotIntegrityViolation::MissingWorkspaceAssignment {
            build: label,
            workspace_id: "primary".to_owned(),
        })
    );
}

#[test]
fn every_invariant_has_a_distinct_stable_refusal_code() {
    use SnapshotIntegrityViolation as Violation;
    let violations = [
        Violation::TotalInstalledBelowVisible {
            total_installed: 0,
            visible_builds: 1,
        },
        Violation::DuplicateSession { session_id: 1 },
        Violation::UnsupportedSessionState {
            session_id: 1,
            raw_value: "crashed".to_owned(),
        },
        Violation::DuplicateWorkspace {
            workspace_id: "w".to_owned(),
        },
        Violation::DuplicateBuild {
            build: "a".to_owned(),
        },
        Violation::DuplicateBuildSession {
            build: "a".to_owned(),
            session_id: 1,
        },
        Violation::MissingBuildSession {
            build: "a".to_owned(),
            session_id: 1,
        },
        Violation::MismatchedBuildSession {
            build: "a".to_owned(),
            session_id: 1,
            session_build: "b".to_owned(),
        },
        Violation::DuplicateWorkspaceAssignment {
            build: "a".to_owned(),
            workspace_id: "w".to_owned(),
        },
        Violation::MissingWorkspaceAssignment {
            build: "a".to_owned(),
            workspace_id: "w".to_owned(),
        },
    ];
    let codes = violations
        .iter()
        .map(Violation::code)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(codes.len(), violations.len());
}
