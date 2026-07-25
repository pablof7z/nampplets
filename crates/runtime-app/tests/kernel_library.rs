//! Installed-library identity, filtering, offline honesty, and uninstall.

mod support;

use std::{collections::BTreeSet, sync::Arc};

use nmp_native_runtime_app::{AppErrorCode, InstalledBuildAvailability, PlatformCommand};
use nmp_native_runtime_core::{ExecutionProfile, GrantDecision, WriteReceiptId};
use nmp_native_runtime_store::{InstalledBuild, UninstallCleanupPolicy, WorkspaceRecord};
use support::*;

#[test]
fn snapshot_manifest_without_d_tag_is_a_typed_identity_refusal() {
    let rig = Rig::new(false);
    let principal = principal('b');
    rig.app.dispatch(PlatformCommand::InstallVerified {
        build: InstalledBuild {
            principal: principal.clone(),
            title: Arc::from("Snapshot napplet"),
            manifest_metadata: json(serde_json::json!({"kind": 5129})),
            capability_requests: Vec::new(),
        },
        artifact: Arc::new(TestArtifact {
            kind: 5_129,
            author: principal.manifest_author().to_owned(),
            d_tag: String::new(),
            aggregate: principal.aggregate_hash().to_owned(),
        }),
    });
    assert_eq!(
        rig.app.snapshot().recent_errors.last().unwrap().code,
        AppErrorCode::UnsupportedManifestIdentity
    );
    assert!(rig.app.snapshot().sessions.is_empty());
    assert!(rig.store.installed_builds().unwrap().is_empty());
}

#[test]
fn library_filter_and_metadata_restore_are_bounded_and_offline_honest() {
    let rig = Rig::new(false);
    let first = principal('b');
    let second = principal('c');
    rig.install(first.clone());
    rig.install(second.clone());
    assert_eq!(rig.app.snapshot().library.total_installed, 2);
    assert!(
        rig.app.snapshot().library.builds.iter().all(|build| {
            build.availability == InstalledBuildAvailability::SealedExactBytesReady
        })
    );

    rig.app.dispatch(PlatformCommand::SetLibraryFilter {
        query: Arc::from(second.aggregate_hash()),
    });
    assert_eq!(rig.app.snapshot().library.builds.len(), 1);
    assert_eq!(rig.app.snapshot().library.builds[0].build.principal, second);
    rig.app.dispatch(PlatformCommand::Close);

    let (restored, _) = open_app(
        Arc::clone(&rig.store),
        rig.host.clone(),
        rig.provider.clone(),
    );
    let snapshot = restored.snapshot();
    assert_eq!(snapshot.library.total_installed, 2);
    assert!(snapshot.library.builds.iter().all(|build| {
        build.availability == InstalledBuildAvailability::MetadataOnly
            && build.active_sessions.is_empty()
    }));
    restored.dispatch(PlatformCommand::Launch {
        principal: first,
        profile: ExecutionProfile::Legacy,
        required_domains: BTreeSet::new(),
    });
    assert_eq!(
        restored.snapshot().recent_errors.last().unwrap().code,
        AppErrorCode::OfflineBytesUnavailable
    );
}

#[test]
fn uninstall_stops_only_exact_build_and_cleans_runtime_owned_state() {
    let rig = Rig::new(false);
    let removed = principal('b');
    let retained = principal('c');
    for principal in [removed.clone(), retained.clone()] {
        rig.install(principal.clone());
        rig.allow_runtime(principal);
    }
    rig.store
        .put_component_value(&removed, "storage", "draft", b"gm")
        .unwrap();
    let receipt_id = WriteReceiptId(Arc::from("nmp-owned-receipt"));
    rig.app.dispatch(PlatformCommand::SaveWorkspace {
        workspace: WorkspaceRecord {
            id: Arc::from("main"),
            definition: json(serde_json::json!({"layout": "two-up"})),
            retained_receipts: vec![receipt_id.clone()],
        },
    });
    rig.app.dispatch(PlatformCommand::AssignWorkspaceBuild {
        workspace_id: Arc::from("main"),
        principal: removed.clone(),
    });
    let removed_session = rig.launch(removed.clone());
    let retained_session = rig.launch(retained.clone());
    assert_eq!(rig.app.snapshot().sessions.len(), 2);

    rig.app.dispatch(PlatformCommand::Uninstall {
        principal: removed.clone(),
        cleanup: UninstallCleanupPolicy::RuntimeOwnedExactBuildState,
    });

    let snapshot = rig.app.snapshot();
    assert_eq!(snapshot.library.total_installed, 1);
    assert_eq!(snapshot.library.builds[0].build.principal, retained);
    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(snapshot.sessions[0].id, retained_session);
    assert_eq!(
        rig.store
            .component_value(&removed, "storage", "draft")
            .unwrap(),
        None
    );
    assert_eq!(
        rig.store.grant(&removed, &canary()).unwrap(),
        GrantDecision::Denied
    );
    assert!(rig.store.workspace_assignments("main").unwrap().is_empty());
    assert_eq!(
        rig.store.load_workspaces().unwrap()[0].retained_receipts,
        [receipt_id]
    );
    assert!(snapshot.workspaces[0].assigned_builds.is_empty());

    rig.app.dispatch(PlatformCommand::Resume {
        session: removed_session,
    });
    assert_eq!(
        rig.app.snapshot().recent_errors.last().unwrap().code,
        AppErrorCode::UnknownSession
    );
    rig.app.dispatch(PlatformCommand::Stop {
        session: retained_session,
    });
}
