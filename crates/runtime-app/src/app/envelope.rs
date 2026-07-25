//! Mapped-envelope dispatch and the exact NAP-SHELL handshake checks.

use std::{collections::BTreeSet, sync::Arc, thread::JoinHandle};

use nmp_native_nap_bridge::{BridgeError, DispatchOutcome, InjectionPlan};
use nmp_native_runtime_core::{BoundedJson, Capability, SessionId, SessionState};

use super::{ActiveOperation, AppState, RuntimeApp};
use crate::{
    commands::{PlatformEvent, ProviderOperationId},
    views::AppErrorCode,
};

impl RuntimeApp {
    pub(super) fn dispatch_envelope(
        self: &Arc<Self>,
        state: &mut AppState,
        session_id: SessionId,
        bytes: &[u8],
        now: u64,
    ) -> Option<JoinHandle<()>> {
        if bytes.len() > self.limits.maximum_envelope_bytes {
            self.refuse(
                state,
                AppErrorCode::Capacity,
                None,
                Some(session_id),
                "mapped envelope exceeds the application bound",
                now,
            );
            return None;
        }
        let Some(entry) = state.sessions.get(&session_id) else {
            self.refuse(
                state,
                AppErrorCode::UnknownSession,
                None,
                Some(session_id),
                "stale or unknown session",
                now,
            );
            return None;
        };
        if entry.session.state() != SessionState::Running {
            let principal = entry.context.principal.clone();
            self.refuse(
                state,
                AppErrorCode::InvalidLifecycle,
                Some(principal),
                Some(session_id),
                "mapped envelopes are refused while the session is suspended",
                now,
            );
            return None;
        }
        let principal = entry.context.principal.clone();
        let context = entry.context.clone();
        let plan = entry.plan.clone();
        let route = envelope_route(bytes);
        let domain = route.as_ref().map(|(domain, _)| domain.clone());
        let is_shell_ready = route
            .as_ref()
            .is_some_and(|(domain, action)| domain.as_str() == "shell" && action == "ready");
        if route.as_ref().is_some_and(|(domain, action)| {
            domain.as_str() == "shell" && action == "ready" && !exact_shell_ready(bytes)
        }) {
            self.refuse(
                state,
                AppErrorCode::Bridge,
                Some(principal),
                Some(session_id),
                "shell.ready must be exactly the uncorrelated liveness envelope",
                now,
            );
            return None;
        }
        if route.as_ref().is_some_and(|(domain, action)| {
            domain.as_str() != "shell"
                && self
                    .mapped_routes
                    .contains(&(domain.clone(), Arc::from(action.as_str())))
        }) && !self.shell_provider.is_ready(session_id)
        {
            self.refuse(
                state,
                AppErrorCode::Bridge,
                Some(principal),
                Some(session_id),
                "NAP-SHELL handshake has not established this mapped session",
                now,
            );
            return None;
        }
        match self.bridge.dispatch(&context, &plan, bytes, now) {
            Ok(DispatchOutcome::IgnoredUnknown) => {
                self.push_event(
                    state,
                    PlatformEvent::EnvelopeIgnored {
                        session: session_id,
                    },
                );
            }
            Ok(DispatchOutcome::Handled(mut call)) => {
                if domain
                    .as_ref()
                    .is_some_and(|domain| domain.as_str() == "shell")
                    && call.response.is_some()
                    && !shell_init_matches_plan(call.response.as_ref(), &plan)
                {
                    self.refuse(
                        state,
                        AppErrorCode::Bridge,
                        Some(principal),
                        Some(session_id),
                        "shell.init capability set does not match the fixed session plan",
                        now,
                    );
                    return self.end_session(
                        state,
                        session_id,
                        SessionState::Crashed,
                        Some(Arc::from("invalid shell.init")),
                        now,
                    );
                }
                if is_shell_ready
                    && let Err(detail) = self.activate_push_delivery(state, session_id)
                {
                    self.refuse(
                        state,
                        AppErrorCode::Bridge,
                        Some(principal),
                        Some(session_id),
                        detail,
                        now,
                    );
                    return self.end_session(
                        state,
                        session_id,
                        SessionState::Crashed,
                        Some(Arc::from("provider delivery activation failed")),
                        now,
                    );
                }
                let mut handle = call.take_operation();
                let mut proposal = call.take_write_proposal();
                if handle.is_some() && proposal.is_some() {
                    if let Some(proposal) = proposal.take() {
                        proposal.refuse(Arc::from(
                            "provider returned both a streaming operation and a write proposal",
                        ));
                    }
                    if let Some(handle) = handle.take() {
                        handle.cancel();
                    }
                    self.refuse(
                        state,
                        AppErrorCode::Bridge,
                        Some(principal.clone()),
                        Some(session_id),
                        "provider returned conflicting operation ownership",
                        now,
                    );
                    return None;
                }
                let operation = if handle.is_some() || proposal.is_some() {
                    if state.operations.len() >= self.limits.maximum_provider_operations {
                        if let Some(proposal) = proposal.take() {
                            proposal.refuse(Arc::from("provider operation capacity is full"));
                        }
                        if let Some(handle) = handle.take() {
                            handle.cancel();
                        }
                        self.refuse(
                            state,
                            AppErrorCode::Capacity,
                            Some(principal),
                            Some(session_id),
                            "provider operation ownership capacity is full",
                            now,
                        );
                        return None;
                    }
                    let Some(next) = state.next_operation_id.checked_add(1) else {
                        if let Some(proposal) = proposal.take() {
                            proposal.refuse(Arc::from(
                                "provider operation identifier space is exhausted",
                            ));
                        }
                        if let Some(handle) = handle.take() {
                            handle.cancel();
                        }
                        self.refuse(
                            state,
                            AppErrorCode::Capacity,
                            Some(principal),
                            Some(session_id),
                            "provider operation identifier space is exhausted",
                            now,
                        );
                        return None;
                    };
                    let domain = domain.clone().unwrap_or_else(|| {
                        Capability::new("unknown").expect("static capability is valid")
                    });
                    let id = ProviderOperationId(next);
                    state.next_operation_id = next;
                    state.operations.insert(
                        id,
                        ActiveOperation {
                            session: session_id,
                            principal: principal.clone(),
                            domain,
                            handle,
                            proposal,
                        },
                    );
                    Some(id)
                } else {
                    None
                };
                self.push_event(
                    state,
                    PlatformEvent::EnvelopeHandled {
                        session: session_id,
                        operation,
                        response: call.response,
                    },
                );
            }
            Err(BridgeError::SessionIdentityMismatch { .. }) => {
                self.refuse(
                    state,
                    AppErrorCode::SessionIdentityMismatch,
                    Some(principal),
                    Some(session_id),
                    "mapped source no longer matches the fixed session identity",
                    now,
                );
            }
            Err(error) => {
                self.refuse_bridge(state, Some(principal), Some(session_id), error, now);
            }
        }
        None
    }
}

