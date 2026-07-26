//! Session launch, lifecycle transitions, teardown, and runtime close.

use std::{collections::BTreeSet, sync::Arc, thread::JoinHandle};

use nmp_native_nap_bridge::{ProviderSessionEnd, SessionContext, SourceWindowId};
use nmp_native_runtime_core::{
    Capability, ExecutionProfile, Principal, ResourceClass, Session, SessionId, SessionState,
};

use super::{AppState, RuntimeApp, SessionEntry};
use crate::{activity::ActivityDetail, commands::PlatformEvent, views::AppErrorCode};

impl RuntimeApp {
    pub(super) fn launch(
        &self,
        state: &mut AppState,
        principal: Principal,
        profile: ExecutionProfile,
        required_domains: BTreeSet<Capability>,
        now: u64,
    ) {
        if !state.installed.contains_key(&principal) {
            self.refuse(
                state,
                AppErrorCode::NotInstalled,
                Some(principal),
                None,
                "launch target is not an installed exact build",
                now,
            );
            return;
        }
        let Some(artifact) = state.artifacts.get(&principal).cloned() else {
            self.refuse(
                state,
                AppErrorCode::OfflineBytesUnavailable,
                Some(principal),
                None,
                "installed exact-build metadata is restored but sealed artifact bytes are not attached",
                now,
            );
            return;
        };
        if state.sessions.len() >= self.limits.maximum_sessions {
            self.refuse(
                state,
                AppErrorCode::Capacity,
                Some(principal),
                None,
                "session capacity is full",
                now,
            );
            return;
        }
        if let Err(error) = self.restore_persistent_grants(&principal) {
            self.refuse_store(state, Some(principal), None, error, now);
            return;
        }
        let advertised_domains = self.bridge.advertised_domains();
        let (grantable_domains, unavailable_domains): (BTreeSet<Capability>, BTreeSet<Capability>) =
            required_domains
                .into_iter()
                .partition(|domain| advertised_domains.contains(domain));
        if !unavailable_domains.is_empty() {
            // One detail per domain rather than one comma-joined blob. The
            // joined string could only be read by splitting it back apart, so
            // in practice nothing did, and the shortfall reached no surface.
            let details = unavailable_domains
                .iter()
                .map(|domain| ActivityDetail::visible("unavailable-domain", domain.as_str()))
                .collect::<Vec<_>>();
            self.record_activity_with_details(
                state,
                &principal,
                "capability",
                "required-domain-unavailable",
                "degraded",
                details,
                now,
            );
        }
        let plan = match self
            .bridge
            .negotiate(&principal, profile, &grantable_domains)
        {
            Ok(plan) => plan,
            Err(error) => {
                self.refuse_bridge(state, Some(principal), None, error, now);
                return;
            }
        };
        let Some(next) = state.next_session_id.checked_add(1) else {
            self.refuse(
                state,
                AppErrorCode::Capacity,
                Some(principal),
                None,
                "session identifier space is exhausted",
                now,
            );
            return;
        };
        let Some(next_source_window) = state.next_source_window_id.checked_add(1) else {
            self.refuse(
                state,
                AppErrorCode::Capacity,
                Some(principal),
                None,
                "source-window identifier space is exhausted",
                now,
            );
            return;
        };
        let session_id = SessionId(next);
        let source_window = SourceWindowId(next_source_window);
        let webview = match self
            .resources
            .admit(session_id, None, ResourceClass::WebView)
        {
            Ok(lease) => lease,
            Err(error) => {
                self.refuse(
                    state,
                    AppErrorCode::Capacity,
                    Some(principal),
                    Some(session_id),
                    error.to_string(),
                    now,
                );
                return;
            }
        };
        let session = Arc::new(Session::new(
            session_id,
            principal.clone(),
            profile,
            Arc::clone(&self.resources),
        ));
        let context = SessionContext {
            id: session_id,
            principal: principal.clone(),
            profile,
        };
        if let Err(error) =
            self.shell_provider
                .prepare_session(&principal, session_id, plan.domains())
        {
            drop(webview);
            self.refuse(
                state,
                AppErrorCode::Bridge,
                Some(principal),
                Some(session_id),
                error.to_string(),
                now,
            );
            return;
        }
        let push_observer =
            match self
                .bridge
                .open_session_bound(&context, &plan, source_window, now)
            {
                Ok(observer) => observer,
                Err(error) => {
                    self.shell_provider.close_session(session_id);
                    drop(webview);
                    self.refuse_bridge(state, Some(principal), Some(session_id), error, now);
                    return;
                }
            };
        if let Err(error) = session.transition(SessionState::Running) {
            self.shell_provider.close_session(session_id);
            self.bridge
                .close_session_with_reason(session_id, ProviderSessionEnd::OpenFailed);
            drop(webview);
            self.refuse_session(state, Some(principal), Some(session_id), error, now);
            return;
        }
        state.next_session_id = next;
        state.next_source_window_id = next_source_window;
        state.sessions.insert(
            session_id,
            SessionEntry {
                session: Arc::clone(&session),
                context,
                plan,
                source_window,
                push_observer: Some(push_observer),
                push_delivery: None,
                unavailable_domains,
                ready: false,
                last_provider_sequence: None,
                delivered_push_count: 0,
                _artifact: artifact,
                _webview: webview,
            },
        );
        self.record_activity(state, &principal, "session", "launch", "running", now);
        self.push_event(state, PlatformEvent::SessionChanged(session.snapshot()));
    }

