use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use nmp_native_nap_bridge::{
    Provider, ProviderCall, ProviderDescriptor, ProviderError, ProviderPlatformAvailability,
    ProviderPushSender, ProviderRequest, ProviderSession, ProviderSessionContext,
    ProviderSessionEnd,
};
use nmp_native_runtime_core::{BoundedJson, Capability, Principal, SessionId, WorkLease};
use parking_lot::Mutex;
use serde_json::{Map, Value};

use crate::PINNED_NAP_PROTOCOL;

pub const INTENT_DOMAIN: &str = "intent";

mod types;
mod validate;
mod wire;
pub use types::*;
use validate::{valid_declaration, validate_limits};
use wire::{invalid, lifecycle_error};

#[derive(Debug)]
pub struct IntentProvider {
    policy: Arc<dyn IntentPolicy>,
    chooser: Arc<dyn IntentChooser>,
    dispatcher: Arc<dyn NativeIntentDispatcher>,
    activity: Arc<dyn IntentActivitySink>,
    limits: IntentProviderLimits,
    descriptor: ProviderDescriptor,
    state: Mutex<IntentState>,
}

#[derive(Debug, Default)]
struct IntentState {
    sessions: BTreeMap<SessionId, IntentSession>,
    handlers: BTreeMap<Arc<str>, BTreeMap<Principal, RegisteredHandler>>,
    defaults: BTreeMap<Arc<str>, Principal>,
    pending: BTreeMap<IntentOperationToken, PendingIntent>,
    next_token: u64,
}

#[derive(Clone, Debug)]
struct IntentSession {
    principal: Principal,
    outbound: ProviderPushSender,
    ready: bool,
}

#[derive(Debug)]
struct PendingIntent {
    caller: Principal,
    session: SessionId,
    correlation_id: Arc<str>,
    archetype: Arc<str>,
    action: Arc<str>,
    convention: Option<Arc<str>>,
    handler: Principal,
    native_handle: Option<Arc<str>>,
    work: WorkLease,
}

struct ValidatedInvocation {
    archetype: Arc<str>,
    action: Arc<str>,
    convention: Option<Arc<str>>,
    payload: BoundedJson,
    handler_request: IntentHandlerRequest,
    behavior: IntentBehavior,
}

impl IntentProvider {
    pub fn new(
        policy: Arc<dyn IntentPolicy>,
        chooser: Arc<dyn IntentChooser>,
        dispatcher: Arc<dyn NativeIntentDispatcher>,
        activity: Arc<dyn IntentActivitySink>,
        limits: IntentProviderLimits,
    ) -> Result<Self, IntentProviderBuildError> {
        validate_limits(limits)?;
        Ok(Self {
            policy,
            chooser,
            dispatcher,
            activity,
            limits,
            descriptor: ProviderDescriptor {
                domain: Capability::new(INTENT_DOMAIN).expect("static intent capability is valid"),
                protocol_versions: BTreeSet::from([Arc::from(PINNED_NAP_PROTOCOL)]),
                actions: ["invoke", "available", "handlers"]
                    .into_iter()
                    .map(Arc::from)
                    .collect(),
                sensitive: true,
                dependencies: BTreeSet::new(),
                platform_availability: ProviderPlatformAvailability::Available,
            },
            state: Mutex::new(IntentState::default()),
        })
    }

    /// Trusted catalog mutation from a verified installed manifest.
    pub fn register_handler(
        &self,
        principal: Principal,
        declarations: Vec<IntentHandlerDeclaration>,
    ) -> Result<(), IntentCatalogError> {
        if declarations.is_empty()
            || declarations
                .iter()
                .any(|declaration| !valid_declaration(declaration, self.limits))
        {
            return Err(IntentCatalogError::InvalidDeclaration);
        }
        let mut affected = declarations
            .iter()
            .map(|declaration| Arc::clone(&declaration.archetype))
            .collect::<BTreeSet<_>>();
        {
            let mut state = self.state.lock();
            let owners = state
                .handlers
                .values()
                .flat_map(BTreeMap::values)
                .filter(|handler| handler.principal.d_tag() == principal.d_tag())
                .map(|handler| &handler.principal)
                .collect::<BTreeSet<_>>();
            if owners.iter().any(|owner| *owner != &principal) {
                return Err(IntentCatalogError::DTagCollision);
            }
            let mut prospective = state.handlers.clone();
            for (archetype, handlers) in &mut prospective {
                if handlers.remove(&principal).is_some() {
                    affected.insert(Arc::clone(archetype));
                }
            }
            prospective.retain(|_, handlers| !handlers.is_empty());
            let distinct_handlers = prospective
                .values()
                .flat_map(BTreeMap::keys)
                .collect::<BTreeSet<_>>();
            if distinct_handlers.len() >= self.limits.maximum_handlers {
                return Err(IntentCatalogError::Capacity);
            }
            for declaration in declarations {
                if !prospective.contains_key(&declaration.archetype)
                    && prospective.len() >= self.limits.maximum_archetypes
                {
                    return Err(IntentCatalogError::Capacity);
                }
                let handlers = prospective
                    .entry(Arc::clone(&declaration.archetype))
                    .or_default();
                if !handlers.contains_key(&principal)
                    && handlers.len() >= self.limits.maximum_candidates_per_archetype
                {
                    return Err(IntentCatalogError::Capacity);
                }
                handlers.insert(
                    principal.clone(),
                    RegisteredHandler {
                        principal: principal.clone(),
                        declaration,
                    },
                );
            }
            state.handlers = prospective;
            let defaults = state
                .defaults
                .iter()
                .filter(|(archetype, default)| {
                    state
                        .handlers
                        .get(*archetype)
                        .is_some_and(|candidates| candidates.contains_key(*default))
                })
                .map(|(archetype, default)| (Arc::clone(archetype), default.clone()))
                .collect();
            state.defaults = defaults;
        }
        for archetype in affected {
            self.publish_changed(&archetype);
        }
        Ok(())
    }

