use nmp_native_runtime_core::{Capability, GrantDecision};
use tempfile::TempDir;

use super::*;

fn principal(hash: char) -> Principal {
    Principal::new("a".repeat(64), "app", hash.to_string().repeat(64)).unwrap()
}

fn store() -> (TempDir, RuntimeStore) {
    let directory = TempDir::new().unwrap();
    let store =
        RuntimeStore::open(directory.path().join("runtime.db"), StoreLimits::default()).unwrap();
    (directory, store)
}

fn workspace(receipts: Vec<WriteReceiptId>) -> WorkspaceRecord {
    WorkspaceRecord {
        id: Arc::from("main"),
        definition: BoundedJson::from_value(&serde_json::json!({"slots": ["feed"]}), 1024).unwrap(),
        retained_receipts: receipts,
    }
}

fn activity(operation: &str, outcome: &str) -> ActivityRecord {
    ActivityRecord {
        principal: principal('b'),
        category: Arc::from("provider"),
        operation: Arc::from(operation),
        outcome: Arc::from(outcome),
        occurred_at_millis: 1,
    }
}

fn install(store: &RuntimeStore, principal: Principal, title: &str) {
    store
        .install(&InstalledBuild {
            principal,
            title: Arc::from(title),
            manifest_metadata: BoundedJson::from_value(&serde_json::json!({"kind": 35129}), 1024)
                .unwrap(),
            capability_requests: Vec::new(),
        })
        .unwrap();
}

#[test]
fn component_storage_isolated_by_build_hash() {
    let (_directory, store) = store();
    store
        .put_component_value(&principal('b'), "storage", "token", b"first")
        .unwrap();
    assert_eq!(
        store
            .component_value(&principal('c'), "storage", "token")
            .unwrap(),
        None
    );
}

#[test]
fn component_key_listing_and_removal_are_exact_bounded_and_isolated() {
    let (_directory, store) = store();
    let first = principal('b');
    let second = principal('c');
    for key in ["z-last", "a-first"] {
        store
            .put_component_value(&first, "storage", key, key.as_bytes())
            .unwrap();
    }
    store
        .put_component_value(&second, "storage", "other", b"other")
        .unwrap();

    assert_eq!(
        store.component_keys(&first, "storage", 2).unwrap(),
        ["a-first", "z-last"]
    );
    assert!(matches!(
        store.component_keys(&first, "storage", 1),
        Err(StoreError::KeyListCapacity {
            actual_at_least: 2,
            maximum: 1
        })
    ));
    assert!(matches!(
        store.component_keys(&first, "storage", 0),
        Err(StoreError::InvalidKeyListLimit { requested: 0, .. })
    ));
    assert!(
        store
            .remove_component_value(&first, "storage", "a-first")
            .unwrap()
    );
    assert!(
        !store
            .remove_component_value(&first, "storage", "a-first")
            .unwrap()
    );
    assert_eq!(
        store.component_keys(&first, "storage", 2).unwrap(),
        ["z-last"]
    );
    assert_eq!(
        store.component_keys(&second, "storage", 2).unwrap(),
        ["other"]
    );
}

#[test]
fn sensitive_grant_does_not_transfer_to_update() {
    let (_directory, store) = store();
    let upload = Capability::new("upload").unwrap();
    store
        .set_grant(&principal('b'), &upload, GrantDecision::AllowExactBuild)
        .unwrap();
    assert_eq!(
        store.grant_entry(&principal('b'), &upload).unwrap(),
        Some(GrantDecision::AllowExactBuild)
    );
    assert_eq!(store.grant_entry(&principal('c'), &upload).unwrap(), None);
    assert_eq!(
        store.grant(&principal('c'), &upload).unwrap(),
        GrantDecision::Denied
    );
}

