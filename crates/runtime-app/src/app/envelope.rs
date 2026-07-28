//! Mapped-envelope dispatch and the exact NAP-SHELL handshake checks.

use std::{collections::BTreeSet, sync::Arc, thread::JoinHandle};

use nmp_native_nap_bridge::{BridgeError, DispatchOutcome, InjectionPlan};
use nmp_native_runtime_core::{BoundedJson, Capability, SessionId, SessionState};

use super::{ActiveOperation, AppState, RuntimeApp};
use crate::{
    activity::ActivityDetail,
    app::diagnostic::{DiagnosticEnvelope, MAXIMUM_SESSION_DIAGNOSTICS, classify_diagnostic},
    commands::{PlatformEvent, ProviderOperationId},
    views::AppErrorCode,
};

/// The most bytes of a raw, unroutable `type` value recorded on an activity
/// fact. Bounded so a hostile envelope cannot inflate the activity ring.
const MAXIMUM_RECORDED_MESSAGE_TYPE_BYTES: usize = 128;

impl RuntimeApp {
    /// Records one classified diagnostic.
    ///
    /// A readable entry becomes a typed event native presentation can render.
    /// An unreadable one still leaves an activity fact, because the failure
    /// mode this replaces was a diagnostic that disappeared without trace.
    fn record_diagnostic(
        &self,
        state: &mut AppState,
        principal: &nmp_native_runtime_core::Principal,
        session_id: SessionId,
        diagnostic: DiagnosticEnvelope,
        now: u64,
    ) {
        match diagnostic {
            DiagnosticEnvelope::Console { level, message } => {
                let Some(entry) = state.sessions.get_mut(&session_id) else {
                    return;
                };
                if entry.diagnostics_mirrored >= MAXIMUM_SESSION_DIAGNOSTICS {
                    return;
                }
                entry.diagnostics_mirrored += 1;
                let exhausted = entry.diagnostics_mirrored == MAXIMUM_SESSION_DIAGNOSTICS;
                self.record_activity(
                    state,
                    principal,
                    "envelope",
                    "diagnostic",
                    level.as_str(),
                    now,
                );
                self.push_event(
                    state,
                    PlatformEvent::NappletDiagnostic {
                        session: session_id,
                        level,
                        message,
                    },
                );
                if exhausted {
                    // Said once, at the boundary, rather than silently from
                    // here on. A console that simply stops is the failure
                    // this whole path exists to prevent.
                    self.record_activity(
                        state,
                        principal,
                        "envelope",
                        "diagnostic",
                        "budget-exhausted",
                        now,
                    );
                }
            }
            DiagnosticEnvelope::Unreadable { reason } => {
                self.record_activity(state, principal, "envelope", "diagnostic", reason, now);
            }
        }
    }

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
        // Classify before any protocol-shaped decision. `debug.*` is
        // reserved: the runtime answers for it, so it never routes, never
        // dispatches, and never reaches a provider — and the shell never has
        // to compare a type string of its own to find that out.
        if let Some(diagnostic) = classify_diagnostic(bytes) {
            self.record_diagnostic(state, &principal, session_id, diagnostic, now);
            return None;
        }
        let route = envelope_route(bytes);
        if route.is_none() {
            // The envelope cannot be routed: its `type` field is missing,
            // is not a string, has no `domain.action` shape, or names a
            // domain `Capability::new` rejects. Dispatch below still runs
            // (the bridge does its own independent parse), but nothing
            // downstream of a route-based branch will ever explain why a
            // caller saw no response. Record the fact here, at the one
            // point that still has the raw bytes, so "why is my napplet
            // dead" is answerable from the activity ring instead of
            // silence.
            self.record_activity_with_details(
                state,
                &principal,
                "envelope",
                "route",
                "unroutable",
                vec![envelope_type_evidence(bytes).detail()],
                now,
            );
        }
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
                // Read independently of `route`. The two disagree on purpose:
                // `link.open` routes fine and is still ignored here for want
                // of a provider, and that is the case a napplet most needs
                // named, because it gets no reply and no refusal either.
                self.push_event(
                    state,
                    PlatformEvent::EnvelopeIgnored {
                        session: session_id,
                        message_type: envelope_type_evidence(bytes).into_named(),
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

/// What the `type` field of an unroutable envelope turned out to be.
///
/// A napplet controls that string, so "the napplet sent this" and "there was
/// nothing to read" cannot both be strings: a napplet sending the literal
/// `<malformed-json>` would otherwise be indistinguishable from a malformed
/// envelope in the evidence a person reads.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum EnvelopeTypeEvidence {
    /// The bounded `type` the napplet actually sent.
    Named(String),
    MalformedJson,
    MissingOrNonStringType,
}

impl EnvelopeTypeEvidence {
    /// The activity detail, keyed by whether there was a type at all.
    ///
    /// The key carries the distinction rather than the value, because the
    /// value is attacker-controlled: a napplet sending the literal
    /// `<malformed-json>` would otherwise produce a detail identical to a
    /// genuinely malformed envelope. Under `type` the string is always the
    /// napplet's own; `type-unavailable` is always the runtime's.
    fn detail(&self) -> ActivityDetail {
        match self {
            Self::Named(message_type) => ActivityDetail::visible("type", message_type),
            Self::MalformedJson => ActivityDetail::visible("type-unavailable", "malformed-json"),
            Self::MissingOrNonStringType => {
                ActivityDetail::visible("type-unavailable", "missing-or-non-string-type")
            }
        }
    }

    /// The napplet-supplied type, if there was one. `None` carries "nothing
    /// to read" without inventing a string for it.
    fn into_named(self) -> Option<String> {
        match self {
            Self::Named(message_type) => Some(message_type),
            Self::MalformedJson | Self::MissingOrNonStringType => None,
        }
    }
}

/// Best-effort, bounded reading of the raw `type` field of an envelope that
/// [`envelope_route`] could not route. Never panics and never grows with
/// attacker-controlled input beyond [`MAXIMUM_RECORDED_MESSAGE_TYPE_BYTES`].
pub(super) fn envelope_type_evidence(bytes: &[u8]) -> EnvelopeTypeEvidence {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return EnvelopeTypeEvidence::MalformedJson;
    };
    let Some(message_type) = value.get("type").and_then(serde_json::Value::as_str) else {
        return EnvelopeTypeEvidence::MissingOrNonStringType;
    };
    EnvelopeTypeEvidence::Named(bounded_utf8_prefix(
        message_type,
        MAXIMUM_RECORDED_MESSAGE_TYPE_BYTES,
    ))
}

/// The longest prefix of `value` that is at most `maximum_bytes` long and
/// still valid UTF-8.
///
/// Bounding by `chars` instead would not bound the bytes: a napplet sending
/// multi-byte characters gets up to four bytes per retained char, so the
/// recorded value overruns the stated maximum by 4x on input it controls.
pub(super) fn bounded_utf8_prefix(value: &str, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        return value.to_owned();
    }
    // The ellipsis is itself three bytes and must fit inside the bound.
    let mut end = maximum_bytes.saturating_sub('…'.len_utf8());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = value[..end].to_owned();
    truncated.push('…');
    truncated
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
