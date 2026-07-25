//! Permission review projection, grant batches, and revocation authority.

mod support;

use std::{collections::BTreeSet, sync::Arc};

use nmp_native_nap_bridge::ProviderPushError;
use nmp_native_runtime_app::{
    AppErrorCode, PermissionPlatformAvailability, PlatformCommand, PlatformEvent,
};
use nmp_native_runtime_core::{
    Capability, CapabilityRequirement, ExecutionProfile, GrantDecision, Sensitivity,
};
use nmp_native_runtime_store::{InstalledBuild, RuntimeStore, StoreLimits, UninstallCleanupPolicy};
use nmp_native_test_harness::FakeHostDataPlane;
use support::*;
use tempfile::TempDir;

#[test]
fn permission_review_is_exact_bounded_and_required_denial_blocks_launch() {
    let rig = Rig::new(false);
    let exact = principal('b');
    let missing = Capability::new("missing").unwrap();
    rig.install_with_requests(
        exact.clone(),
        vec![
            request(canary(), CapabilityRequirement::Required),
            request(missing.clone(), CapabilityRequirement::Optional),
        ],
    );

    let review = rig.app.permission_review(&exact).unwrap();
    assert_eq!(review.principal, exact);
    assert_eq!(review.capabilities.len(), 2);
    assert_eq!(review.capabilities[0].capability, canary());
    assert_eq!(
        review.capabilities[0].platform_availability,
        PermissionPlatformAvailability::Available
    );
    assert_eq!(
        review.capabilities[0].sensitivity,
        Some(Sensitivity::Ordinary)
    );
    assert_eq!(
        review.capabilities[1].platform_availability,
        PermissionPlatformAvailability::Unknown {
            reason: Arc::from(
                "no provider metadata is registered for this capability on this runtime"
            )
        }
    );
    assert_eq!(
        review.capabilities[1].requested_decision,
        Some(GrantDecision::Denied)
    );
    assert!(
        review.capabilities[1]
            .decision_options
            .iter()
            .all(|option| option.decision == GrantDecision::Denied || !option.valid)
    );

    rig.app.dispatch(PlatformCommand::ApplyPermissionBatch {
        principal: exact.clone(),
        decisions: vec![
            permission(canary(), GrantDecision::Denied),
            permission(missing, GrantDecision::Denied),
        ],
    });
    assert!(matches!(
        rig.app.events_after(0).events.last().unwrap().event,
        PlatformEvent::PermissionBatchApplied { .. }
    ));
    rig.app.dispatch(PlatformCommand::Launch {
        principal: exact,
        profile: ExecutionProfile::Legacy,
        required_domains: BTreeSet::from([canary()]),
    });
    assert!(rig.app.snapshot().sessions.is_empty());
    assert_eq!(
        rig.app.snapshot().recent_errors.last().unwrap().code,
        AppErrorCode::Bridge
    );
}

#[test]
fn required_capability_with_no_registered_provider_does_not_block_launch() {
    // A manifest may `requires` a domain from the wider known vocabulary
    // that this runtime build has no provider for at all (no descriptor,
    // `PermissionPlatformAvailability::Unknown`). Such a domain can never
    // receive a decision, so treating it as launch-blocking would make the
    // napplet permanently unlaunchable. Launch must drop it and proceed
    // with every domain it can actually grant.
    let rig = Rig::new(false);
    let exact = principal('c');
    let missing = Capability::new("missing").unwrap();
    rig.install_with_requests(
        exact.clone(),
        vec![
            request(canary(), CapabilityRequirement::Required),
            request(missing.clone(), CapabilityRequirement::Required),
        ],
    );
    rig.allow_runtime(exact.clone());

    rig.app.dispatch(PlatformCommand::Launch {
        principal: exact.clone(),
        profile: ExecutionProfile::Legacy,
        required_domains: BTreeSet::from([canary(), missing.clone()]),
    });

    let snapshot = rig.app.snapshot();
    let session = snapshot
        .sessions
        .last()
        .expect("launch succeeds without the unregistered domain");
    let domains = &snapshot
        .session_domains
        .iter()
        .find(|view| view.session == session.id)
        .unwrap()
        .domains;
    assert!(domains.contains(&canary()));
    assert!(!domains.contains(&missing));
    assert!(snapshot.recent_activity.iter().any(|fact| {
        fact.operation.as_ref() == "required-domain-unavailable"
            && fact.outcome.as_ref() == "missing"
    }));
}

