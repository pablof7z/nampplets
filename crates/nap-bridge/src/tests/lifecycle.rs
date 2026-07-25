use super::*;

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
