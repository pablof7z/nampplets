//! Producer-side referential integrity for the projected snapshot.
//!
//! These invariants used to be re-derived by each platform binding, which made
//! the fail-closed guarantee a per-platform reimplementation (#106). They are
//! asserted here once, against the record every binding actually receives.

use nmp_native_runtime_app::WorkspaceView;

use super::*;
use crate::{
    projection::{assigned_workspace_ids, project_workspaces},
    snapshot_integrity::{SnapshotIntegrityViolation, check_snapshot_integrity},
};

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
fn every_cross_reference_and_duplicate_is_refused_with_the_offending_identity() {
    let first = coordinate("good-morning");
    let second = coordinate("second");
    let label = format!(
        "{}:{}:{}",
        first.manifest_author, first.d_tag, first.aggregate_hash
    );

    let mut below = snapshot(vec![build(first.clone())], Vec::new(), Vec::new());
    below.installed_library.total_installed = 0;
    assert_eq!(
        check_snapshot_integrity(&below),
        Err(SnapshotIntegrityViolation::TotalInstalledBelowVisible {
            total_installed: 0,
            visible_builds: 1,
        })
    );

    let duplicate_session = snapshot(
        Vec::new(),
        vec![
            session(7, &first, "running"),
            session(7, &second, "running"),
        ],
        Vec::new(),
    );
    assert_eq!(
        check_snapshot_integrity(&duplicate_session),
        Err(SnapshotIntegrityViolation::DuplicateSession { session_id: 7 })
    );

    let launching = snapshot(
        Vec::new(),
        vec![session(7, &first, "launching")],
        Vec::new(),
    );
    assert_eq!(
        check_snapshot_integrity(&launching),
        Err(SnapshotIntegrityViolation::UnsupportedSessionState {
            session_id: 7,
            raw_value: "launching".to_owned(),
        })
    );

    let duplicate_workspace = snapshot(
        Vec::new(),
        Vec::new(),
        vec![
            workspace_definition("primary"),
            workspace_definition("primary"),
        ],
    );
    assert_eq!(
        check_snapshot_integrity(&duplicate_workspace),
        Err(SnapshotIntegrityViolation::DuplicateWorkspace {
            workspace_id: "primary".to_owned(),
        })
    );

    let duplicate_build = snapshot(
        vec![build(first.clone()), build(first.clone())],
        Vec::new(),
        Vec::new(),
    );
    assert_eq!(
        check_snapshot_integrity(&duplicate_build),
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
fn each_violation_carries_a_distinct_refusal_code() {
    let codes = [
        SnapshotIntegrityViolation::TotalInstalledBelowVisible {
            total_installed: 0,
            visible_builds: 1,
        },
        SnapshotIntegrityViolation::DuplicateSession { session_id: 1 },
        SnapshotIntegrityViolation::UnsupportedSessionState {
            session_id: 1,
            raw_value: "crashed".to_owned(),
        },
        SnapshotIntegrityViolation::DuplicateWorkspace {
            workspace_id: "a".to_owned(),
        },
        SnapshotIntegrityViolation::DuplicateBuild {
            build: "a".to_owned(),
        },
        SnapshotIntegrityViolation::DuplicateBuildSession {
            build: "a".to_owned(),
            session_id: 1,
        },
        SnapshotIntegrityViolation::MissingBuildSession {
            build: "a".to_owned(),
            session_id: 1,
        },
        SnapshotIntegrityViolation::MismatchedBuildSession {
            build: "a".to_owned(),
            session_id: 1,
            session_build: "b".to_owned(),
        },
        SnapshotIntegrityViolation::DuplicateWorkspaceAssignment {
            build: "a".to_owned(),
            workspace_id: "w".to_owned(),
        },
        SnapshotIntegrityViolation::MissingWorkspaceAssignment {
            build: "a".to_owned(),
            workspace_id: "w".to_owned(),
        },
    ]
    .iter()
    .map(SnapshotIntegrityViolation::code)
    .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(codes.len(), 10);
}

#[test]
fn a_workspace_that_cannot_cross_the_schema_boundary_leaves_no_dangling_assignment() {
    let principal = Principal::new(AUTHOR, "good-morning", "c".repeat(64)).unwrap();
    let valid = workspace_record_from_ffi(workspace_definition("primary")).unwrap();
    let mut future: Value = serde_json::from_str(valid.definition.as_str()).unwrap();
    future["schema_version"] = serde_json::json!(WORKSPACE_SCHEMA_VERSION.saturating_add(1));

    let views = vec![
        WorkspaceView {
            id: Arc::clone(&valid.id),
            definition: valid.definition.clone(),
            retained_receipts: Vec::new(),
            assigned_builds: vec![principal.clone()],
        },
        WorkspaceView {
            id: Arc::from("unreadable"),
            definition: BoundedJson::from_value(&future, MAXIMUM_WORKSPACE_JSON_BYTES).unwrap(),
            retained_receipts: Vec::new(),
            assigned_builds: vec![principal.clone()],
        },
    ];

    let projected = project_workspaces(&views);
    assert_eq!(projected.workspaces.len(), 1);
    assert_eq!(projected.unprojectable.len(), 1);
    assert_eq!(projected.unprojectable[0].0, "unreadable");
    assert_eq!(
        assigned_workspace_ids(&views, &projected.published_ids, &principal),
        ["primary".to_owned()]
    );
}

#[test]
fn a_persistent_projection_fault_is_surfaced_once_rather_than_every_projection() {
    let temp = TempDir::new().unwrap();
    let runtime = controller(&temp);
    let detail = "session 7 appears more than once".to_owned();
    for _ in 0..3 {
        runtime.report_projection_fault("snapshot-integrity-duplicate-session", detail.clone());
        let _ = runtime.snapshot();
    }
    runtime.report_projection_fault(
        "snapshot-integrity-duplicate-session",
        "session 8".to_owned(),
    );

    let recorded = runtime
        .snapshot()
        .boundary_refusals
        .into_iter()
        .filter(|refusal| refusal.code == "snapshot-integrity-duplicate-session")
        .collect::<Vec<_>>();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].detail, detail);
    assert_eq!(recorded[1].detail, "session 8");
}

