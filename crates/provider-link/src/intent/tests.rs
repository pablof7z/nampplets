use nmp_native_nap_bridge::{
    ActivitySink, BridgeLimits, DispatchOutcome, ProviderActivity, ProviderPushObserver,
    ProviderRegistry, SessionContext, SourceWindowId,
};
use nmp_native_runtime_core::{
    ExecutionProfile, GrantDecision, GrantLedger, GrantLimits, ResourceLimits, ResourceTracker,
    Sensitivity,
};
use serde_json::json;

use super::*;

#[derive(Debug, Default)]
struct FakeDispatcher {
    requests: Mutex<Vec<NativeIntentDispatch>>,
    cancelled: Mutex<Vec<Arc<str>>>,
}

impl NativeIntentDispatcher for FakeDispatcher {
    fn try_dispatch(
        &self,
        request: NativeIntentDispatch,
    ) -> Result<Arc<str>, NativeIntentStartError> {
        let handle: Arc<str> = Arc::from(format!("intent-{}", request.token.0));
        self.requests.lock().push(request);
        Ok(handle)
    }

    fn cancel(&self, native_handle: &str) {
        self.cancelled.lock().push(Arc::from(native_handle));
    }
}

#[derive(Debug)]
struct FixedChoice(Arc<str>);

impl IntentChooser for FixedChoice {
    fn try_choose(&self, _request: IntentChoiceRequest) -> Result<IntentChoice, IntentChoiceError> {
        Ok(IntentChoice::Selected(Arc::clone(&self.0)))
    }
}

#[derive(Debug)]
struct NoBridgeActivity;

impl ActivitySink for NoBridgeActivity {
    fn record(&self, _fact: ProviderActivity) {}
}

struct Rig {
    provider: Arc<IntentProvider>,
    dispatcher: Arc<FakeDispatcher>,
    registry: ProviderRegistry,
    context: SessionContext,
    plan: nmp_native_nap_bridge::InjectionPlan,
    observer: ProviderPushObserver,
}

impl Rig {
    fn new(chooser: Arc<dyn IntentChooser>) -> Self {
        let dispatcher = Arc::new(FakeDispatcher::default());
        let provider = Arc::new(
            IntentProvider::new(
                Arc::new(ConfirmEveryIntent),
                chooser,
                dispatcher.clone(),
                Arc::new(NoopIntentActivity),
                IntentProviderLimits::default(),
            )
            .unwrap(),
        );
        let resources = Arc::new(ResourceTracker::new(ResourceLimits::default()).unwrap());
        let grants = Arc::new(GrantLedger::new(GrantLimits::default(), resources.clone()).unwrap());
        let mut registry = ProviderRegistry::new(
            BridgeLimits::default(),
            resources,
            grants.clone(),
            Arc::new(NoBridgeActivity),
        )
        .unwrap();
        registry.register(provider.clone()).unwrap();
        let context = SessionContext {
            id: SessionId(17),
            principal: principal("caller", 'b'),
            profile: ExecutionProfile::Legacy,
        };
        let capability = Capability::new(INTENT_DOMAIN).unwrap();
        grants
            .set(
                context.principal.clone(),
                capability.clone(),
                Sensitivity::Sensitive,
                GrantDecision::AllowExactBuild,
            )
            .unwrap();
        let plan = registry
            .negotiate(
                &context.principal,
                context.profile,
                &BTreeSet::from([capability]),
            )
            .unwrap();
        let observer = registry
            .open_session_bound(&context, &plan, SourceWindowId(117), 0)
            .unwrap();
        registry.mark_session_ready(context.id).unwrap();
        Self {
            provider,
            dispatcher,
            registry,
            context,
            plan,
            observer,
        }
    }

    fn dispatch(&self, envelope: Value) -> Result<Option<Value>, String> {
        match self
            .registry
            .dispatch(
                &self.context,
                &self.plan,
                &serde_json::to_vec(&envelope).unwrap(),
                1,
            )
            .map_err(|error| error.to_string())?
        {
            DispatchOutcome::Handled(call) => Ok(call
                .response
                .map(|response| response.decode().expect("bounded JSON"))),
            DispatchOutcome::IgnoredUnknown => Err("unexpected unknown action".to_owned()),
        }
    }
}

fn principal(d_tag: &str, hash: char) -> Principal {
    Principal::new("a".repeat(64), d_tag, hash.to_string().repeat(64)).unwrap()
}

fn note_declaration() -> IntentHandlerDeclaration {
    IntentHandlerDeclaration {
        archetype: Arc::from("note"),
        title: Some(Arc::from("Note Viewer")),
        actions: BTreeSet::from([Arc::from("open")]),
        conventions: BTreeSet::from([Arc::from("napplet:note/open")]),
    }
}

