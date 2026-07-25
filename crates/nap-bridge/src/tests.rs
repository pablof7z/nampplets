use std::sync::atomic::{AtomicUsize, Ordering};

use nmp_native_runtime_core::{GrantLimits, ResourceLimits, Sensitivity};

use super::*;

#[derive(Debug)]
struct EchoProvider {
    descriptor: ProviderDescriptor,
    calls: Arc<AtomicUsize>,
}

impl Provider for EchoProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn call(&self, request: ProviderRequest) -> Result<ProviderCall, ProviderError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(ProviderCall::completed(Some(
            BoundedJson::from_value(&request.payload, 1024).unwrap(),
        )))
    }
}

fn fixture(burst: u32) -> (ProviderRegistry, Principal, Arc<GrantLedger>, Capability) {
    let resources = Arc::new(ResourceTracker::new(ResourceLimits::default()).unwrap());
    let grants =
        Arc::new(GrantLedger::new(GrantLimits::default(), Arc::clone(&resources)).unwrap());
    let activity = Arc::new(MemoryActivitySink::bounded(32));
    let mut registry = ProviderRegistry::new(
        BridgeLimits {
            message_burst: burst,
            ..BridgeLimits::default()
        },
        resources,
        Arc::clone(&grants),
        activity,
    )
    .unwrap();
    let domain = Capability::new("storage").unwrap();
    registry
        .register(Arc::new(EchoProvider {
            descriptor: ProviderDescriptor {
                domain: domain.clone(),
                protocol_versions: BTreeSet::from([Arc::from("1")]),
                actions: BTreeSet::from([Arc::from("get")]),
                sensitive: false,
                dependencies: BTreeSet::new(),
                platform_availability: ProviderPlatformAvailability::Available,
            },
            calls: Arc::new(AtomicUsize::new(0)),
        }))
        .unwrap();
    let principal = Principal::new("a".repeat(64), "app", "b".repeat(64)).unwrap();
    grants
        .set(
            principal.clone(),
            domain.clone(),
            Sensitivity::Ordinary,
            GrantDecision::AllowExactBuild,
        )
        .unwrap();
    (registry, principal, grants, domain)
}

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

#[derive(Debug)]
struct StreamingProvider {
    descriptor: ProviderDescriptor,
    cancellation: Arc<Mutex<Option<Cancellation>>>,
}

impl Provider for StreamingProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn call(&self, request: ProviderRequest) -> Result<ProviderCall, ProviderError> {
        *self.cancellation.lock() = Some(request.work.cancellation().clone());
        Ok(ProviderCall::streaming(None, request.work))
    }
}

#[derive(Debug)]
struct ProposalReceiptSink;

impl ReceiptEventSink for ProposalReceiptSink {
    fn push_latest(
        &self,
        _snapshot: nmp_native_runtime_core::ReceiptSnapshot,
    ) -> Result<(), nmp_native_runtime_core::ReceiptSinkError> {
        Ok(())
    }

    fn close(&self, _reason: Option<Arc<str>>) {}
}

#[derive(Debug)]
struct TestWriteCompletion {
    converted: Arc<AtomicUsize>,
    refused: Arc<AtomicUsize>,
}

impl ProviderWriteCompletion for TestWriteCompletion {
    fn into_receipt_sink(self: Box<Self>) -> Arc<dyn ReceiptEventSink> {
        self.converted.fetch_add(1, Ordering::Relaxed);
        Arc::new(ProposalReceiptSink)
    }

    fn refused(self: Box<Self>, _reason: Arc<str>) {
        self.refused.fetch_add(1, Ordering::Relaxed);
    }
}

fn test_approved_write(session: SessionId) -> ApprovedWrite {
    ApprovedWrite {
        approval_id: Arc::from("approval-1"),
        origin_principal: Principal::new("a".repeat(64), "app", "b".repeat(64)).unwrap(),
        origin_session: session,
        account: nmp_native_runtime_core::AccountRef(Arc::from("c".repeat(64))),
        draft: BoundedJson::from_raw("{}", 16).unwrap(),
    }
}

#[test]
fn write_proposal_retains_work_and_transfers_completion_once() {
    let resources = ResourceTracker::new(ResourceLimits::default()).unwrap();
    let session = SessionId(91);
    let converted = Arc::new(AtomicUsize::new(0));
    let refused = Arc::new(AtomicUsize::new(0));
    let work = resources
        .admit(session, None, ResourceClass::ProviderCall)
        .unwrap();
    let mut call = ProviderCall::proposed_write(
        None,
        test_approved_write(session),
        Box::new(TestWriteCompletion {
            converted: Arc::clone(&converted),
            refused: Arc::clone(&refused),
        }),
        work,
    );

    assert!(call.is_active());
    assert_eq!(resources.census().admitted, 1);
    assert_eq!(
        call.write_proposal()
            .unwrap()
            .write
            .as_ref()
            .unwrap()
            .approval_id
            .as_ref(),
        "approval-1"
    );

    let proposal = call.take_write_proposal().unwrap();
    assert!(!call.is_active());
    let (write, completion, work) = proposal.into_parts();
    assert_eq!(write.origin_session, session);
    assert_eq!(resources.census().admitted, 1);
    drop(work);
    assert_eq!(resources.census().admitted, 0);
    let _sink = completion.into_receipt_sink();
    assert_eq!(converted.load(Ordering::Relaxed), 1);
    assert_eq!(refused.load(Ordering::Relaxed), 0);
}

