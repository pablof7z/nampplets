//! Session binding and dispatch for the lists provider.
//!
//! Kept apart from the mutation logic so the decision path in `provider.rs`
//! stays readable: this file is only about which lane a request is allowed to
//! arrive on.

use std::sync::Arc;

use nmp_native_nap_bridge::{
    Provider, ProviderCall, ProviderDescriptor, ProviderError, ProviderRequest, ProviderSession,
    ProviderSessionContext, ProviderSessionEnd,
};

use crate::{
    DOMAIN,
    provider::{ListsProvider, ListsSession, ListsState},
    wire::{failed, invalid_payload},
    write::ListsAction,
};

impl Provider for ListsProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn call(&self, request: ProviderRequest) -> Result<ProviderCall, ProviderError> {
        match request.action.as_ref() {
            "supported" => self.supported(request),
            "add" => self.mutate(ListsAction::Add, request),
            "remove" => self.mutate(ListsAction::Remove, request),
            _ => Err(invalid_payload(&request, "unknown action")),
        }
    }

    fn session_opened(&self, session: ProviderSession) -> Result<(), ProviderError> {
        let mut state = self.state.lock();
        if state.closed {
            return Err(failed("session.open", "lists provider is closed"));
        }
        if session.outbound.domain().as_str() != DOMAIN
            || session.outbound.session() != session.context.session
        {
            return Err(ProviderError::Denied {
                domain: Arc::from(DOMAIN),
                action: Arc::from("session.open"),
                reason: Arc::from("outbound lists lane does not match the mapped session"),
            });
        }
        if let Some(existing) = state.sessions.get(&session.context.session) {
            return if existing.principal == session.context.principal
                && existing.outbound.source_window() == session.context.source_window
            {
                Ok(())
            } else {
                Err(ProviderError::Denied {
                    domain: Arc::from(DOMAIN),
                    action: Arc::from("session.open"),
                    reason: Arc::from("session id is already bound to another lists lane"),
                })
            };
        }
        if state.sessions.len() >= self.limits.maximum_sessions {
            return Err(ProviderError::Denied {
                domain: Arc::from(DOMAIN),
                action: Arc::from("session.open"),
                reason: Arc::from(format!(
                    "lists session capacity {} is full",
                    self.limits.maximum_sessions
                )),
            });
        }
        state.sessions.insert(
            session.context.session,
            ListsSession {
                principal: session.context.principal,
                outbound: session.outbound,
            },
        );
        Ok(())
    }

    fn session_closed(&self, context: &ProviderSessionContext, _reason: ProviderSessionEnd) {
        remove_exact_session(&mut self.state.lock(), context);
    }

    fn session_revoked(&self, context: &ProviderSessionContext) {
        remove_exact_session(&mut self.state.lock(), context);
    }
}

fn remove_exact_session(state: &mut ListsState, context: &ProviderSessionContext) {
    let matches = state.sessions.get(&context.session).is_some_and(|session| {
        session.principal == context.principal
            && session.outbound.source_window() == context.source_window
    });
    if matches {
        state.sessions.remove(&context.session);
    }
}
