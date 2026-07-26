//! Wires `nmp_native_provider_link::IntentProvider`'s `NativeIntentDispatcher`
//! seam to this crate's own `RuntimeApp`/`IncProvider` handles.
//!
//! `IntentProvider` is constructed before `RuntimeApp::open` returns (it must
//! be in the provider list `RuntimeApp::open` consumes), but this dispatcher
//! needs a live `Arc<RuntimeApp>`/`Arc<IntentProvider>` that only exist
//! *after* that call returns. `app`/`intent_provider` are populated via
//! `OnceLock` immediately after construction succeeds in
//! `open_runtime_controller`; every other use only ever reads them.

use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Arc, OnceLock, Weak,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use nmp_native_artifact::VerifiedArtifactHandle;
use nmp_native_provider_inc::{IncNativePushError, IncProvider};
use nmp_native_provider_link::{
    IntentPolicy, IntentPolicyDecision, IntentPolicyRequest, IntentProvider, NativeIntentDispatch,
    NativeIntentDispatcher, NativeIntentFailureReason, NativeIntentOutcome, NativeIntentStartError,
};
use nmp_native_runtime_app::{PlatformCommand, RuntimeApp};
use nmp_native_runtime_core::{ExecutionProfile, Principal, SessionState};
use parking_lot::Mutex;

use crate::{VerifiedArtifact, controller::support::installation_capability_requests};

/// Native signal that a handler's window should be created (if not already
/// running) and brought to front. Distinct from the `inc` native-action
/// pipeline, which is scoped to an already-live session/window and refuses
/// otherwise -- this fires *before* any session may exist yet.
pub trait IntentActivationSink: Send + Sync + fmt::Debug {
    fn focus_or_launch(&self, handler: Principal);
}

/// Identifies the NAP-INTENT handler a launched/focused window should target.
/// `Principal` (manifest author + d tag + aggregate hash) already *is* an
/// exact-build identity, so this maps 1:1 onto a native workspace window
/// identity with no further resolution.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NativeIntentActivationRequest {
    pub manifest_author: String,
    pub d_tag: String,
    pub aggregate_hash: String,
}

/// Native signal fired before any webview session may exist yet: "create (if
/// needed) and bring to front the window for this handler." Distinct from
/// `NativeIncActionExecutor`, which is scoped to an already-live session and
/// refuses otherwise.
#[uniffi::export(callback_interface)]
pub trait NativeIntentActivationExecutor: Send + Sync {
    fn focus_or_launch(&self, handler: NativeIntentActivationRequest);
}

pub(crate) struct CallbackIntentActivation {
    pub(crate) callback: Arc<dyn NativeIntentActivationExecutor>,
}

impl fmt::Debug for CallbackIntentActivation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CallbackIntentActivation")
            .finish_non_exhaustive()
    }
}

impl IntentActivationSink for CallbackIntentActivation {
    fn focus_or_launch(&self, handler: Principal) {
        self.callback
            .focus_or_launch(NativeIntentActivationRequest {
                manifest_author: handler.manifest_author().to_owned(),
                d_tag: handler.d_tag().to_owned(),
                aggregate_hash: handler.aggregate_hash().to_owned(),
            });
    }
}

/// Every capability this MVP grants without a native confirmation prompt or
/// a chooser UI: default-handler dispatch only, gated purely by the
/// `"intent"` NAP grant the caller already holds (enforced upstream by
/// `Bridge::negotiate`/the provider's own session admission, not by this
/// policy). See `Plans/giggly-finding-eich.md` for the accepted MVP scope.
#[derive(Debug, Default)]
pub struct DefaultOnlyIntentPolicy;

impl IntentPolicy for DefaultOnlyIntentPolicy {
    fn evaluate(&self, _request: &IntentPolicyRequest) -> IntentPolicyDecision {
        IntentPolicyDecision {
            allow: true,
            allow_specific_handler: false,
            confirmation_required: false,
            reveal_candidates: true,
        }
    }

    fn allow_discovery(&self, _caller: &Principal, _archetype: &str) -> bool {
        true
    }
}