    pub(super) fn transition_session(
        &self,
        state: &mut AppState,
        session_id: SessionId,
        next: SessionState,
        now: u64,
    ) {
        let Some(entry) = state.sessions.get(&session_id) else {
            self.refuse(
                state,
                AppErrorCode::UnknownSession,
                None,
                Some(session_id),
                "stale or unknown session",
                now,
            );
            return;
        };
        let principal = entry.context.principal.clone();
        let session = Arc::clone(&entry.session);
        if let Err(error) = session.transition(next) {
            self.refuse_session(state, Some(principal), Some(session_id), error, now);
            return;
        }
        let (operation, outcome) = match next {
            SessionState::Suspended => ("suspend", "suspended"),
            SessionState::Running => ("resume", "running"),
            _ => ("transition", "completed"),
        };
        self.record_activity(
            state,
            session.principal(),
            "session",
            operation,
            outcome,
            now,
        );
        self.push_event(state, PlatformEvent::SessionChanged(session.snapshot()));
    }

    pub(super) fn end_session(
        &self,
        state: &mut AppState,
        session_id: SessionId,
        terminal: SessionState,
        reason: Option<Arc<str>>,
        now: u64,
    ) -> Option<JoinHandle<()>> {
        let Some(mut entry) = state.sessions.remove(&session_id) else {
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
        let operation_ids = state
            .operations
            .iter()
            .filter_map(|(id, operation)| (operation.session == session_id).then_some(*id))
            .collect::<Vec<_>>();
        for operation_id in operation_ids {
            if let Some(operation) = state.operations.remove(&operation_id) {
                operation.cancel(Arc::from("provider operation cancelled"));
            }
        }
        self.shell_provider.close_session(session_id);
        let provider_reason = match terminal {
            SessionState::Crashed => ProviderSessionEnd::Crashed,
            _ => ProviderSessionEnd::Stopped,
        };
        self.bridge
            .close_session_with_reason(session_id, provider_reason);
        let transition = if terminal == SessionState::Stopped {
            entry.session.stop();
            Ok(())
        } else {
            entry.session.transition(terminal)
        };
        if let Err(error) = transition {
            self.refuse_session(
                state,
                Some(entry.context.principal.clone()),
                Some(session_id),
                error,
                now,
            );
        }
        let snapshot = entry.session.snapshot();
        let outcome = match terminal {
            SessionState::Crashed => reason
                .as_deref()
                .map_or("crashed".to_owned(), |reason| format!("crashed:{reason}")),
            _ => "stopped".to_owned(),
        };
        self.record_activity(
            state,
            &entry.context.principal,
            "session",
            "teardown",
            &outcome,
            now,
        );
        let delivery_join = entry
            .push_delivery
            .take()
            .and_then(|mut delivery| delivery.join.take());
        drop(entry);
        self.push_event(state, PlatformEvent::SessionChanged(snapshot));
        delivery_join
    }

    pub(super) fn close(&self, state: &mut AppState, now: u64) -> Vec<JoinHandle<()>> {
        if state.closed {
            return Vec::new();
        }
        let mut delivery_joins = Vec::new();
        let sessions = state.sessions.keys().copied().collect::<Vec<_>>();
        for session in sessions {
            self.bridge
                .close_session_with_reason(session, ProviderSessionEnd::RuntimeClosed);
            if let Some(join) = self.end_session(state, session, SessionState::Stopped, None, now) {
                delivery_joins.push(join);
            }
        }
        let bindings = state.bindings.keys().cloned().collect::<Vec<_>>();
        for binding in bindings {
            self.close_binding(state, &binding, now);
        }
        for (_, receipt) in std::mem::take(&mut state.receipts) {
            receipt.stop_delivery();
        }
        state.operations.clear();
        state.artifacts.clear();
        state.closed = true;
        self.push_event(state, PlatformEvent::Closed);
        delivery_joins
    }
}