#[test]
fn capability_requests_and_atomic_grant_batch_survive_restart_without_partial_rows() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("runtime.db");
    let exact = principal('b');
    let identity = Capability::new("identity").unwrap();
    let outbox = Capability::new("outbox").unwrap();
    let store = RuntimeStore::open(&path, StoreLimits::default()).unwrap();
    store
        .install(&InstalledBuild {
            principal: exact.clone(),
            title: Arc::from("Good Morning"),
            manifest_metadata: BoundedJson::from_value(&serde_json::json!({"kind": 35129}), 1024)
                .unwrap(),
            capability_requests: vec![
                CapabilityRequest {
                    capability: identity.clone(),
                    requirement: nmp_native_runtime_core::CapabilityRequirement::Required,
                },
                CapabilityRequest {
                    capability: outbox.clone(),
                    requirement: nmp_native_runtime_core::CapabilityRequirement::Optional,
                },
            ],
        })
        .unwrap();

    let trigger = Connection::open(&path).unwrap();
    trigger
        .execute_batch(
            "CREATE TRIGGER refuse_outbox_grant
             BEFORE INSERT ON grants
             WHEN NEW.capability = 'outbox'
             BEGIN
               SELECT RAISE(ABORT, 'injected grant write failure');
             END;",
        )
        .unwrap();
    assert!(matches!(
        store.set_grants_atomic(
            &exact,
            &[
                (identity.clone(), GrantDecision::AllowExactBuild),
                (outbox.clone(), GrantDecision::AllowExactBuild),
            ],
        ),
        Err(StoreError::Sqlite(_))
    ));
    assert_eq!(
        store.grant(&exact, &identity).unwrap(),
        GrantDecision::Denied
    );
    assert_eq!(store.grant(&exact, &outbox).unwrap(), GrantDecision::Denied);

    trigger
        .execute("DROP TRIGGER refuse_outbox_grant", [])
        .unwrap();
    store
        .set_grants_atomic(
            &exact,
            &[
                (identity.clone(), GrantDecision::AllowExactBuild),
                (outbox.clone(), GrantDecision::AllowExactBuild),
            ],
        )
        .unwrap();
    store
        .set_grants_atomic(&exact, &[(identity.clone(), GrantDecision::AllowSession)])
        .unwrap();
    drop(trigger);
    drop(store);

    let reopened = RuntimeStore::open(&path, StoreLimits::default()).unwrap();
    let installed = reopened.installed_builds().unwrap();
    assert_eq!(installed[0].capability_requests.len(), 2);
    assert_eq!(
        reopened.grant(&exact, &identity).unwrap(),
        GrantDecision::Denied,
        "session-only allowance must not resurrect a prior durable grant"
    );
    assert_eq!(
        reopened.grant(&exact, &outbox).unwrap(),
        GrantDecision::AllowExactBuild
    );
}

#[test]
fn restart_restores_workspace_and_receipt_reference() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("runtime.db");
    {
        let store = RuntimeStore::open(&path, StoreLimits::default()).unwrap();
        store
            .save_workspace(&workspace(vec![WriteReceiptId(Arc::from("receipt-1"))]))
            .unwrap();
    }
    let reopened = RuntimeStore::open(&path, StoreLimits::default()).unwrap();
    let workspaces = reopened.load_workspaces().unwrap();
    assert_eq!(workspaces[0].retained_receipts[0].0.as_ref(), "receipt-1");
}

#[test]
fn installation_title_and_metadata_are_refused_before_persistence() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("runtime.db");
    let limits = StoreLimits {
        maximum_install_title_bytes: 4,
        maximum_value_bytes: 16,
        ..StoreLimits::default()
    };
    let store = RuntimeStore::open(&path, limits).unwrap();
    let metadata = BoundedJson::from_value(&serde_json::json!({}), 16).unwrap();

    assert!(matches!(
        store.install(&InstalledBuild {
            principal: principal('b'),
            title: Arc::from("large"),
            manifest_metadata: metadata.clone(),
            capability_requests: Vec::new(),
        }),
        Err(StoreError::InstallTitleTooLarge {
            actual: 5,
            maximum: 4
        })
    ));
    assert!(store.installed_builds().unwrap().is_empty());

    store
        .install(&InstalledBuild {
            principal: principal('b'),
            title: Arc::from("four"),
            manifest_metadata: metadata,
            capability_requests: Vec::new(),
        })
        .unwrap();
    drop(store);
    let reopened = RuntimeStore::open(&path, limits).unwrap();
    assert_eq!(
        reopened.installed_builds().unwrap()[0].title.as_ref(),
        "four"
    );
}

#[test]
fn installed_search_is_deterministic_bounded_and_survives_restart() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("runtime.db");
    {
        let store = RuntimeStore::open(&path, StoreLimits::default()).unwrap();
        install(&store, principal('b'), "Good Morning");
        install(&store, principal('c'), "Weather");
        assert_eq!(
            store.search_installed_builds("MORNING", 2).unwrap()[0]
                .title
                .as_ref(),
            "Good Morning"
        );
        assert!(matches!(
            store.search_installed_builds("", 1),
            Err(StoreError::InstallSearchCapacity {
                actual_at_least: 2,
                maximum: 1
            })
        ));
        assert!(matches!(
            store.search_installed_builds("bad\nquery", 2),
            Err(StoreError::InvalidInstallSearchQuery)
        ));
    }
    let reopened = RuntimeStore::open(&path, StoreLimits::default()).unwrap();
    assert_eq!(
        reopened
            .search_installed_builds("weather", 2)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn uninstall_removes_only_exact_build_runtime_state() {
    let (_directory, store) = store();
    let build = principal('b');
    let receipt = WriteReceiptId(Arc::from("nmp-receipt"));
    install(&store, build.clone(), "Good Morning");
    store
        .set_grant(
            &build,
            &Capability::new("identity").unwrap(),
            GrantDecision::AllowExactBuild,
        )
        .unwrap();
    store
        .put_component_value(&build, "storage", "draft", b"gm")
        .unwrap();
    store
        .save_workspace(&workspace(vec![receipt.clone()]))
        .unwrap();
    store.assign_build_to_workspace("main", &build).unwrap();
    store.append_activity(&activity("launch", "ok")).unwrap();

    let report = store
        .uninstall_exact_build(&build, UninstallCleanupPolicy::RuntimeOwnedExactBuildState)
        .unwrap();
    assert_eq!(
        report,
        UninstallReport {
            installation_removed: true,
            grants_removed: 1,
            component_values_removed: 1,
            workspace_assignments_removed: 1,
        }
    );
    assert!(store.installed_builds().unwrap().is_empty());
    assert_eq!(
        store
            .grant(&build, &Capability::new("identity").unwrap())
            .unwrap(),
        GrantDecision::Denied
    );
    assert_eq!(
        store.component_value(&build, "storage", "draft").unwrap(),
        None
    );
    assert!(store.workspace_assignments("main").unwrap().is_empty());
    assert_eq!(
        store.load_workspaces().unwrap()[0].retained_receipts,
        [receipt]
    );
    assert_eq!(store.activity_records().unwrap().len(), 1);
}

#[test]
fn schema_one_migrates_to_explicit_workspace_assignments() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("runtime.db");
    {
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE runtime_schema (version INTEGER NOT NULL);
                 INSERT INTO runtime_schema(version) VALUES (1);",
            )
            .unwrap();
    }
    let store = RuntimeStore::open(&path, StoreLimits::default()).unwrap();
    assert!(
        store
            .table_names()
            .unwrap()
            .contains(&"workspace_assignments".to_owned())
    );
}