const MAXIMUM_READY_ATTEMPTS: u32 = 40;
const READY_POLL_INTERVAL: Duration = Duration::from_millis(250);

pub struct RuntimeIntentDispatcher {
    app: OnceLock<Weak<RuntimeApp>>,
    intent_provider: OnceLock<Weak<IntentProvider>>,
    inc_provider: Arc<IncProvider>,
    artifacts: Arc<Mutex<BTreeMap<Principal, Arc<VerifiedArtifactHandle>>>>,
    activation: Mutex<Option<Arc<dyn IntentActivationSink>>>,
    next_handle: AtomicU64,
}

impl fmt::Debug for RuntimeIntentDispatcher {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeIntentDispatcher")
            .finish_non_exhaustive()
    }
}

impl RuntimeIntentDispatcher {
    pub fn new(
        inc_provider: Arc<IncProvider>,
        artifacts: Arc<Mutex<BTreeMap<Principal, Arc<VerifiedArtifactHandle>>>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            app: OnceLock::new(),
            intent_provider: OnceLock::new(),
            inc_provider,
            artifacts,
            activation: Mutex::new(None),
            next_handle: AtomicU64::new(0),
        })
    }

    pub fn bind(&self, app: &Arc<RuntimeApp>, intent_provider: &Arc<IntentProvider>) {
        let _ = self.app.set(Arc::downgrade(app));
        let _ = self.intent_provider.set(Arc::downgrade(intent_provider));
    }

    pub fn set_activation(&self, activation: Option<Arc<dyn IntentActivationSink>>) {
        *self.activation.lock() = activation;
    }
}

impl NativeIntentDispatcher for RuntimeIntentDispatcher {
    fn try_dispatch(
        &self,
        request: NativeIntentDispatch,
    ) -> Result<Arc<str>, NativeIntentStartError> {
        let Some(app) = self.app.get().and_then(Weak::upgrade) else {
            return Err(NativeIntentStartError::Unavailable);
        };
        let Some(intent_provider) = self.intent_provider.get().and_then(Weak::upgrade) else {
            return Err(NativeIntentStartError::Unavailable);
        };
        let Some(convention) = request.convention.clone() else {
            return Err(NativeIntentStartError::Unavailable);
        };
        let handle: Arc<str> = Arc::from(format!(
            "intent-{}",
            self.next_handle.fetch_add(1, Ordering::Relaxed)
        ));
        let inc_provider = Arc::clone(&self.inc_provider);
        let artifacts = Arc::clone(&self.artifacts);
        let activation = self.activation.lock().clone();
        let spawned = thread::Builder::new()
            .name("intent-dispatch".to_owned())
            .spawn(move || {
                let outcome = run_dispatch(
                    &app,
                    &inc_provider,
                    &artifacts,
                    activation.as_deref(),
                    &request,
                    &convention,
                );
                let _ = intent_provider.complete(request.token, outcome);
            });
        spawned
            .map(|_| handle)
            .map_err(|_| NativeIntentStartError::Unavailable)
    }

    fn cancel(&self, _native_handle: &str) {
        // No-op: `NativeIntentDispatch.cancellation` is the same shared
        // `Cancellation` handle `IntentProvider::remove_session` already
        // cancels on session teardown; `run_dispatch`'s poll loop observes
        // that directly via `wait_for`, so there is nothing extra to signal
        // here keyed only by the opaque handle string.
    }
}

