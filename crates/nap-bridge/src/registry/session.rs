use std::{collections::BTreeSet, sync::Arc};

use nmp_native_runtime_core::{Capability, SessionId};

use crate::outbound::OutboundMailbox;
use crate::{
    BridgeError, Provider, ProviderPushObserver, ProviderSession, ProviderSessionContext,
    ProviderSessionEnd, SessionContext, SourceWindowId,
};

use super::{InjectionPlan, ProviderRegistry, SessionSlot, TokenBucket};

impl ProviderRegistry {
    pub fn open_session(
        &self,
        context: &SessionContext,
        now_millis: u64,
    ) -> Result<(), BridgeError> {
        self.insert_session(
            context,
            SourceWindowId(context.id.0),
            BTreeSet::new(),
            now_millis,
        )
        .map(|_| ())
    }

    /// Production session boundary. The source-window token and immutable
    /// injection plan are trusted runtime values, never envelope fields.
    pub fn open_session_bound(
        &self,
        context: &SessionContext,
        plan: &InjectionPlan,
        source_window: SourceWindowId,
        now_millis: u64,
    ) -> Result<ProviderPushObserver, BridgeError> {
        if context.principal != plan.principal {
            return Err(BridgeError::PlanPrincipalMismatch);
        }
        if context.profile != plan.profile {
            return Err(BridgeError::SessionIdentityMismatch {
                session: context.id,
            });
        }
        let outbound =
            self.insert_session(context, source_window, plan.domains.clone(), now_millis)?;
        let lifecycle_context = ProviderSessionContext {
            principal: context.principal.clone(),
            session: context.id,
            source_window,
            profile: context.profile,
        };
        let mut opened: Vec<Arc<dyn Provider>> = Vec::new();
        for domain in plan.domains() {
            let Some(provider) = self.providers.get(domain) else {
                continue;
            };
            let session = ProviderSession {
                context: lifecycle_context.clone(),
                outbound: outbound.sender(domain.clone()),
            };
            if let Err(source) = provider.session_opened(session) {
                outbound.close();
                self.state.lock().sessions.remove(&context.id);
                self.resources.cancel_session(context.id);
                for opened_provider in opened.into_iter().rev() {
                    opened_provider
                        .session_closed(&lifecycle_context, ProviderSessionEnd::OpenFailed);
                }
                return Err(BridgeError::Provider(source));
            }
            opened.push(Arc::clone(provider));
        }
        Ok(outbound.observe())
    }

    fn insert_session(
        &self,
        context: &SessionContext,
        source_window: SourceWindowId,
        domains: BTreeSet<Capability>,
        now_millis: u64,
    ) -> Result<Arc<OutboundMailbox>, BridgeError> {
        let mut state = self.state.lock();
        if let Some(existing) = state.sessions.get(&context.id) {
            return if existing.principal == context.principal
                && existing.profile == context.profile
                && existing.source_window == source_window
                && existing.domains == domains
            {
                Ok(Arc::clone(&existing.outbound))
            } else {
                Err(BridgeError::SessionIdentityMismatch {
                    session: context.id,
                })
            };
        }
        if state.sessions.len() >= self.limits.maximum_sessions {
            return Err(BridgeError::SessionCapacity {
                capacity: self.limits.maximum_sessions,
            });
        }
        let outbound = OutboundMailbox::new(
            context.principal.clone(),
            context.id,
            source_window,
            self.limits.provider_pushes,
        );
        state.sessions.insert(
            context.id,
            SessionSlot {
                principal: context.principal.clone(),
                profile: context.profile,
                bucket: TokenBucket {
                    tokens_milli: u64::from(self.limits.message_burst) * 1_000,
                    updated_at_millis: now_millis,
                },
                source_window,
                domains,
                ready: false,
                outbound: Arc::clone(&outbound),
            },
        );
        Ok(outbound)
    }

    pub fn close_session(&self, session: SessionId) {
        self.close_session_with_reason(session, ProviderSessionEnd::Stopped);
    }

    pub fn close_session_with_reason(&self, session: SessionId, reason: ProviderSessionEnd) {
        let Some(slot) = self.state.lock().sessions.remove(&session) else {
            return;
        };
        slot.outbound.close();
        self.resources.cancel_session(session);
        let context = ProviderSessionContext {
            principal: slot.principal,
            session,
            source_window: slot.source_window,
            profile: slot.profile,
        };
        for domain in slot.domains {
            if let Some(provider) = self.providers.get(&domain) {
                provider.session_closed(&context, reason);
            }
        }
    }

    pub fn mark_session_ready(&self, session: SessionId) -> Result<(), BridgeError> {
        let (context, domains) = {
            let mut state = self.state.lock();
            let slot = state
                .sessions
                .get_mut(&session)
                .ok_or(BridgeError::UnknownSession { session })?;
            if slot.ready {
                return Ok(());
            }
            slot.ready = true;
            (
                ProviderSessionContext {
                    principal: slot.principal.clone(),
                    session,
                    source_window: slot.source_window,
                    profile: slot.profile,
                },
                slot.domains.clone(),
            )
        };
        for domain in domains {
            if let Some(provider) = self.providers.get(&domain)
                && let Err(source) = provider.session_ready(&context)
            {
                return Err(BridgeError::Provider(source));
            }
        }
        Ok(())
    }

    pub fn observe_pushes(
        &self,
        session: SessionId,
        source_window: SourceWindowId,
    ) -> Result<ProviderPushObserver, BridgeError> {
        let state = self.state.lock();
        let slot = state
            .sessions
            .get(&session)
            .ok_or(BridgeError::UnknownSession { session })?;
        if slot.source_window != source_window {
            return Err(BridgeError::SourceWindowMismatch {
                session,
                source_window,
            });
        }
        Ok(slot.outbound.observe())
    }
}
