use tempfile::TempDir;

use super::{
    MAXIMUM_PROFILE_RELAYS_PER_LANE, PermissionDefaultPreference, ProfilePreferences, RuntimeStore,
    StoreError, StoreLimits,
};

#[test]
fn profile_preferences_are_atomic_bounded_and_survive_restart() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("runtime.db");
    let store = RuntimeStore::open(&path, StoreLimits::default()).unwrap();
    assert_eq!(store.profile_preferences().unwrap(), None);
    assert!(
        store
            .table_names()
            .unwrap()
            .contains(&"profile_preferences".to_owned())
    );

    let preferences = ProfilePreferences::new(
        vec!["wss://search.example".to_owned()],
        vec![
            "wss://home.example".to_owned(),
            "wss://friends.example".to_owned(),
        ],
        PermissionDefaultPreference::AllowExactBuild,
    )
    .unwrap();
    store.save_profile_preferences(&preferences).unwrap();
    assert_eq!(
        store.profile_preferences().unwrap(),
        Some(preferences.clone())
    );

    let duplicate = ProfilePreferences::new(
        vec!["wss://same.example".to_owned()],
        vec![
            "wss://same.example".to_owned(),
            "wss://same.example".to_owned(),
        ],
        PermissionDefaultPreference::AskEveryTime,
    );
    assert!(matches!(
        duplicate,
        Err(StoreError::DuplicateProfileRelay { lane: "app" })
    ));
    assert_eq!(
        store.profile_preferences().unwrap(),
        Some(preferences.clone())
    );

    drop(store);
    let reopened = RuntimeStore::open(&path, StoreLimits::default()).unwrap();
    assert_eq!(reopened.profile_preferences().unwrap(), Some(preferences));
}

#[test]
fn profile_preferences_refuse_unbounded_relay_lanes() {
    let too_many = (0..=MAXIMUM_PROFILE_RELAYS_PER_LANE)
        .map(|index| format!("wss://relay-{index}.example"))
        .collect();
    assert!(matches!(
        ProfilePreferences::new(
            too_many,
            vec!["wss://home.example".to_owned()],
            PermissionDefaultPreference::AllowSession,
        ),
        Err(StoreError::ProfileRelayCapacity {
            lane: "indexer",
            actual,
            maximum: MAXIMUM_PROFILE_RELAYS_PER_LANE,
        }) if actual == MAXIMUM_PROFILE_RELAYS_PER_LANE + 1
    ));
}