    pub fn unregister_handler(&self, principal: &Principal) {
        let affected = {
            let mut state = self.state.lock();
            let affected = state
                .handlers
                .iter_mut()
                .filter_map(|(archetype, handlers)| {
                    handlers.remove(principal).map(|_| Arc::clone(archetype))
                })
                .collect::<Vec<_>>();
            state.handlers.retain(|_, handlers| !handlers.is_empty());
            state.defaults.retain(|_, default| default != principal);
            affected
        };
        for archetype in affected {
            self.publish_changed(&archetype);
        }
    }

    /// Trusted, user-driven preference mutation. No NAP wire action reaches it.
    pub fn set_default(
        &self,
        archetype: &str,
        principal: Option<Principal>,
    ) -> Result<(), IntentCatalogError> {
        let archetype: Arc<str> = Arc::from(archetype);
        {
            let mut state = self.state.lock();
            match principal {
                Some(principal)
                    if state
                        .handlers
                        .get(&archetype)
                        .is_some_and(|handlers| handlers.contains_key(&principal)) =>
                {
                    state.defaults.insert(Arc::clone(&archetype), principal);
                }
                Some(_) => return Err(IntentCatalogError::UnknownDefault),
                None => {
                    state.defaults.remove(&archetype);
                }
            }
        }
        self.publish_changed(&archetype);
        Ok(())
    }

    pub fn pending_count(&self) -> usize {
        self.state.lock().pending.len()
    }