fn run_dispatch(
    app: &Arc<RuntimeApp>,
    inc_provider: &Arc<IncProvider>,
    artifacts: &Mutex<BTreeMap<Principal, Arc<VerifiedArtifactHandle>>>,
    activation: Option<&dyn IntentActivationSink>,
    request: &NativeIntentDispatch,
    convention: &str,
) -> NativeIntentOutcome {
    let mut launched = false;
    // The retry loop can observe `NotSubscribed` many times before either
    // succeeding or exhausting its budget. Remember that it happened at
    // least once so the eventual `Failed` names the actual last-observed
    // cause instead of collapsing "handler launched but its own JS never
    // subscribed" into the same opaque outcome as "handler never came up
    // at all".
    let mut observed_unsubscribed = false;
    for _ in 0..MAXIMUM_READY_ATTEMPTS {
        if request.cancellation.is_cancelled() {
            return NativeIntentOutcome::Cancelled;
        }
        let snapshot = app.snapshot();
        let running = snapshot.sessions.iter().find(|session| {
            session.principal == request.handler
                && matches!(
                    session.state,
                    SessionState::Launching | SessionState::Running | SessionState::Suspended
                )
        });
        match running {
            Some(session) => {
                match inc_provider.native_push(
                    session.id,
                    convention,
                    request.caller.d_tag(),
                    &request.payload,
                ) {
                    Ok(()) => return NativeIntentOutcome::Handled { window_id: None },
                    Err(IncNativePushError::NotSubscribed) => {
                        // The session exists but its own JS hasn't reached
                        // `inc.subscribe(convention, ...)` yet -- keep polling.
                        observed_unsubscribed = true;
                    }
                    Err(IncNativePushError::UnknownSession) => {
                        return NativeIntentOutcome::Failed {
                            reason: NativeIntentFailureReason::HandlerSessionEnded,
                        };
                    }
                    Err(IncNativePushError::Push(error)) => {
                        return NativeIntentOutcome::Failed {
                            reason: NativeIntentFailureReason::PushRefused {
                                detail: error.to_string(),
                            },
                        };
                    }
                }
            }
            None if !launched => {
                launched = true;
                if !launch_handler(app, artifacts, request) {
                    return NativeIntentOutcome::Failed {
                        reason: NativeIntentFailureReason::HandlerLaunchRefused,
                    };
                }
                if let Some(activation) = activation {
                    activation.focus_or_launch(request.handler.clone());
                }
            }
            None => {}
        }
        // `wait_for` returns `Ok(())` the moment cancellation is observed
        // (including if it already happened) and `Err(Cancelled)` once the
        // interval elapses with no cancellation -- i.e. the normal "keep
        // polling" case.
        if request.cancellation.wait_for(READY_POLL_INTERVAL).is_ok() {
            return NativeIntentOutcome::Cancelled;
        }
    }
    // The poll budget is exhausted. If a session was ever observed running
    // but never subscribed, that is the reportable cause; otherwise the
    // handler's session never reached a running state at all within the
    // budget (stuck launch, or the launched session immediately ended).
    NativeIntentOutcome::Failed {
        reason: if observed_unsubscribed {
            NativeIntentFailureReason::HandlerNeverSubscribed
        } else {
            NativeIntentFailureReason::HandlerNeverObservedRunning
        },
    }
}

fn launch_handler(
    app: &Arc<RuntimeApp>,
    artifacts: &Mutex<BTreeMap<Principal, Arc<VerifiedArtifactHandle>>>,
    request: &NativeIntentDispatch,
) -> bool {
    let Some(handle) = artifacts.lock().get(&request.handler).cloned() else {
        return false;
    };
    // Deliberately the *same* derivation an interactive install/launch uses,
    // not `manifest().requirements()` alone. A build whose domains are
    // declared by the `napplet-requires` meta in its verified `/index.html`
    // rather than by signed tags -- `nip29-groups` is exactly that -- would
    // otherwise be launched here with an empty required set and come up
    // without the relay/config/intent domains its own content needs.
    let artifact = VerifiedArtifact {
        handle,
        principal: Some(request.handler.clone()),
    };
    let Ok(capability_requests) = installation_capability_requests(&artifact) else {
        return false;
    };
    let required_domains = capability_requests
        .into_iter()
        .filter(|request| {
            request.requirement == nmp_native_runtime_core::CapabilityRequirement::Required
        })
        .map(|request| request.capability)
        .collect();
    app.dispatch(PlatformCommand::Launch {
        principal: request.handler.clone(),
        profile: ExecutionProfile::Legacy,
        required_domains,
    });
    true
}