#[test]
fn default_handler_receives_exact_validated_dispatch_and_result_is_pushed() {
    let rig = Rig::new(Arc::new(CancelIntentChoice));
    let handler = principal("note-viewer", 'c');
    rig.provider
        .register_handler(handler.clone(), vec![note_declaration()])
        .unwrap();
    rig.provider
        .set_default("note", Some(handler.clone()))
        .unwrap();
    let _ = rig.observer.drain(16).unwrap();

    let availability = rig
        .dispatch(json!({
            "type":"intent.available",
            "id":"available-1",
            "archetype":"note"
        }))
        .unwrap()
        .unwrap();
    assert_eq!(availability["availability"]["available"], true);
    assert_eq!(
        availability["availability"]["candidates"][0]["dTag"],
        "note-viewer"
    );
    assert_eq!(
        rig.dispatch(json!({
            "type":"intent.invoke",
            "id":"invoke-1",
            "request":{
                "archetype":"note",
                "action":"open",
                "convention":"napplet:note/open",
                "payload":{"target":"abc"},
                "behavior":{"focus":true}
            }
        }))
        .unwrap(),
        None
    );
    let requests = rig.dispatcher.requests.lock();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].caller, rig.context.principal);
    assert_eq!(requests[0].handler, handler);
    assert!(requests[0].confirmation_required);
    assert_eq!(
        requests[0].payload.decode().unwrap(),
        json!({"target":"abc"})
    );
    let token = requests[0].token;
    drop(requests);

    rig.provider
        .complete(
            token,
            NativeIntentOutcome::Handled {
                window_id: Some(Arc::from("window-1")),
            },
        )
        .unwrap();
    let pushed = rig.observer.drain(8).unwrap().pushes;
    assert_eq!(pushed.len(), 1);
    let result = pushed[0].envelope.decode().unwrap();
    assert_eq!(result["type"], "intent.invoke.result");
    assert_eq!(result["id"], "invoke-1");
    assert_eq!(result["result"]["handler"], "note-viewer");
    assert_eq!(result["result"]["windowId"], "window-1");
}

#[test]
fn protocol_field_is_accepted_as_a_convention_alias() {
    // Real published napplets are built against the `@napplet/nap` SDK,
    // whose `intent.open(archetype, payload, { protocol })` sugar sends
    // the wire field as `protocol`, not the vendored spec's `convention`.
    let rig = Rig::new(Arc::new(CancelIntentChoice));
    let handler = principal("note-viewer", 'c');
    rig.provider
        .register_handler(handler.clone(), vec![note_declaration()])
        .unwrap();
    rig.provider
        .set_default("note", Some(handler.clone()))
        .unwrap();
    let _ = rig.observer.drain(16).unwrap();

    assert_eq!(
        rig.dispatch(json!({
            "type":"intent.invoke",
            "id":"invoke-protocol-1",
            "request":{
                "archetype":"note",
                "action":"open",
                "protocol":"napplet:note/open",
                "payload":{"target":"abc"}
            }
        }))
        .unwrap(),
        None
    );
    let requests = rig.dispatcher.requests.lock();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].handler, handler);
    drop(requests);
}

#[test]
fn undeclared_choice_and_specific_target_execute_nothing() {
    let rig = Rig::new(Arc::new(FixedChoice(Arc::from("spoofed"))));
    rig.provider
        .register_handler(principal("note-viewer", 'c'), vec![note_declaration()])
        .unwrap();
    let _ = rig.observer.drain(8).unwrap();
    let choose = rig
        .dispatch(json!({
            "type":"intent.invoke",
            "id":"choose-1",
            "request":{"archetype":"note","handler":"choose"}
        }))
        .unwrap()
        .unwrap();
    assert_eq!(choose["result"]["handled"], false);
    assert_eq!(choose["result"]["error"], "no handler");

    let specific = rig
        .dispatch(json!({
            "type":"intent.invoke",
            "id":"specific-1",
            "request":{"archetype":"note","handler":"note-viewer"}
        }))
        .unwrap()
        .unwrap();
    assert_eq!(specific["result"]["error"], "invoke denied");
    assert!(rig.dispatcher.requests.lock().is_empty());
}

#[test]
fn exact_build_dtag_collision_is_rejected() {
    let rig = Rig::new(Arc::new(CancelIntentChoice));
    rig.provider
        .register_handler(principal("note-viewer", 'c'), vec![note_declaration()])
        .unwrap();
    assert_eq!(
        rig.provider
            .register_handler(principal("note-viewer", 'd'), vec![note_declaration()]),
        Err(IntentCatalogError::DTagCollision)
    );
}

#[test]
fn teardown_cancels_pending_dispatch_and_late_completion_is_refused() {
    let rig = Rig::new(Arc::new(CancelIntentChoice));
    let handler = principal("note-viewer", 'c');
    rig.provider
        .register_handler(handler.clone(), vec![note_declaration()])
        .unwrap();
    rig.provider.set_default("note", Some(handler)).unwrap();
    let _ = rig.observer.drain(8).unwrap();
    rig.dispatch(json!({
        "type":"intent.invoke",
        "id":"invoke-1",
        "request":{"archetype":"note"}
    }))
    .unwrap();
    let requests = rig.dispatcher.requests.lock();
    let token = requests[0].token;
    let cancellation = requests[0].cancellation.clone();
    drop(requests);
    rig.registry.close_session(rig.context.id);
    assert!(cancellation.is_cancelled());
    assert_eq!(
        rig.dispatcher.cancelled.lock().as_slice(),
        &[Arc::from("intent-1")]
    );
    assert_eq!(
        rig.provider.complete(
            token,
            NativeIntentOutcome::Failed {
                reason: NativeIntentFailureReason::HandlerLaunchRefused,
            },
        ),
        Err(IntentCompletionError::UnknownOperation)
    );
}

#[test]
fn malformed_convention_and_behavior_are_rejected_before_dispatch() {
    let rig = Rig::new(Arc::new(CancelIntentChoice));
    for request in [
        json!({"archetype":"note","convention":"https://example.com"}),
        json!({"archetype":"Note"}),
        json!({"archetype":"note","behavior":{"focus":"yes"}}),
        json!({"archetype":"note","unknown":true}),
    ] {
        assert!(
            rig.dispatch(json!({
                "type":"intent.invoke",
                "id":"bad",
                "request":request
            }))
            .is_err()
        );
    }
    assert!(rig.dispatcher.requests.lock().is_empty());
}
