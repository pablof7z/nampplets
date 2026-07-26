use super::*;

fn controller_with_boundary_event_limit(
    temp: &TempDir,
    maximum_boundary_events: u64,
) -> Arc<RuntimeController> {
    RuntimeController::open(
        RuntimeConfig {
            runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
            nmp_store_path: None,
            artifact_cache_path: temp.path().join("artifacts").display().to_string(),
            maximum_boundary_events,
            ..RuntimeConfig::default()
        },
        Box::new(FixtureSource(BTreeMap::from([(
            DIGEST.to_owned(),
            INDEX.to_vec(),
        )]))),
    )
    .unwrap()
}

#[test]
fn latch_overflow_is_counted_when_projection_evidence_exceeds_the_refusal_ring() {
    let temp = TempDir::new().unwrap();
    let controller = controller_with_boundary_event_limit(&temp, 4);
    let starting_signal = controller.projection_signal_revision();

    for index in 0..64 {
        let id = format!("ring-overflow-{index}");
        expect_workspace_refusal(
            controller.project_snapshot(&malformed_app_snapshot(&controller, &id)),
            &id,
        );
    }
    let exact_signal = controller.projection_signal_revision();
    let exact_snapshot = controller.snapshot_value();
    assert_eq!(exact_signal - starting_signal, 64);
    assert_eq!(exact_snapshot.boundary_refusals.len(), 4);
    assert_eq!(exact_snapshot.dropped_boundary_refusals, 60);

    expect_workspace_refusal(
        controller.project_snapshot(&malformed_app_snapshot(&controller, "ring-overflow-marker")),
        "ring-overflow-marker",
    );
    let overflow_signal = controller.projection_signal_revision();
    let overflow_snapshot = controller.snapshot_value();
    assert_eq!(overflow_signal, exact_signal + 1);
    assert_eq!(overflow_snapshot.boundary_refusals.len(), 4);
    assert_eq!(overflow_snapshot.dropped_boundary_refusals, 61);
    assert_eq!(
        overflow_snapshot.boundary_refusals.last().unwrap().code,
        "projection-fault-latch-capacity"
    );

    for id in ["ring-overflow-0", "ring-overflow-unseen"] {
        expect_workspace_refusal(
            controller.project_snapshot(&malformed_app_snapshot(&controller, id)),
            id,
        );
    }
    let later_snapshot = controller.snapshot_value();
    assert_eq!(controller.projection_signal_revision(), overflow_signal);
    assert_eq!(
        later_snapshot.dropped_boundary_refusals,
        overflow_snapshot.dropped_boundary_refusals
    );
    assert_eq!(
        later_snapshot
            .boundary_refusals
            .iter()
            .map(|refusal| (&refusal.code, &refusal.detail, refusal.occurred_at_millis))
            .collect::<Vec<_>>(),
        overflow_snapshot
            .boundary_refusals
            .iter()
            .map(|refusal| (&refusal.code, &refusal.detail, refusal.occurred_at_millis))
            .collect::<Vec<_>>()
    );
}
