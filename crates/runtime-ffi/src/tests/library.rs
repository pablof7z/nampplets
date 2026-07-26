use super::*;

#[test]
fn installed_library_projects_filter_lifecycle_workspace_and_uninstall() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp);
    let artifact = controller
        .verify_artifact(
            EVENT.to_vec(),
            ArtifactCoordinate::Named {
                author: AUTHOR.to_owned(),
                d_tag: "good-morning".to_owned(),
            },
        )
        .artifact
        .expect("fixture verifies");
    let coordinate = exact_coordinate(&artifact);

    assert_eq!(
        controller
            .snapshot_value()
            .installed_library
            .total_installed,
        0
    );
    controller.install(Arc::clone(&artifact));
    let installed = controller.snapshot_value().installed_library;
    assert_eq!(installed.query, "");
    assert_eq!(installed.total_installed, 1);
    assert_eq!(installed.builds.len(), 1);
    assert_eq!(installed.builds[0].coordinate, coordinate);
    assert_eq!(
        installed.builds[0].availability,
        RuntimeInstalledBuildAvailability::SealedExactBytesReady
    );
    assert!(installed.builds[0].active_session_ids.is_empty());
    assert!(installed.builds[0].assigned_workspace_ids.is_empty());
    assert!(serde_json::from_str::<Value>(&installed.builds[0].manifest_metadata_json).is_ok());

    controller.set_library_filter("no-match".to_owned());
    let filtered = controller.snapshot_value().installed_library;
    assert_eq!(filtered.query, "no-match");
    assert_eq!(filtered.total_installed, 1);
    assert!(filtered.builds.is_empty());
    controller.set_library_filter("GOOD-MORNING".to_owned());
    assert_eq!(
        controller.snapshot_value().installed_library.builds.len(),
        1
    );

    assert!(
        controller
            .save_workspace(workspace_definition("library"))
            .accepted
    );
    controller.assign_build_to_workspace("library".to_owned(), coordinate.clone());
    assert_eq!(
        controller.snapshot_value().installed_library.builds[0].assigned_workspace_ids,
        ["library"]
    );

    controller.launch(Arc::clone(&artifact), RuntimeExecutionProfile::Legacy);
    assert!(
        controller.snapshot_value().sessions.is_empty(),
        "the pinned required profile must refuse before execution"
    );
    for domain in ["identity", "inc", "outbox"] {
        controller.set_grant(
            Arc::clone(&artifact),
            domain.to_owned(),
            RuntimeSensitivity::Sensitive,
            RuntimeGrantDecision::AllowExactBuild,
        );
    }
    controller.launch(Arc::clone(&artifact), RuntimeExecutionProfile::Legacy);
    let session = controller.snapshot_value().installed_library.builds[0].active_session_ids[0];
    controller.suspend(session);
    assert_eq!(controller.snapshot_value().sessions[0].state, "suspended");
    controller.resume(session);
    assert_eq!(controller.snapshot_value().sessions[0].state, "running");

    controller.clear_build_from_workspace("library".to_owned(), coordinate.clone());
    assert!(
        controller.snapshot_value().installed_library.builds[0]
            .assigned_workspace_ids
            .is_empty()
    );

    controller.uninstall_build(coordinate.clone());
    let uninstalled = controller.snapshot_value();
    assert_eq!(uninstalled.installed_library.total_installed, 0);
    assert!(uninstalled.installed_library.builds.is_empty());
    assert!(uninstalled.sessions.is_empty());
    assert!(uninstalled.workspaces.iter().any(|workspace| {
        workspace.workspace_id == "library" && workspace.retained_receipt_ids.is_empty()
    }));
    assert!(
        !controller.artifacts.lock().contains_key(
            &Principal::new(
                coordinate.manifest_author,
                coordinate.d_tag,
                coordinate.aggregate_hash
            )
            .unwrap()
        ),
        "the boundary must release its live verifier handle after kernel-confirmed uninstall"
    );
}

