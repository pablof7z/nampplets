use super::*;

#[test]
fn pinned_good_morning_installs_rust_owned_permission_profile() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp);
    let artifact = controller
        .verify_artifact(
            EVENT.to_vec(),
            ArtifactCoordinate::Named {
                author: AUTHOR.to_owned(),
                d_tag: GOOD_MORNING_D_TAG.to_owned(),
            },
        )
        .artifact
        .expect("published fixture verifies");
    assert!(
        artifact.requires().is_empty(),
        "the immutable manifest remains unchanged"
    );

    controller.install(Arc::clone(&artifact));
    let review = controller
        .permission_review(exact_coordinate(&artifact))
        .review
        .expect("the installed exact build has a permission review");
    assert_eq!(
        review
            .capabilities
            .iter()
            .map(|capability| { (capability.domain.as_str(), capability.requirement) })
            .collect::<Vec<_>>(),
        vec![
            ("identity", RuntimePermissionRequirement::Required),
            ("inc", RuntimePermissionRequirement::Required),
            ("outbox", RuntimePermissionRequirement::Required),
            ("resource", RuntimePermissionRequirement::Optional),
            ("theme", RuntimePermissionRequirement::Optional),
            ("link", RuntimePermissionRequirement::Optional),
        ]
    );
    assert!(!review.launch_permitted);
    let outbox = review
        .capabilities
        .iter()
        .find(|capability| capability.domain == "outbox")
        .expect("outbox permission");
    assert_eq!(outbox.sensitivity, RuntimePermissionSensitivity::Sensitive);

    controller.launch(artifact, RuntimeExecutionProfile::Legacy);
    assert!(
        controller.snapshot_value().sessions.is_empty(),
        "required compatibility capabilities are enforced before execution"
    );
}

#[test]
fn permission_review_and_atomic_batch_are_exact_typed_and_restart_safe() {
    let temp = TempDir::new().unwrap();
    let runtime = controller(&temp);
    let coordinate = install_permission_fixture(&runtime);

    let initial = runtime.permission_review(coordinate.clone());
    assert!(initial.refusal.is_none());
    let initial = initial.review.unwrap();
    assert_eq!(initial.coordinate, coordinate);
    assert_eq!(initial.capabilities.len(), 2);
    assert_eq!(initial.capabilities[0].domain, "identity");
    assert_eq!(
        initial.capabilities[0].platform_availability,
        RuntimePermissionPlatformAvailability::Available
    );
    assert_eq!(
        initial.capabilities[0].sensitivity,
        RuntimePermissionSensitivity::Sensitive
    );
    assert_eq!(initial.capabilities[0].decision_options.len(), 4);
    assert_eq!(
        initial.capabilities[1].platform_availability,
        RuntimePermissionPlatformAvailability::Unknown {
            reason: "no provider metadata is registered for this capability on this runtime"
                .to_owned()
        }
    );

    let duplicate = runtime.apply_permission_decisions(RuntimePermissionDecisionBatch {
        coordinate: coordinate.clone(),
        decisions: vec![
            RuntimePermissionDecisionSelection {
                domain: "identity".to_owned(),
                decision: RuntimeGrantDecision::AllowExactBuild,
            },
            RuntimePermissionDecisionSelection {
                domain: "identity".to_owned(),
                decision: RuntimeGrantDecision::Denied,
            },
        ],
    });
    assert!(!duplicate.applied);
    assert_eq!(
        duplicate.refusal.unwrap().code,
        "duplicate-permission-domain"
    );

    let applied = runtime.apply_permission_decisions(RuntimePermissionDecisionBatch {
        coordinate: coordinate.clone(),
        decisions: vec![
            RuntimePermissionDecisionSelection {
                domain: "identity".to_owned(),
                decision: RuntimeGrantDecision::AllowExactBuild,
            },
            RuntimePermissionDecisionSelection {
                domain: "missing".to_owned(),
                decision: RuntimeGrantDecision::Denied,
            },
        ],
    });
    assert!(applied.applied);
    assert!(applied.refusal.is_none());
    let applied_review = applied.review.unwrap();
    assert!(applied_review.launch_permitted);
    assert_eq!(
        applied_review.capabilities[0].existing_decision,
        RuntimePermissionExistingDecision::AllowExactBuild
    );
    runtime.close();
    drop(runtime);

    let reopened = controller(&temp);
    let restored = reopened.permission_review(coordinate).review.unwrap();
    assert_eq!(restored.capabilities.len(), 2);
    assert_eq!(
        restored.capabilities[0].existing_decision,
        RuntimePermissionExistingDecision::AllowExactBuild
    );
    assert!(restored.launch_permitted);
}

