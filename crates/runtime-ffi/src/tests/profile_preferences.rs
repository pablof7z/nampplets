use super::*;

fn persistent_controller(temp: &TempDir) -> Arc<RuntimeController> {
    RuntimeController::open(
        RuntimeConfig {
            runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
            nmp_store_path: Some(temp.path().join("nmp.redb").display().to_string()),
            artifact_cache_path: temp.path().join("artifacts").display().to_string(),
            indexer_relays: vec!["wss://bundled-search.example".to_owned()],
            app_relays: vec!["wss://bundled-home.example".to_owned()],
            ..RuntimeConfig::default()
        },
        Box::new(FixtureSource(BTreeMap::new())),
    )
    .unwrap()
}

#[test]
fn profile_preferences_are_rust_validated_persisted_and_restart_explicit() {
    let temp = TempDir::new().unwrap();
    let controller = persistent_controller(&temp);
    assert_eq!(
        controller.profile_preferences(),
        RuntimeProfilePreferences {
            indexer_relays: vec!["wss://bundled-search.example".to_owned()],
            app_relays: vec!["wss://bundled-home.example".to_owned()],
            permission_default: RuntimePermissionDefault::AskEveryTime,
        }
    );

    let invalid = controller.update_profile_preferences(RuntimeProfilePreferences {
        indexer_relays: vec!["https://not-a-relay.example".to_owned()],
        app_relays: vec!["wss://home.example".to_owned()],
        permission_default: RuntimePermissionDefault::AllowSession,
    });
    assert!(!invalid.applied);
    assert!(!invalid.restart_required);
    assert_eq!(
        invalid
            .refusal
            .as_ref()
            .map(|refusal| refusal.code.as_str()),
        Some("invalid-preferences")
    );

    let saved = RuntimeProfilePreferences {
        indexer_relays: vec!["wss://search.example".to_owned()],
        app_relays: vec![
            "wss://home.example".to_owned(),
            "wss://friends.example".to_owned(),
        ],
        permission_default: RuntimePermissionDefault::AllowExactBuild,
    };
    let update = controller.update_profile_preferences(saved.clone());
    assert!(update.applied);
    assert!(update.restart_required);
    assert_eq!(update.preferences, Some(saved.clone()));
    controller.close();
    drop(controller);

    let reopened = persistent_controller(&temp);
    assert_eq!(
        reopened.profile_preferences(),
        saved,
        "stored user preferences override the bundled deployment defaults"
    );
    reopened.close();
}

#[test]
fn storage_snapshot_is_bounded_and_nmp_reset_closes_before_deleting() {
    let temp = TempDir::new().unwrap();
    let controller = persistent_controller(&temp);
    let cache_path = temp.path().join("nmp.redb");
    assert!(cache_path.exists());

    let storage = controller.storage_snapshot();
    assert!(storage.nmp_cache_bytes > 0);
    assert!(storage.app_data_bytes > 0);
    assert_eq!(
        storage.total_bytes,
        storage
            .nmp_cache_bytes
            .saturating_add(storage.app_data_bytes)
    );
    assert!(!storage.incomplete);

    let reset = controller.reset_nmp_cache();
    assert!(reset.reset, "{:?}", reset.refusal);
    assert!(reset.refusal.is_none());
    assert!(!cache_path.exists());
    assert!(controller.snapshot_value().closed);

    drop(controller);
    let reopened = persistent_controller(&temp);
    assert!(cache_path.exists());
    reopened.close();
}
