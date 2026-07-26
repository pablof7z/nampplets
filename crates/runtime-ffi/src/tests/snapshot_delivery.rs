//! Same-call refusal delivery and bounded projection-fault evidence.

use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use nmp_native_runtime_app::{AppSnapshot, WorkspaceView};

use super::*;

mod ring_overflow;

fn malformed_view(id: &str) -> WorkspaceView {
    WorkspaceView {
        id: Arc::from(id),
        definition: BoundedJson::from_value(&serde_json::json!({}), 4_096).unwrap(),
        retained_receipts: Vec::new(),
        assigned_builds: Vec::new(),
    }
}

fn malformed_app_snapshot(controller: &RuntimeController, id: &str) -> AppSnapshot {
    let mut snapshot = (*controller.app.snapshot()).clone();
    snapshot.workspaces = vec![malformed_view(id)];
    snapshot
}

fn expect_workspace_refusal(projection: RuntimeSnapshotProjection, id: &str) -> RuntimeRefusal {
    match projection {
        RuntimeSnapshotProjection::Refused {
            revision: _,
            closed: _,
            refusal,
        } => {
            assert_eq!(refusal.code, "workspace-projection");
            assert!(refusal.detail.contains(id), "{refusal:?}");
            refusal
        }
        RuntimeSnapshotProjection::Snapshot { .. } => {
            panic!("malformed workspace {id} crossed the FFI projection")
        }
    }
}

#[test]
fn first_malformed_projection_returns_only_the_exact_refusal() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp);
    let malformed = malformed_app_snapshot(&controller, "broken-first");

    let refusal = match controller.project_snapshot(&malformed) {
        RuntimeSnapshotProjection::Refused {
            revision,
            closed,
            refusal,
        } => {
            assert_eq!(revision, malformed.revision);
            assert_eq!(closed, malformed.closed);
            assert_eq!(refusal.code, "workspace-projection");
            assert!(refusal.detail.contains("broken-first"), "{refusal:?}");
            refusal
        }
        RuntimeSnapshotProjection::Snapshot { .. } => {
            panic!("the malformed first-call snapshot crossed the FFI projection")
        }
    };
    assert_eq!(refusal.code, "workspace-projection");

    let evidence = controller.snapshot_value();
    assert_eq!(evidence.boundary_refusals.len(), 1);
    assert_eq!(evidence.boundary_refusals[0].code, refusal.code);
    assert_eq!(evidence.boundary_refusals[0].detail, refusal.detail);
    assert_eq!(
        evidence.boundary_refusals[0].occurred_at_millis,
        refusal.occurred_at_millis
    );
}

struct ProjectionObserver(mpsc::Sender<RuntimeObservationFrame>);

impl RuntimeObserver for ProjectionObserver {
    fn update(&self, frame: RuntimeObservationFrame) {
        let _ = self.0.send(frame);
    }
}

#[test]
fn observer_receives_the_refusal_in_the_frame_for_the_malformed_update() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp);
    let (send, receive) = mpsc::channel();
    let observation = controller
        .clone()
        .observe(Box::new(ProjectionObserver(send)))
        .observation
        .expect("observer admitted");
    let initial = receive
        .recv_timeout(Duration::from_secs(2))
        .expect("initial frame");
    assert!(matches!(
        initial.snapshot,
        RuntimeSnapshotProjection::Snapshot { .. }
    ));

    controller.app.dispatch(PlatformCommand::SaveWorkspace {
        workspace: WorkspaceRecord {
            id: Arc::from("broken-observer"),
            definition: BoundedJson::from_value(&serde_json::json!({}), 4_096).unwrap(),
            retained_receipts: Vec::new(),
        },
    });

    let deadline = Instant::now() + Duration::from_secs(10);
    let refused_frame = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let frame = receive
            .recv_timeout(remaining)
            .expect("observer did not receive the malformed projection");
        if matches!(frame.snapshot, RuntimeSnapshotProjection::Refused { .. }) {
            break frame;
        }
    };
    expect_workspace_refusal(refused_frame.snapshot, "broken-observer");
    observation.stop();
}

#[test]
fn first_refusal_return_does_not_deadlock_on_boundary_evidence_append() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp);
    let malformed = malformed_app_snapshot(&controller, "broken-deadlock");
    let projecting = Arc::clone(&controller);
    let (finished, projected) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = finished.send(projecting.project_snapshot(&malformed));
    });

    let first_return = projected.recv_timeout(Duration::from_secs(10)).expect(
        "project_snapshot deadlocked before its first refusal return while \
         appending boundary evidence",
    );
    expect_workspace_refusal(first_return, "broken-deadlock");
}

#[test]
fn latch_records_64_exact_faults_then_one_overflow_without_rewake() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp);
    let starting_signal = controller.projection_signal_revision();

    for index in 0..64 {
        let id = format!("broken-{index}");
        let malformed = malformed_app_snapshot(&controller, &id);
        expect_workspace_refusal(controller.project_snapshot(&malformed), &id);
    }
    let exact_signal = controller.projection_signal_revision();
    let exact_evidence = controller.snapshot_value().boundary_refusals;
    assert_eq!(exact_evidence.len(), 64);
    assert_eq!(exact_signal - starting_signal, 64);
    assert!(
        exact_evidence
            .iter()
            .all(|refusal| refusal.code == "workspace-projection")
    );

    let overflow_id = "broken-overflow";
    expect_workspace_refusal(
        controller.project_snapshot(&malformed_app_snapshot(&controller, overflow_id)),
        overflow_id,
    );
    let overflow_signal = controller.projection_signal_revision();
    let overflow_snapshot = controller.snapshot_value();
    let overflow_dropped = overflow_snapshot.dropped_boundary_refusals;
    let overflow_evidence = overflow_snapshot.boundary_refusals;
    assert_eq!(overflow_signal, exact_signal + 1);
    assert_eq!(overflow_evidence.len(), 65);
    assert_eq!(
        overflow_evidence.last().unwrap().code,
        "projection-fault-latch-capacity"
    );

    let existing_id = "broken-0";
    expect_workspace_refusal(
        controller.project_snapshot(&malformed_app_snapshot(&controller, existing_id)),
        existing_id,
    );
    let existing_snapshot = controller.snapshot_value();
    assert_eq!(controller.projection_signal_revision(), overflow_signal);
    assert_eq!(
        existing_snapshot.dropped_boundary_refusals,
        overflow_dropped
    );
    assert_eq!(
        existing_snapshot.boundary_refusals.len(),
        overflow_evidence.len()
    );

    let later_id = "broken-later";
    let later_refusal = expect_workspace_refusal(
        controller.project_snapshot(&malformed_app_snapshot(&controller, later_id)),
        later_id,
    );
    assert_eq!(later_refusal.code, "workspace-projection");
    assert_eq!(controller.projection_signal_revision(), overflow_signal);
    let later_snapshot = controller.snapshot_value();
    assert_eq!(later_snapshot.dropped_boundary_refusals, overflow_dropped);
    let later_evidence = later_snapshot.boundary_refusals;
    assert_eq!(later_evidence.len(), overflow_evidence.len());
    assert_eq!(
        later_evidence
            .iter()
            .map(|refusal| (&refusal.code, &refusal.detail, refusal.occurred_at_millis))
            .collect::<Vec<_>>(),
        overflow_evidence
            .iter()
            .map(|refusal| (&refusal.code, &refusal.detail, refusal.occurred_at_millis))
            .collect::<Vec<_>>()
    );
}
