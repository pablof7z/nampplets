use super::*;

#[test]
fn missing_required_domain_rejects_before_session_execution() {
    let (registry, principal, _, _) = fixture(5);
    let required = BTreeSet::from([Capability::new("ble").unwrap()]);
    assert!(matches!(
        registry.negotiate(&principal, ExecutionProfile::Legacy, &required),
        Err(BridgeError::MissingRequiredDomains { .. })
    ));
    assert_eq!(registry.census().sessions, 0);
}

#[test]
fn foundational_shell_is_grantless_but_no_other_domain_bypasses_grants() {
    let (mut registry, principal, grants, storage) = fixture(8);
    let shell = Capability::new("shell").unwrap();
    let shell_calls = Arc::new(AtomicUsize::new(0));
    registry
        .register(Arc::new(EchoProvider {
            descriptor: ProviderDescriptor {
                domain: shell.clone(),
                protocol_versions: BTreeSet::from([Arc::from("NAP-SHELL")]),
                actions: BTreeSet::from([Arc::from("ready")]),
                sensitive: false,
                dependencies: BTreeSet::new(),
                platform_availability: ProviderPlatformAvailability::Available,
            },
            calls: Arc::clone(&shell_calls),
        }))
        .unwrap();
    let context = SessionContext {
        id: SessionId(1),
        principal: principal.clone(),
        profile: ExecutionProfile::Legacy,
    };
    registry.open_session(&context, 0).unwrap();
    assert_eq!(
        grants.decision(&principal, &shell),
        GrantDecision::Denied,
        "shell has no synthetic grant"
    );
    let plan = registry
        .negotiate(&principal, ExecutionProfile::Legacy, &BTreeSet::new())
        .unwrap();
    assert!(plan.exposes(&shell));
    assert!(matches!(
        registry
            .dispatch(&context, &plan, br#"{"type":"shell.ready"}"#, 0)
            .unwrap(),
        DispatchOutcome::Handled(_)
    ));
    assert_eq!(shell_calls.load(Ordering::Relaxed), 1);

    registry.revoke(&principal, &shell);
    let after_shell_revoke = registry
        .negotiate(&principal, ExecutionProfile::Legacy, &BTreeSet::new())
        .unwrap();
    assert!(after_shell_revoke.exposes(&shell));
    assert!(
        registry
            .dispatch(
                &context,
                &after_shell_revoke,
                br#"{"type":"shell.ready"}"#,
                0,
            )
            .is_ok()
    );

    grants
        .set(
            principal.clone(),
            storage.clone(),
            Sensitivity::Ordinary,
            GrantDecision::Denied,
        )
        .unwrap();
    let after_storage_revoke = registry
        .negotiate(&principal, ExecutionProfile::Legacy, &BTreeSet::new())
        .unwrap();
    assert!(!after_storage_revoke.exposes(&storage));
    assert!(matches!(
        registry.dispatch(
            &context,
            &plan,
            br#"{"type":"storage.get","key":"x"}"#,
            0,
        ),
        Err(BridgeError::CapabilityDenied { domain }) if domain == storage
    ));
}

#[test]
fn unknown_message_type_is_ignored_and_session_stays_healthy() {
    let (registry, principal, _, _) = fixture(5);
    let session = SessionId(1);
    let context = SessionContext {
        id: session,
        principal,
        profile: ExecutionProfile::Legacy,
    };
    registry.open_session(&context, 0).unwrap();
    let plan = registry
        .negotiate(
            &context.principal,
            ExecutionProfile::Legacy,
            &BTreeSet::new(),
        )
        .unwrap();
    assert!(matches!(
        registry
            .dispatch(
                &context,
                &plan,
                br#"{"type":"storage.future","payload":{}}"#,
                0,
            )
            .unwrap(),
        DispatchOutcome::IgnoredUnknown
    ));
    assert!(matches!(
        registry
            .dispatch(
                &context,
                &plan,
                br#"{"type":"storage.get","payload":{"key":"x"}}"#,
                0,
            )
            .unwrap(),
        DispatchOutcome::Handled(_)
    ));
}

#[test]
fn pinned_flat_provider_fields_are_preserved_without_trusting_payload_identity() {
    let (registry, principal, _, _) = fixture(5);
    let context = SessionContext {
        id: SessionId(1),
        principal,
        profile: ExecutionProfile::Legacy,
    };
    registry.open_session(&context, 0).unwrap();
    let plan = registry
        .negotiate(
            &context.principal,
            ExecutionProfile::Legacy,
            &BTreeSet::new(),
        )
        .unwrap();

    let call = match registry
        .dispatch(
            &context,
            &plan,
            br#"{"type":"storage.get","id":"request-1","key":"x","principal":"forged","session":999}"#,
            0,
        )
        .unwrap()
    {
        DispatchOutcome::Handled(call) => call,
        DispatchOutcome::IgnoredUnknown => panic!("known pinned message was ignored"),
    };

    assert_eq!(
        call.response.as_ref().unwrap().decode().unwrap(),
        serde_json::json!({
            "key": "x",
            "principal": "forged",
            "session": 999
        })
    );
}

#[test]
fn renderer_profile_cannot_escalate_to_outbox() {
    let (mut registry, principal, grants, _) = fixture(5);
    let outbox = Capability::new("outbox").unwrap();
    registry
        .register(Arc::new(EchoProvider {
            descriptor: ProviderDescriptor {
                domain: outbox.clone(),
                protocol_versions: BTreeSet::from([Arc::from("1")]),
                actions: BTreeSet::from([Arc::from("publish")]),
                sensitive: true,
                dependencies: BTreeSet::new(),
                platform_availability: ProviderPlatformAvailability::Available,
            },
            calls: Arc::new(AtomicUsize::new(0)),
        }))
        .unwrap();
    grants
        .set(
            principal.clone(),
            outbox.clone(),
            Sensitivity::Sensitive,
            GrantDecision::AllowExactBuild,
        )
        .unwrap();
    let plan = registry
        .negotiate(&principal, ExecutionProfile::Renderer, &BTreeSet::new())
        .unwrap();
    assert!(!plan.exposes(&outbox));
}

#[test]
fn one_flooded_session_does_not_starve_another() {
    let (registry, principal, _, _) = fixture(1);
    let first = SessionContext {
        id: SessionId(1),
        principal: principal.clone(),
        profile: ExecutionProfile::Legacy,
    };
    let second = SessionContext {
        id: SessionId(2),
        principal: principal.clone(),
        profile: ExecutionProfile::Legacy,
    };
    registry.open_session(&first, 0).unwrap();
    registry.open_session(&second, 0).unwrap();
    let plan = registry
        .negotiate(&principal, ExecutionProfile::Legacy, &BTreeSet::new())
        .unwrap();
    let message = br#"{"type":"storage.get","payload":{}}"#;
    registry.dispatch(&first, &plan, message, 0).unwrap();
    assert!(matches!(
        registry.dispatch(&first, &plan, message, 0),
        Err(BridgeError::MessageRateExceeded { .. })
    ));
    assert!(registry.dispatch(&second, &plan, message, 0).is_ok());
}

#[test]
fn ask_every_time_is_absent_and_a_stale_plan_cannot_dispatch_it() {
    let (registry, principal, grants, domain) = fixture(5);
    let context = SessionContext {
        id: SessionId(1),
        principal: principal.clone(),
        profile: ExecutionProfile::Legacy,
    };
    registry.open_session(&context, 0).unwrap();
    let plan = registry
        .negotiate(&principal, ExecutionProfile::Legacy, &BTreeSet::new())
        .unwrap();

    grants
        .set(
            principal,
            domain.clone(),
            Sensitivity::Ordinary,
            GrantDecision::AskEveryTime,
        )
        .unwrap();

    assert!(matches!(
        registry.dispatch(
            &context,
            &plan,
            br#"{"type":"storage.get","payload":{}}"#,
            0,
        ),
        Err(BridgeError::GrantDecisionRequired { domain: refused }) if refused == domain
    ));
    assert_eq!(registry.resources.census().admitted, 0);
    assert_eq!(registry.census().dispatched, 0);

    let fresh = registry
        .negotiate(
            &context.principal,
            ExecutionProfile::Legacy,
            &BTreeSet::new(),
        )
        .unwrap();
    assert!(!fresh.exposes(&domain));
}

#[test]
fn plan_is_bound_to_the_exact_build_principal() {
    let (registry, principal, _, _) = fixture(5);
    let context = SessionContext {
        id: SessionId(1),
        principal: principal.clone(),
        profile: ExecutionProfile::Legacy,
    };
    registry.open_session(&context, 0).unwrap();
    let plan = registry
        .negotiate(&principal, ExecutionProfile::Legacy, &BTreeSet::new())
        .unwrap();
    let different_build = SessionContext {
        id: SessionId(2),
        principal: Principal::new("a".repeat(64), "app", "c".repeat(64)).unwrap(),
        profile: ExecutionProfile::Legacy,
    };
    registry.open_session(&different_build, 0).unwrap();

    assert!(matches!(
        registry.dispatch(
            &different_build,
            &plan,
            br#"{"type":"storage.get","payload":{}}"#,
            0,
        ),
        Err(BridgeError::PlanPrincipalMismatch)
    ));
}
