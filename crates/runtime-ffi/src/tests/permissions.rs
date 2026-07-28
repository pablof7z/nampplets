use super::*;

#[test]
fn no_published_build_receives_a_runtime_pinned_capability_profile() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp);
    let artifact = controller
        .verify_artifact(
            EVENT.to_vec(),
            ArtifactCoordinate::Named {
                author: AUTHOR.to_owned(),
                d_tag: D_TAG.to_owned(),
            },
        )
        .artifact
        .expect("published fixture verifies");
    assert!(
        artifact.requires().is_empty(),
        "the immutable manifest declares no signed `requires` tags"
    );

    controller.install(Arc::clone(&artifact));
    let review = controller
        .permission_review(exact_coordinate(&artifact))
        .review
        .expect("the installed exact build has a permission review");
    // This exact build is the one the runtime used to special-case by
    // author/d-tag/aggregate into a pinned identity/inc/outbox-required,
    // link/resource/theme-optional profile. That pin is gone. The inventory
    // now comes only from the fixture's own `napplet-requires` meta, inside
    // bytes the signed path digest and aggregate already pin -- so it is
    // every declared domain, all required, and nothing the build did not ask
    // for.
    assert_eq!(
        review
            .capabilities
            .iter()
            .map(|capability| (capability.domain.as_str(), capability.requirement))
            .collect::<Vec<_>>(),
        GOOD_MORNING_DECLARED_DOMAINS
            .iter()
            .map(|domain| (*domain, RuntimePermissionRequirement::Required))
            .collect::<Vec<_>>(),
        "the inventory must be the artifact's own declaration, nothing else"
    );
    assert!(
        !review.launch_permitted,
        "a declared required domain is still enforced before execution"
    );

    controller.launch(Arc::clone(&artifact), RuntimeExecutionProfile::Legacy);
    assert!(
        controller.snapshot_value().sessions.is_empty(),
        "declared required capabilities are enforced before execution"
    );

    // Granting exactly what it declared is what lets it run -- its identity
    // never was and never becomes a shortcut around that.
    for domain in GOOD_MORNING_DECLARED_DOMAINS {
        controller.set_grant(
            Arc::clone(&artifact),
            domain.to_owned(),
            RuntimeSensitivity::Sensitive,
            RuntimeGrantDecision::AllowExactBuild,
        );
    }
    controller.launch(Arc::clone(&artifact), RuntimeExecutionProfile::Legacy);
    let snapshot = controller.snapshot_value();
    assert_eq!(snapshot.sessions.len(), 1);
    // `link` and `resource` have no registered provider in this runtime at
    // all, so `RuntimeApp::launch` partitions them out and records a
    // `required-domain-unavailable` activity rather than injecting them. The
    // session comes up with exactly what the runtime can actually deliver --
    // the same four domains the removed pin produced.
    assert_eq!(
        snapshot.sessions[0].domains,
        ["identity", "inc", "outbox", "shell"],
        "only domains with a registered provider may be injected"
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
        review_revision: initial.revision.clone(),
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
        RuntimePermissionChangeRefusalCode::DuplicateCapability
    );

    let applied = runtime.apply_permission_decisions(RuntimePermissionDecisionBatch {
        coordinate: coordinate.clone(),
        review_revision: initial.revision,
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
fn outbox_grant_survives_default_profile_restart() {
    let temp = TempDir::new().unwrap();
    let (event, author, digest) = signed_manifest_event(
        "restart-grant-test",
        b"<html>restart-grant</html>",
        vec![
            vec!["requires".to_owned(), "identity".to_owned()],
            vec!["requires".to_owned(), "inc".to_owned()],
            vec!["requires".to_owned(), "outbox".to_owned()],
        ],
    );
    let coordinate = ArtifactCoordinate::Named {
        author: author.clone(),
        d_tag: "restart-grant-test".to_owned(),
    };
    let runtime = RuntimeController::open(
        RuntimeConfig {
            runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
            nmp_store_path: None,
            artifact_cache_path: temp.path().join("artifacts").display().to_string(),
            ..RuntimeConfig::default()
        },
        Box::new(FixtureSource(BTreeMap::from([(
            digest.clone(),
            b"<html>restart-grant</html>".to_vec(),
        )]))),
    )
    .unwrap();
    let artifact = runtime
        .verify_artifact(event.clone(), coordinate.clone())
        .artifact
        .expect("locally signed fixture verifies");
    runtime.install(Arc::clone(&artifact));
    let coordinate = exact_coordinate(&artifact);
    let review = runtime
        .permission_review(coordinate.clone())
        .review
        .expect("installed napplet has a permission review");
    let update = runtime.apply_permission_decisions(RuntimePermissionDecisionBatch {
        coordinate: coordinate.clone(),
        review_revision: review.revision.clone(),
        decisions: review
            .capabilities
            .iter()
            .map(|capability| RuntimePermissionDecisionSelection {
                domain: capability.domain.clone(),
                decision: RuntimeGrantDecision::AllowExactBuild,
            })
            .collect(),
    });
    assert!(update.applied);
    assert!(update.review.unwrap().launch_permitted);
    runtime.close();
    drop(runtime);

    let reopened = RuntimeController::open(
        RuntimeConfig {
            runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
            nmp_store_path: None,
            artifact_cache_path: temp.path().join("artifacts").display().to_string(),
            ..RuntimeConfig::default()
        },
        Box::new(FixtureSource(BTreeMap::from([(
            digest,
            b"<html>restart-grant</html>".to_vec(),
        )]))),
    )
    .unwrap();
    let artifact = reopened
        .verify_artifact(
            event,
            ArtifactCoordinate::Named {
                author,
                d_tag: "restart-grant-test".to_owned(),
            },
        )
        .artifact
        .expect("locally signed fixture verifies after restart");
    reopened.install(Arc::clone(&artifact));
    let review = reopened
        .permission_review(coordinate)
        .review
        .expect("review restores after restart");
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
fn malformed_declared_config_schema_reaches_the_snapshot_surface() {
    // The sibling unit tests in `controller::support` prove the parse half
    // returns an error instead of `None`. They cannot prove anyone ever sees
    // it. This asserts the whole seam: a malformed declaration becomes a
    // boundary refusal that is actually present in the snapshot the host
    // reads, which is the difference between a signal existing and a signal
    // arriving.
    //
    // The launch must still succeed. Failing open is the deliberate choice
    // here -- refusing to launch over a bad config schema would be a worse
    // answer than launching without one -- but failing open silently is what
    // this fixes: `config.subscribe` otherwise answers `no-schema` forever
    // for a napplet that plainly did declare a schema, and nothing anywhere
    // records why.
    let temp = TempDir::new().unwrap();
    let content: &[u8] =
        br#"<head><meta name="napplet-config-schema" content="{not valid json"></head><body></body>"#;
    let (event, author, digest) =
        signed_manifest_event("malformed-config-schema-test", content, Vec::new());
    let coordinate = ArtifactCoordinate::Named {
        author,
        d_tag: "malformed-config-schema-test".to_owned(),
    };
    let controller = RuntimeController::open(
        RuntimeConfig {
            runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
            nmp_store_path: None,
            artifact_cache_path: temp.path().join("artifacts").display().to_string(),
            ..RuntimeConfig::default()
        },
        Box::new(FixtureSource(BTreeMap::from([(digest, content.to_vec())]))),
    )
    .unwrap();
    let artifact = controller
        .verify_artifact(event, coordinate)
        .artifact
        .expect("locally signed fixture with no `requires` tags verifies");

    controller.install(Arc::clone(&artifact));
    controller.launch(artifact, RuntimeExecutionProfile::Legacy);

    let snapshot = controller.snapshot_value();
    assert_eq!(
        snapshot.sessions.len(),
        1,
        "a malformed config schema must not block the launch itself"
    );
    let refusal = snapshot
        .boundary_refusals
        .iter()
        .find(|refusal| refusal.code == "config-schema-malformed")
        .expect("the malformed declaration must be a recorded boundary refusal");
    assert!(
        refusal.detail.contains("napplet-config-schema"),
        "the refusal detail should name what was malformed: {}",
        refusal.detail
    );
}

/// Reproduces the defect behind the "This app can't tell whether that works
/// here" caution the permission sheet used to show for `lists`: the runtime
/// registered no provider for that domain, so the review could only project
/// `Unknown`. A registered provider is the whole difference between a caution
/// and a usable choice.
#[test]
fn a_napplet_requesting_lists_gets_a_definite_verdict_and_a_real_choice() {
    let temp = TempDir::new().unwrap();
    let runtime = controller(&temp);
    let coordinate = install_lists_fixture(&runtime);

    let review = runtime.permission_review(coordinate).review.unwrap();
    let lists = review
        .capabilities
        .iter()
        .find(|capability| capability.domain == "lists")
        .expect("the review lists the requested lists capability");

    assert_eq!(
        lists.platform_availability,
        RuntimePermissionPlatformAvailability::Available,
        "lists must not project as unknown once its provider is registered"
    );
    assert_eq!(
        lists.sensitivity,
        RuntimePermissionSensitivity::Sensitive,
        "changing who you follow or mute is social-graph data"
    );
    // The sheet's Allow switch is only usable when an affirmative decision is
    // actually offered as valid.
    assert!(
        lists
            .decision_options
            .iter()
            .any(|option| option.valid && option.decision == RuntimeGrantDecision::AllowExactBuild),
        "the user is offered a real affirmative choice"
    );
}