#[test]
fn good_morning_outbox_grant_survives_default_profile_restart() {
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
        .expect("published fixture verifies");
    runtime.install(Arc::clone(&artifact));
    let coordinate = exact_coordinate(&artifact);
    let review = runtime
        .permission_review(coordinate.clone())
        .review
        .expect("installed Good Morning has a permission review");
    let update = runtime.apply_permission_decisions(RuntimePermissionDecisionBatch {
        coordinate: coordinate.clone(),
        decisions: review
            .capabilities
            .iter()
            .map(|capability| RuntimePermissionDecisionSelection {
                domain: capability.domain.clone(),
                decision: match capability.requirement {
                    RuntimePermissionRequirement::Required => RuntimeGrantDecision::AllowExactBuild,
                    RuntimePermissionRequirement::Optional => RuntimeGrantDecision::Denied,
                },
            })
            .collect(),
    });
    assert!(update.applied);
    assert!(update.review.unwrap().launch_permitted);
    runtime.close();
    drop(runtime);

    let reopened = controller(&temp);
    let artifact = reopened
        .verify_artifact(
            EVENT.to_vec(),
            ArtifactCoordinate::Named {
                author: AUTHOR.to_owned(),
                d_tag: "good-morning".to_owned(),
            },
        )
        .artifact
        .expect("published fixture verifies after restart");
    reopened.install(Arc::clone(&artifact));
    let review = reopened
        .permission_review(coordinate)
        .review
        .expect("Good Morning review restores after restart");
    for domain in ["identity", "inc", "outbox"] {
        let capability = review
            .capabilities
            .iter()
            .find(|capability| capability.domain == domain)
            .unwrap_or_else(|| panic!("missing required {domain} capability"));
        assert_eq!(
            capability.existing_decision,
            RuntimePermissionExistingDecision::AllowExactBuild
        );
    }
    assert!(review.launch_permitted);

    reopened.launch(artifact, RuntimeExecutionProfile::Legacy);
    let session = reopened.snapshot_value().sessions[0].clone();
    assert_eq!(
        session.domains,
        ["identity", "inc", "outbox", "shell"],
        "the restored exact-build grant must negotiate NAP-OUTBOX"
    );
    reopened.mapped_envelope(session.id, br#"{"type":"shell.ready"}"#.to_vec());
    assert_eq!(
        response_of_type(&reopened, "shell.init")["capabilities"]["domains"],
        serde_json::json!(["identity", "inc", "outbox", "shell"]),
        "the trusted shell must receive the same Rust-negotiated domain set"
    );
}

#[test]
fn demo_profile_repairs_a_persisted_denied_outbox_grant() {
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
        .expect("published fixture verifies");
    runtime.install(Arc::clone(&artifact));
    let coordinate = exact_coordinate(&artifact);
    let review = runtime
        .permission_review(coordinate.clone())
        .review
        .expect("installed Good Morning has a permission review");
    let denied = runtime.apply_permission_decisions(RuntimePermissionDecisionBatch {
        coordinate: coordinate.clone(),
        decisions: review
            .capabilities
            .iter()
            .map(|capability| RuntimePermissionDecisionSelection {
                domain: capability.domain.clone(),
                decision: RuntimeGrantDecision::Denied,
            })
            .collect(),
    });
    assert!(denied.applied);
    assert!(!denied.review.unwrap().launch_permitted);
    runtime.close();
    drop(runtime);

    let demo = RuntimeController::open(
        RuntimeConfig {
            runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
            nmp_store_path: None,
            artifact_cache_path: temp.path().join("artifacts").display().to_string(),
            permission_mode: RuntimePermissionMode::DemoPinnedGoodMorning,
            ..RuntimeConfig::default()
        },
        Box::new(FixtureSource(BTreeMap::from([(
            DIGEST.to_owned(),
            INDEX.to_vec(),
        )]))),
    )
    .unwrap();
    let repaired = demo
        .permission_review(coordinate)
        .review
        .expect("demo startup restores the installed exact build review");
    for domain in ["identity", "inc", "outbox"] {
        let capability = repaired
            .capabilities
            .iter()
            .find(|capability| capability.domain == domain)
            .unwrap_or_else(|| panic!("missing required {domain} capability"));
        assert_eq!(
            capability.existing_decision,
            RuntimePermissionExistingDecision::AllowExactBuild,
            "demo startup must repair persisted denial for {domain}"
        );
    }
    assert!(repaired.launch_permitted);
    demo.close();
}