#[test]
fn write_proposal_refusal_is_typed_and_releases_work() {
    let resources = ResourceTracker::new(ResourceLimits::default()).unwrap();
    let session = SessionId(92);
    let converted = Arc::new(AtomicUsize::new(0));
    let refused = Arc::new(AtomicUsize::new(0));
    let work = resources
        .admit(session, None, ResourceClass::ProviderCall)
        .unwrap();
    let mut call = ProviderCall::proposed_write(
        None,
        test_approved_write(session),
        Box::new(TestWriteCompletion {
            converted: Arc::clone(&converted),
            refused: Arc::clone(&refused),
        }),
        work,
    );

    call.take_write_proposal()
        .unwrap()
        .refuse(Arc::from("not approved"));
    assert_eq!(resources.census().admitted, 0);
    assert_eq!(converted.load(Ordering::Relaxed), 0);
    assert_eq!(refused.load(Ordering::Relaxed), 1);
}

#[test]
fn revocation_blocks_stale_plan_and_signals_retained_charged_work() {
    let resources = Arc::new(ResourceTracker::new(ResourceLimits::default()).unwrap());
    let grants =
        Arc::new(GrantLedger::new(GrantLimits::default(), Arc::clone(&resources)).unwrap());
    let activity = Arc::new(MemoryActivitySink::bounded(8));
    let cancellation = Arc::new(Mutex::new(None));
    let domain = Capability::new("resource").unwrap();
    let mut registry = ProviderRegistry::new(
        BridgeLimits::default(),
        Arc::clone(&resources),
        Arc::clone(&grants),
        activity,
    )
    .unwrap();
    registry
        .register(Arc::new(StreamingProvider {
            descriptor: ProviderDescriptor {
                domain: domain.clone(),
                protocol_versions: BTreeSet::from([Arc::from("1")]),
                actions: BTreeSet::from([Arc::from("subscribe")]),
                sensitive: true,
                dependencies: BTreeSet::new(),
                platform_availability: ProviderPlatformAvailability::Available,
            },
            cancellation: Arc::clone(&cancellation),
        }))
        .unwrap();
    let principal = Principal::new("a".repeat(64), "app", "b".repeat(64)).unwrap();
    grants
        .set(
            principal.clone(),
            domain.clone(),
            Sensitivity::Sensitive,
            GrantDecision::AllowExactBuild,
        )
        .unwrap();
    let context = SessionContext {
        id: SessionId(7),
        principal: principal.clone(),
        profile: ExecutionProfile::Legacy,
    };
    registry.open_session(&context, 0).unwrap();
    let plan = registry
        .negotiate(&principal, ExecutionProfile::Legacy, &BTreeSet::new())
        .unwrap();
    let mut call = match registry
        .dispatch(
            &context,
            &plan,
            br#"{"type":"resource.subscribe","payload":{}}"#,
            0,
        )
        .unwrap()
    {
        DispatchOutcome::Handled(call) => call,
        DispatchOutcome::IgnoredUnknown => panic!("registered action must be handled"),
    };

    assert!(call.is_active());
    assert_eq!(resources.census().admitted, 1);
    assert_eq!(registry.revoke(&principal, &domain), 1);
    assert!(cancellation.lock().as_ref().unwrap().is_cancelled());
    assert!(call.operation().unwrap().is_cancelled());
    assert!(matches!(
        registry.dispatch(
            &context,
            &plan,
            br#"{"type":"resource.subscribe","payload":{}}"#,
            0,
        ),
        Err(BridgeError::CapabilityDenied { domain: refused }) if refused == domain
    ));

    call.take_operation().unwrap().cancel();
    assert_eq!(resources.census().admitted, 0);
}

#[test]
fn closing_session_cancels_active_operation_and_blocks_context_rebinding() {
    let (registry, principal, _, _) = fixture(5);
    let original = SessionContext {
        id: SessionId(4),
        principal,
        profile: ExecutionProfile::Legacy,
    };
    registry.open_session(&original, 0).unwrap();
    let rebound = SessionContext {
        id: original.id,
        principal: Principal::new("a".repeat(64), "app", "c".repeat(64)).unwrap(),
        profile: ExecutionProfile::Legacy,
    };
    assert!(matches!(
        registry.open_session(&rebound, 0),
        Err(BridgeError::SessionIdentityMismatch { session }) if session == original.id
    ));
}