#[test]
fn permission_batch_revokes_live_work_without_overwriting_ask_every_time() {
    let rig = Rig::new(true);
    let exact = principal('b');
    rig.install_with_requests(
        exact.clone(),
        vec![request(canary(), CapabilityRequirement::Required)],
    );
    rig.app.dispatch(PlatformCommand::ApplyPermissionBatch {
        principal: exact.clone(),
        decisions: vec![permission(canary(), GrantDecision::AllowExactBuild)],
    });
    let session = rig.launch(exact.clone());
    let sender = rig.provider.sender(session);
    rig.ready(session);
    rig.app.dispatch(PlatformCommand::MappedEnvelope {
        session,
        bytes: ping(serde_json::json!({})),
    });
    assert_eq!(rig.app.snapshot().resources.admitted, 3);

    rig.app.dispatch(PlatformCommand::ApplyPermissionBatch {
        principal: exact.clone(),
        decisions: vec![permission(canary(), GrantDecision::AskEveryTime)],
    });

    assert_eq!(rig.app.snapshot().resources.admitted, 2);
    assert_eq!(
        rig.app.permission_review(&exact).unwrap().capabilities[0].current_decision,
        GrantDecision::AskEveryTime
    );
    assert_eq!(
        rig.store.grant(&exact, &canary()).unwrap(),
        GrantDecision::AskEveryTime
    );
    assert_eq!(
        sender.push("canary.state", serde_json::Map::new(), None),
        Err(ProviderPushError::Revoked)
    );
    assert_eq!(rig.provider.revoked.lock().len(), 1);
}

#[test]
fn permission_batch_store_failure_changes_neither_ledger_nor_outcome() {
    let rig = Rig::new(false);
    let exact = principal('b');
    rig.install_with_requests(
        exact.clone(),
        vec![request(canary(), CapabilityRequirement::Required)],
    );
    rig.store
        .uninstall_exact_build(&exact, UninstallCleanupPolicy::RuntimeOwnedExactBuildState)
        .unwrap();

    rig.app.dispatch(PlatformCommand::ApplyPermissionBatch {
        principal: exact.clone(),
        decisions: vec![permission(canary(), GrantDecision::AllowExactBuild)],
    });

    assert_eq!(
        rig.app.permission_review(&exact).unwrap().capabilities[0].current_decision,
        GrantDecision::Denied
    );
    assert_eq!(
        rig.app.snapshot().recent_errors.last().unwrap().code,
        AppErrorCode::Store
    );
    assert!(
        !rig.app
            .events_after(0)
            .events
            .iter()
            .any(|event| matches!(event.event, PlatformEvent::PermissionBatchApplied { .. }))
    );
}

#[test]
fn permission_batch_cannot_override_managed_host_policy() {
    let rig = Rig::new(false);
    let exact = principal('b');
    rig.install_with_requests(
        exact.clone(),
        vec![request(canary(), CapabilityRequirement::Required)],
    );
    rig.store
        .set_grant(&exact, &canary(), GrantDecision::Managed)
        .unwrap();
    let review = rig.app.permission_review(&exact).unwrap();
    assert_eq!(
        review.capabilities[0].current_decision,
        GrantDecision::Managed
    );
    assert_eq!(review.capabilities[0].requested_decision, None);

    rig.app.dispatch(PlatformCommand::ApplyPermissionBatch {
        principal: exact.clone(),
        decisions: vec![permission(canary(), GrantDecision::Denied)],
    });

    assert_eq!(
        rig.store.grant(&exact, &canary()).unwrap(),
        GrantDecision::Managed
    );
    assert_eq!(
        rig.app.snapshot().recent_errors.last().unwrap().code,
        AppErrorCode::Grant
    );
}