#[test]
fn installed_artifact_reacquisition_reuses_the_live_exact_handle() {
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
    let coordinate = exact_coordinate(&artifact);
    runtime.install(artifact);
    runtime.set_library_filter("does-not-match".to_owned());

    let reopened = runtime.reacquire_installed_artifact(coordinate);
    assert!(reopened.failure.is_none());
    let confirmation = reopened.confirmation.expect("exact confirmation");
    let event: Value = serde_json::from_slice(EVENT).unwrap();
    assert_eq!(
        confirmation.event_id,
        event["id"].as_str().expect("fixture event id")
    );
    assert_eq!(confirmation.manifest_author, AUTHOR);
    assert_eq!(confirmation.d_tag.as_deref(), Some("good-morning"));
    assert_eq!(confirmation.aggregate_hash, GOOD_MORNING_AGGREGATE_HASH);
    assert_eq!(
        reopened
            .artifact
            .expect("opaque artifact")
            .handle
            .read_verified(nmp_native_artifact::INDEX_PATH, INDEX.len())
            .unwrap(),
        INDEX
    );
}

#[test]
fn persisted_install_without_a_live_handle_fails_closed_after_restart() {
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
    runtime.install(Arc::clone(&artifact));
    let coordinate = exact_coordinate(&artifact);
    runtime.close();
    drop(runtime);

    let reopened = controller(&temp);
    let result = reopened.reacquire_installed_artifact(coordinate);
    assert_eq!(
        reopened.snapshot_value().installed_library.builds[0].availability,
        RuntimeInstalledBuildAvailability::MetadataOnly
    );
    assert!(result.artifact.is_none());
    assert_eq!(
        result.failure.expect("typed refusal").code,
        "artifact-handle-unavailable"
    );
}

#[test]
fn installed_artifact_reattach_refuses_signed_event_drift() {
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
    runtime.install(Arc::clone(&artifact));
    let mut installed = runtime.app.snapshot().library.builds[0].build.clone();
    installed.manifest_metadata = BoundedJson::from_value(
        &serde_json::json!({
            "event_id": "0".repeat(64),
            "kind": 35_129,
            "mode": "single-file",
            "paths": 1,
        }),
        1_024,
    )
    .unwrap();

    let failure = runtime
        .verified_installed_artifact(&installed, Arc::clone(&artifact.handle))
        .expect_err("a different signed event must not inherit the persisted install");
    assert_eq!(failure.code, "installed-artifact-mismatch");
}

#[test]
fn installed_library_restores_metadata_only_and_refuses_invalid_inputs() {
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
    runtime.install(artifact);
    runtime.close();
    drop(runtime);

    let reopened = controller(&temp);
    let restored = reopened.snapshot_value().installed_library;
    assert_eq!(restored.total_installed, 1);
    assert_eq!(restored.builds.len(), 1);
    assert_eq!(
        restored.builds[0].availability,
        RuntimeInstalledBuildAvailability::MetadataOnly
    );
    assert!(restored.builds[0].active_session_ids.is_empty());

    reopened.uninstall_build(RuntimeExactBuildCoordinate {
        manifest_author: AUTHOR.to_ascii_uppercase(),
        d_tag: "good-morning".to_owned(),
        aggregate_hash: restored.builds[0].coordinate.aggregate_hash.clone(),
    });
    let snapshot = reopened.snapshot_value();
    assert_eq!(snapshot.installed_library.total_installed, 1);
    assert_eq!(
        snapshot.boundary_refusals.last().unwrap().code,
        "invalid-exact-build-coordinate"
    );

    reopened.assign_build_to_workspace("\n".to_owned(), restored.builds[0].coordinate.clone());
    assert_eq!(
        reopened
            .snapshot_value()
            .boundary_refusals
            .last()
            .unwrap()
            .code,
        "invalid-workspace-assignment"
    );

    reopened.set_library_filter("x".repeat(AppLimits::default().maximum_library_query_bytes + 1));
    let refused = reopened.snapshot_value();
    assert_eq!(refused.installed_library.query, "");
    assert_eq!(refused.recent_errors.last().unwrap().code, "capacity");
}