pub(super) fn envelope_route(bytes: &[u8]) -> Option<(Capability, String)> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let message_type = value.get("type")?.as_str()?;
    let (domain, action) = message_type.split_once('.')?;
    Some((Capability::new(domain).ok()?, action.to_owned()))
}

pub(super) fn exact_shell_ready(bytes: &[u8]) -> bool {
    let Ok(serde_json::Value::Object(fields)) = serde_json::from_slice::<serde_json::Value>(bytes)
    else {
        return false;
    };
    fields.len() == 1
        && fields
            .get("type")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|message_type| message_type == "shell.ready")
}

pub(super) fn shell_init_matches_plan(
    response: Option<&BoundedJson>,
    plan: &InjectionPlan,
) -> bool {
    let Some(response) = response.and_then(|response| response.decode().ok()) else {
        return false;
    };
    let Some(domains) = response
        .get("capabilities")
        .and_then(|capabilities| capabilities.get("domains"))
        .and_then(serde_json::Value::as_array)
    else {
        return false;
    };
    let Some(domains) = domains
        .iter()
        .map(serde_json::Value::as_str)
        .collect::<Option<BTreeSet<_>>>()
    else {
        return false;
    };
    let planned = plan
        .domains()
        .iter()
        .map(Capability::as_str)
        .collect::<BTreeSet<_>>();
    domains == planned
}