#[test]
fn a_live_controller_never_publishes_an_inconsistent_snapshot() {
    let temp = TempDir::new().unwrap();
    let runtime = controller(&temp);
    let artifact = runtime
        .verify_artifact(
            EVENT.to_vec(),
            ArtifactCoordinate::Named {
                author: AUTHOR.to_owned(),
                d_tag: "good-morning".to_owned(),
            },
        )
        .artifact
        .expect("fixture verifies");
    let installed = exact_coordinate(&artifact);
    runtime.install(Arc::clone(&artifact));
    assert!(
        runtime
            .save_workspace(workspace_definition("primary"))
            .accepted
    );
    runtime.assign_build_to_workspace("primary".to_owned(), installed);
    for domain in ["identity", "inc", "outbox"] {
        runtime.set_grant(
            Arc::clone(&artifact),
            domain.to_owned(),
            RuntimeSensitivity::Sensitive,
            RuntimeGrantDecision::AllowExactBuild,
        );
    }
    runtime.launch(Arc::clone(&artifact), RuntimeExecutionProfile::Legacy);

    let projected = runtime.snapshot();
    assert_eq!(projected.sessions.len(), 1);
    assert_eq!(check_snapshot_integrity(&projected), Ok(()));
    assert_eq!(
        projected.installed_library.builds[0].assigned_workspace_ids,
        ["primary".to_owned()]
    );
    assert!(
        projected
            .boundary_refusals
            .iter()
            .all(|refusal| !refusal.code.starts_with("snapshot-integrity"))
    );
}

/// Regression (#106): reporting a projection fault must not deadlock.
///
/// `project_snapshot` holds the `boundary_refusals` guard while it reads the
/// ring into the snapshot, and `report_projection_faults` re-enters that same
/// lock through `record_boundary_refusal`. `parking_lot` mutexes are not
/// reentrant, so the read guard has to be released before the report runs.
///
/// The invariant tests above cannot catch this: they call
/// `check_snapshot_integrity` on a fixture and never go through
/// `project_snapshot`, which is the only place the two locks meet. A healthy
/// snapshot never reports anything, so the hazard is invisible until a fault
/// actually occurs in production.
///
/// The projection runs on its own thread against a deadline so that a
/// regression fails this test instead of hanging the whole suite.
#[test]
fn reporting_a_projection_fault_through_project_snapshot_does_not_deadlock() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp);

    // A stored workspace whose id can no longer cross the boundary: control
    // characters are rejected by `validate_workspace_name`, so this view is
    // dropped by the projection and reported as a fault.
    let mut app_snapshot = (*controller.app.snapshot()).clone();
    app_snapshot.workspaces = vec![WorkspaceView {
        id: Arc::from("broken\u{1}workspace"),
        definition: BoundedJson::from_value(&serde_json::json!({}), 4_096).unwrap(),
        retained_receipts: Vec::new(),
        assigned_builds: Vec::new(),
    }];

    let projecting = Arc::clone(&controller);
    let (finished, projected) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let snapshot = projecting.project_snapshot(&app_snapshot);
        let _ = finished.send(snapshot);
    });

    let snapshot = projected
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect(
            "project_snapshot deadlocked while reporting a projection fault: \
             the boundary_refusals guard must be released before \
             report_projection_faults re-enters it",
        );

    assert!(snapshot.workspaces.is_empty());
    assert!(
        controller
            .snapshot()
            .boundary_refusals
            .iter()
            .any(|refusal| refusal.code == "workspace-projection"),
        "a dropped workspace must be reported as a boundary refusal",
    );
}