    pub fn complete(
        &self,
        token: IntentOperationToken,
        outcome: NativeIntentOutcome,
    ) -> Result<(), IntentCompletionError> {
        let (pending, outbound) = {
            let mut state = self.state.lock();
            let pending = state
                .pending
                .remove(&token)
                .ok_or(IntentCompletionError::UnknownOperation)?;
            let outbound = state
                .sessions
                .get(&pending.session)
                .filter(|session| session.ready && session.principal == pending.caller)
                .map(|session| session.outbound.clone());
            (pending, outbound)
        };
        drop(pending.work);
        let (ok, handled, error, window_id, activity_outcome) = match outcome {
            NativeIntentOutcome::Handled { window_id } => {
                (true, true, None, window_id, IntentActivityOutcome::Handled)
            }
            NativeIntentOutcome::Cancelled => (
                false,
                false,
                Some("user cancelled".to_owned()),
                None,
                IntentActivityOutcome::Cancelled,
            ),
            NativeIntentOutcome::Failed { reason } => (
                false,
                false,
                // Previously every failure reported the same fixed
                // "invoke failed" string. A napplet dispatching an intent
                // could not tell "the handler never launched" from "the
                // handler launched but its own JS never subscribed" from
                // "the handler's session ended mid-dispatch" from "the
                // push itself was refused" -- all reachable, distinct
                // causes the retry loop already knows apart.
                Some(match reason {
                    NativeIntentFailureReason::HandlerLaunchRefused => {
                        "handler could not be launched".to_owned()
                    }
                    NativeIntentFailureReason::HandlerNeverSubscribed => {
                        "handler launched but never subscribed to the requested convention"
                            .to_owned()
                    }
                    NativeIntentFailureReason::HandlerNeverObservedRunning => {
                        "handler never reached a running session within the poll budget"
                            .to_owned()
                    }
                    NativeIntentFailureReason::HandlerSessionEnded => {
                        "handler session ended before the intent could be delivered".to_owned()
                    }
                    NativeIntentFailureReason::PushRefused { detail } => {
                        format!("push refused: {detail}")
                    }
                }),
                None,
                IntentActivityOutcome::Refused,
            ),
        };
        self.activity.record(IntentActivity {
            principal: pending.caller.clone(),
            session: pending.session,
            action: Arc::clone(&pending.action),
            outcome: activity_outcome,
        });
        let Some(outbound) = outbound else {
            return Ok(());
        };
        let mut result = Map::from_iter([
            ("ok".to_owned(), Value::Bool(ok)),
            (
                "archetype".to_owned(),
                Value::String(pending.archetype.to_string()),
            ),
            (
                "action".to_owned(),
                Value::String(pending.action.to_string()),
            ),
            ("handled".to_owned(), Value::Bool(handled)),
            (
                "handler".to_owned(),
                Value::String(pending.handler.d_tag().to_owned()),
            ),
        ]);
        if let Some(convention) = &pending.convention {
            result.insert(
                "convention".to_owned(),
                Value::String(convention.to_string()),
            );
        }
        if let Some(window_id) = window_id {
            result.insert("windowId".to_owned(), Value::String(window_id.to_string()));
        }
        if let Some(error) = error {
            result.insert("error".to_owned(), Value::String(error));
        }
        outbound
            .push(
                "intent.invoke.result",
                Map::from_iter([
                    (
                        "id".to_owned(),
                        Value::String(pending.correlation_id.to_string()),
                    ),
                    ("result".to_owned(), Value::Object(result)),
                ]),
                None,
            )
            .map(|_| ())
            .map_err(|error| {
                self.activity.record(IntentActivity {
                    principal: pending.caller,
                    session: pending.session,
                    action: pending.action,
                    outcome: IntentActivityOutcome::PushRefused,
                });
                IntentCompletionError::Push(error)
            })
    }
    fn remove_session(&self, context: &ProviderSessionContext) {
        let cancelled = {
            let mut state = self.state.lock();
            if state
                .sessions
                .get(&context.session)
                .is_none_or(|session| session.principal != context.principal)
            {
                return;
            }
            state.sessions.remove(&context.session);
            let tokens = state
                .pending
                .iter()
                .filter_map(|(token, pending)| {
                    (pending.session == context.session).then_some(*token)
                })
                .collect::<Vec<_>>();
            tokens
                .into_iter()
                .filter_map(|token| state.pending.remove(&token))
                .collect::<Vec<_>>()
        };
        for pending in cancelled {
            pending.work.cancellation().cancel();
            if let Some(handle) = pending.native_handle {
                self.dispatcher.cancel(&handle);
            }
            self.activity.record(IntentActivity {
                principal: pending.caller,
                session: pending.session,
                action: pending.action,
                outcome: IntentActivityOutcome::LifecycleCancelled,
            });
        }
    }
}

impl Provider for IntentProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn call(&self, request: ProviderRequest) -> Result<ProviderCall, ProviderError> {
        match request.action.as_ref() {
            "invoke" => self.invoke(request),
            "available" => self.available(request),
            "handlers" => self.handlers(request),
            _ => Err(invalid(&request, "unknown action")),
        }
    }

    fn session_opened(&self, session: ProviderSession) -> Result<(), ProviderError> {
        let mut state = self.state.lock();
        if let Some(existing) = state.sessions.get(&session.context.session) {
            return if existing.principal == session.context.principal
                && existing.outbound.source_window() == session.context.source_window
            {
                Ok(())
            } else {
                Err(lifecycle_error("mapped intent session identity changed"))
            };
        }
        if state.sessions.len() >= self.limits.maximum_sessions {
            return Err(lifecycle_error("intent session capacity is full"));
        }
        state.sessions.insert(
            session.context.session,
            IntentSession {
                principal: session.context.principal,
                outbound: session.outbound,
                ready: false,
            },
        );
        Ok(())
    }

    fn session_ready(&self, context: &ProviderSessionContext) -> Result<(), ProviderError> {
        let mut state = self.state.lock();
        let session = state
            .sessions
            .get_mut(&context.session)
            .ok_or_else(|| lifecycle_error("intent session was not opened"))?;
        if session.principal != context.principal
            || session.outbound.source_window() != context.source_window
        {
            return Err(lifecycle_error("mapped intent session identity changed"));
        }
        session.ready = true;
        Ok(())
    }

    fn session_closed(&self, context: &ProviderSessionContext, _reason: ProviderSessionEnd) {
        self.remove_session(context);
    }

    fn session_revoked(&self, context: &ProviderSessionContext) {
        self.remove_session(context);
    }
}
#[cfg(test)]
mod tests;