#[test]
fn permission_batch_persists_and_dependency_policy_is_owner_validated() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("runtime.db");
    let store = Arc::new(RuntimeStore::open(&path, StoreLimits::default()).unwrap());
    let host = Arc::new(FakeHostDataPlane::new(16));
    let dependency = Capability::new("identity").unwrap();
    let provider = Arc::new(CapturingProvider::new(false).with_dependencies([dependency]));
    let (app, _) = open_app(Arc::clone(&store), host, provider);
    let exact = principal('b');
    app.dispatch(PlatformCommand::InstallVerified {
        build: InstalledBuild {
            principal: exact.clone(),
            title: Arc::from("Dependent napplet"),
            manifest_metadata: json(serde_json::json!({"kind": 35129})),
            capability_requests: vec![request(canary(), CapabilityRequirement::Required)],
        },
        artifact: Arc::new(TestArtifact {
            kind: 35_129,
            author: exact.manifest_author().to_owned(),
            d_tag: exact.d_tag().to_owned(),
            aggregate: exact.aggregate_hash().to_owned(),
        }),
    });
    app.dispatch(PlatformCommand::ApplyPermissionBatch {
        principal: exact.clone(),
        decisions: vec![permission(canary(), GrantDecision::AllowExactBuild)],
    });
    assert_eq!(
        app.snapshot().recent_errors.last().unwrap().code,
        AppErrorCode::Grant
    );

    app.dispatch(PlatformCommand::SetGrant {
        principal: exact.clone(),
        capability: Capability::new("identity").unwrap(),
        sensitivity: Sensitivity::Sensitive,
        decision: GrantDecision::AllowExactBuild,
    });
    app.dispatch(PlatformCommand::ApplyPermissionBatch {
        principal: exact.clone(),
        decisions: vec![permission(canary(), GrantDecision::AllowExactBuild)],
    });
    assert_eq!(
        app.permission_review(&exact).unwrap().capabilities[0].current_decision,
        GrantDecision::AllowExactBuild
    );
    app.dispatch(PlatformCommand::Close);
    drop(app);
    drop(store);

    let reopened_store = Arc::new(RuntimeStore::open(&path, StoreLimits::default()).unwrap());
    let reopened_host = Arc::new(FakeHostDataPlane::new(16));
    let reopened_provider = Arc::new(
        CapturingProvider::new(false).with_dependencies([Capability::new("identity").unwrap()]),
    );
    let (reopened, _) = open_app(reopened_store, reopened_host, reopened_provider);
    assert_eq!(
        reopened.permission_review(&exact).unwrap().capabilities[0].current_decision,
        GrantDecision::AllowExactBuild
    );
}

#[test]
fn exact_build_revoke_does_not_cancel_another_principals_operation() {
    let rig = Rig::new(true);
    let first_principal = principal('b');
    let second_principal = principal('c');
    for principal in [first_principal.clone(), second_principal.clone()] {
        rig.install(principal.clone());
        rig.allow_runtime(principal);
    }
    let first = rig.launch(first_principal.clone());
    let second = rig.launch(second_principal);
    let first_sender = rig.provider.sender(first);
    let second_sender = rig.provider.sender(second);
    assert_ne!(
        first_sender.source_window(),
        second_sender.source_window(),
        "each launch owns an opaque source-window identity"
    );
    for session in [first, second] {
        rig.ready(session);
        rig.app.dispatch(PlatformCommand::MappedEnvelope {
            session,
            bytes: ping(serde_json::json!({})),
        });
    }
    assert_eq!(rig.app.snapshot().resources.admitted, 6);

    rig.app.dispatch(PlatformCommand::Revoke {
        principal: first_principal,
        capability: canary(),
    });
    assert_eq!(
        rig.app.snapshot().resources.admitted,
        5,
        "only the revoked exact build's operation is cancelled"
    );
    assert_eq!(
        first_sender.push("canary.state", serde_json::Map::new(), None),
        Err(ProviderPushError::Revoked)
    );
    second_sender
        .push(
            "canary.state",
            serde_json::Map::from_iter([("owner".to_owned(), serde_json::json!("second"))]),
            None,
        )
        .unwrap();
    let _ = wait_for_event(&rig.app, |event| {
        matches!(
            event,
            nmp_native_runtime_app::PlatformEvent::ProviderPush {
                session: pushed_session,
                ..
            } if *pushed_session == second
        )
    });
    rig.app.dispatch(PlatformCommand::Stop { session: first });
    rig.app.dispatch(PlatformCommand::Stop { session: second });
    assert_eq!(rig.app.snapshot().resources.admitted, 0);
}