#[test]
fn retained_receipt_count_and_bytes_are_typed_refusals_and_survive_restart() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("runtime.db");
    let limits = StoreLimits {
        maximum_retained_receipts_per_workspace: 1,
        maximum_retained_receipt_bytes_per_workspace: 20,
        ..StoreLimits::default()
    };
    let store = RuntimeStore::open(&path, limits).unwrap();

    assert!(matches!(
        store.save_workspace(&workspace(vec![
            WriteReceiptId(Arc::from("one")),
            WriteReceiptId(Arc::from("two")),
        ])),
        Err(StoreError::RetainedReceiptCapacity {
            actual: 2,
            maximum: 1
        })
    ));
    assert!(matches!(
        store.save_workspace(&workspace(vec![WriteReceiptId(Arc::from(
            "receipt-id-that-does-not-fit"
        ))])),
        Err(StoreError::RetainedReceiptBytes {
            actual,
            maximum: 20
        }) if actual > 20
    ));
    assert!(store.load_workspaces().unwrap().is_empty());

    store
        .save_workspace(&workspace(vec![WriteReceiptId(Arc::from("receipt"))]))
        .unwrap();
    drop(store);
    let reopened = RuntimeStore::open(&path, limits).unwrap();
    assert_eq!(
        reopened.load_workspaces().unwrap()[0].retained_receipts,
        vec![WriteReceiptId(Arc::from("receipt"))]
    );
}

#[test]
fn activity_strings_and_records_are_refused_and_retention_is_count_and_byte_bounded() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("runtime.db");
    let limits = StoreLimits {
        maximum_activity_facts: 2,
        maximum_activity_string_bytes: 8,
        maximum_activity_record_bytes: 16,
        maximum_activity_total_bytes: 28,
        ..StoreLimits::default()
    };
    let store = RuntimeStore::open(&path, limits).unwrap();

    assert!(matches!(
        store.append_activity(&activity("operation", "ok")),
        Err(StoreError::ActivityStringTooLarge {
            field: "operation",
            actual: 9,
            maximum: 8
        })
    ));
    let mut aggregate_too_large = activity("12345678", "12345678");
    aggregate_too_large.category = Arc::from("p");
    assert!(matches!(
        store.append_activity(&aggregate_too_large),
        Err(StoreError::ActivityRecordTooLarge {
            actual: 17,
            maximum: 16
        })
    ));

    store.append_activity(&activity("one", "ok")).unwrap();
    store.append_activity(&activity("two", "ok")).unwrap();
    store.append_activity(&activity("three", "ok")).unwrap();
    assert_eq!(
        store
            .activity_records()
            .unwrap()
            .iter()
            .map(|record| record.operation.as_ref())
            .collect::<Vec<_>>(),
        vec!["two", "three"]
    );

    drop(store);
    let reopened = RuntimeStore::open(&path, limits).unwrap();
    let records = reopened.activity_records().unwrap();
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .map(|record| { record.category.len() + record.operation.len() + record.outcome.len() })
            .sum::<usize>()
            <= limits.maximum_activity_total_bytes
    );
}

#[test]
fn schema_contains_no_parallel_nostr_truth() {
    let (_directory, store) = store();
    let names = store.table_names().unwrap();
    for forbidden in [
        "events",
        "replacements",
        "deletions",
        "pending_rows",
        "receipt_facts",
        "relay_routes",
    ] {
        assert!(!names.iter().any(|name| name == forbidden));
    }
}
