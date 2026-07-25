use super::*;

#[test]
fn typed_workspace_round_trips_only_through_rust_owned_storage() {
    let temp = TempDir::new().unwrap();
    let runtime = controller(&temp);
    let expected = workspace_definition("primary");
    let saved = runtime.save_workspace(expected.clone());
    assert!(saved.accepted);
    assert_eq!(saved.workspace, Some(expected.clone()));
    assert_eq!(
        runtime.snapshot().workspaces,
        std::slice::from_ref(&expected)
    );
    runtime.close();
    drop(runtime);

    let reopened = controller(&temp);
    assert!(reopened.snapshot().workspaces.is_empty());
    let restored = reopened.restore_workspaces();
    assert!(restored.accepted);
    assert_eq!(restored.workspaces, std::slice::from_ref(&expected));
    assert_eq!(reopened.snapshot().workspaces, [expected]);
}

#[test]
fn workspace_validation_refuses_unknown_duplicate_and_oversized_input() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp);

    let mut unknown = workspace_definition("unknown");
    unknown.schema_version = WORKSPACE_SCHEMA_VERSION + 1;
    let refusal = controller.save_workspace(unknown).refusal.unwrap();
    assert_eq!(refusal.code, "invalid-workspace");

    let mut duplicate = workspace_definition("duplicate");
    duplicate.slots[1].slot_id = duplicate.slots[0].slot_id.clone();
    let refusal = controller.save_workspace(duplicate).refusal.unwrap();
    assert_eq!(refusal.code, "invalid-workspace");
    assert!(refusal.detail.contains("duplicate workspace slot id"));

    let mut oversized = workspace_definition("oversized");
    oversized.preferences_json = format!(
        r#"{{"value":"{}"}}"#,
        "x".repeat(MAXIMUM_WORKSPACE_FIELD_BYTES)
    );
    let refusal = controller.save_workspace(oversized).refusal.unwrap();
    assert_eq!(refusal.code, "invalid-workspace");
    assert!(refusal.detail.contains("maximum"));
    assert!(controller.snapshot().workspaces.is_empty());
}

#[test]
fn workspace_restore_is_all_or_nothing_for_corrupt_or_future_rows() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp);
    let valid = workspace_record_from_ffi(workspace_definition("a-valid")).unwrap();
    controller.runtime_store.save_workspace(&valid).unwrap();

    let mut future_value: serde_json::Value =
        serde_json::from_str(valid.definition.as_str()).unwrap();
    future_value["schema_version"] = serde_json::json!(WORKSPACE_SCHEMA_VERSION.saturating_add(1));
    controller
        .runtime_store
        .save_workspace(&WorkspaceRecord {
            id: Arc::from("z-future"),
            definition: BoundedJson::from_value(&future_value, MAXIMUM_WORKSPACE_JSON_BYTES)
                .unwrap(),
            retained_receipts: Vec::new(),
        })
        .unwrap();

    let restored = controller.restore_workspaces();
    assert!(!restored.accepted);
    assert!(restored.workspaces.is_empty());
    assert_eq!(restored.refusal.unwrap().code, "invalid-workspace");
    assert!(controller.snapshot().workspaces.is_empty());
}
