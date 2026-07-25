use nmp_native_runtime_core::{Capability, Principal};

use crate::ProviderSessionContext;

use super::ProviderRegistry;

impl ProviderRegistry {
    /// Revokes an exact-build grant and signals all matching active work.
    ///
    /// Existing injection plans become unusable immediately because dispatch
    /// rechecks the live grant ledger before admitting provider work.
    pub fn revoke(&self, principal: &Principal, domain: &Capability) -> usize {
        self.cancel_capability(principal, domain, true)
    }

    /// Cancels every non-durable operation and provider-push lane for one
    /// exact-build capability after an owner-level grant transaction changed
    /// the ledger. This does not overwrite the newly committed decision.
    pub fn cancel_capability_work(&self, principal: &Principal, domain: &Capability) -> usize {
        self.cancel_capability(principal, domain, false)
    }

    fn cancel_capability(
        &self,
        principal: &Principal,
        domain: &Capability,
        deny_grant: bool,
    ) -> usize {
        let state = self.state.lock();
        let sessions = state
            .sessions
            .iter()
            .filter_map(|(session, slot)| (&slot.principal == principal).then_some(*session))
            .collect::<Vec<_>>();
        let lifecycle = state
            .sessions
            .iter()
            .filter(|(_, slot)| &slot.principal == principal && slot.domains.contains(domain))
            .map(|(session, slot)| {
                slot.outbound.revoke(domain);
                ProviderSessionContext {
                    principal: slot.principal.clone(),
                    session: *session,
                    source_window: slot.source_window,
                    profile: slot.profile,
                }
            })
            .collect::<Vec<_>>();
        let cancelled = if deny_grant {
            self.grants.revoke(principal, domain, sessions)
        } else {
            sessions
                .into_iter()
                .map(|session| self.resources.cancel_session_capability(session, domain))
                .sum()
        };
        drop(state);
        if let Some(provider) = self.providers.get(domain) {
            for context in lifecycle {
                provider.session_revoked(&context);
            }
        }
        cancelled
    }
}
