use std::sync::Arc;

use nmp_native_runtime_core::{Capability, GrantDecision, ResourceClass, WorkLease};
use serde_json::Value;

use crate::{
    ActivityOutcome, BridgeError, DispatchOutcome, Envelope, ProviderActivity, ProviderRequest,
    SessionContext,
};

use super::{InjectionPlan, ProviderRegistry, is_foundational_shell};

impl ProviderRegistry {
    /// Dispatches a mapped message. Unknown well-formed types are ignored.
    pub fn dispatch(
        &self,
        context: &SessionContext,
        plan: &InjectionPlan,
        bytes: &[u8],
        now_millis: u64,
    ) -> Result<DispatchOutcome, BridgeError> {
        if bytes.len() > self.limits.maximum_envelope_bytes {
            self.state.lock().refusals += 1;
            return Err(BridgeError::EnvelopeTooLarge {
                actual: bytes.len(),
                maximum: self.limits.maximum_envelope_bytes,
            });
        }
        self.take_rate_token(context, now_millis)?;
        let envelope: Envelope =
            serde_json::from_slice(bytes).map_err(|error| BridgeError::MalformedEnvelope {
                reason: error.to_string(),
            })?;
        let Some((domain_text, action_text)) = envelope.message_type.split_once('.') else {
            self.state.lock().ignored_unknown += 1;
            return Ok(DispatchOutcome::IgnoredUnknown);
        };
        let domain = Capability::new(domain_text).map_err(|_| BridgeError::MalformedEnvelope {
            reason: "invalid domain name".to_owned(),
        })?;
        let Some(provider) = self.providers.get(&domain) else {
            self.state.lock().ignored_unknown += 1;
            return Ok(DispatchOutcome::IgnoredUnknown);
        };
        if !provider.descriptor().actions.contains(action_text) {
            self.state.lock().ignored_unknown += 1;
            return Ok(DispatchOutcome::IgnoredUnknown);
        }
        if context.principal != plan.principal {
            self.record_refusal(context, &domain, Arc::from(action_text));
            return Err(BridgeError::PlanPrincipalMismatch);
        }
        if context.profile != plan.profile || !plan.exposes(&domain) {
            self.record_refusal(context, &domain, Arc::from(action_text));
            return Err(BridgeError::CapabilityDenied { domain });
        }
        let lease = match self.admit_authorized_call(context, &domain) {
            Ok(lease) => lease,
            Err(error) => {
                self.record_refusal(context, &domain, Arc::from(action_text));
                return Err(error);
            }
        };
        let request = ProviderRequest {
            principal: context.principal.clone(),
            session: context.id,
            action: Arc::from(action_text),
            correlation_id: envelope.id.map(Arc::from),
            payload: Value::Object(envelope.fields),
            work: lease,
        };
        let action = Arc::clone(&request.action);
        match provider.call(request) {
            Ok(call) => {
                if call.response.as_ref().is_some_and(|response| {
                    response.byte_len() > self.limits.maximum_response_bytes
                }) {
                    self.record_refusal(context, &domain, action);
                    return Err(BridgeError::ResponseTooLarge);
                }
                let outcome = if call.is_active() {
                    ActivityOutcome::Active
                } else {
                    ActivityOutcome::Completed
                };
                self.activity.record(ProviderActivity {
                    principal: context.principal.clone(),
                    session: context.id,
                    domain,
                    action,
                    outcome,
                });
                self.state.lock().dispatched += 1;
                Ok(DispatchOutcome::Handled(call))
            }
            Err(source) => {
                self.record_refusal(context, &domain, action);
                Err(BridgeError::Provider(source))
            }
        }
    }

    fn take_rate_token(
        &self,
        context: &SessionContext,
        now_millis: u64,
    ) -> Result<(), BridgeError> {
        let mut state = self.state.lock();
        let slot = state
            .sessions
            .get_mut(&context.id)
            .ok_or(BridgeError::UnknownSession {
                session: context.id,
            })?;
        if slot.principal != context.principal || slot.profile != context.profile {
            return Err(BridgeError::SessionIdentityMismatch {
                session: context.id,
            });
        }
        let bucket = &mut slot.bucket;
        let elapsed = now_millis.saturating_sub(bucket.updated_at_millis);
        let refill = elapsed.saturating_mul(u64::from(self.limits.message_refill_per_second));
        let capacity = u64::from(self.limits.message_burst) * 1_000;
        bucket.tokens_milli = bucket.tokens_milli.saturating_add(refill).min(capacity);
        bucket.updated_at_millis = now_millis;
        if bucket.tokens_milli < 1_000 {
            state.throttles += 1;
            return Err(BridgeError::MessageRateExceeded {
                session: context.id,
            });
        }
        bucket.tokens_milli -= 1_000;
        Ok(())
    }

    /// Serializes admission with bridge-owned revocation so a dispatch cannot
    /// slip new work into the interval between a live grant check and active
    /// work cancellation.
    fn admit_authorized_call(
        &self,
        context: &SessionContext,
        domain: &Capability,
    ) -> Result<WorkLease, BridgeError> {
        let _state = self.state.lock();
        if is_foundational_shell(domain) {
            return self
                .resources
                .admit(
                    context.id,
                    Some(domain.clone()),
                    ResourceClass::ProviderCall,
                )
                .map_err(BridgeError::ResourceRefused);
        }
        match self.grants.decision(&context.principal, domain) {
            decision if decision.allows_without_prompt() => self
                .resources
                .admit(
                    context.id,
                    Some(domain.clone()),
                    ResourceClass::ProviderCall,
                )
                .map_err(BridgeError::ResourceRefused),
            GrantDecision::AskEveryTime => Err(BridgeError::GrantDecisionRequired {
                domain: domain.clone(),
            }),
            GrantDecision::Denied => Err(BridgeError::CapabilityDenied {
                domain: domain.clone(),
            }),
            GrantDecision::AllowSession
            | GrantDecision::AllowExactBuild
            | GrantDecision::Managed => unreachable!("covered by allows_without_prompt"),
        }
    }

    fn record_refusal(&self, context: &SessionContext, domain: &Capability, action: Arc<str>) {
        self.state.lock().refusals += 1;
        self.activity.record(ProviderActivity {
            principal: context.principal.clone(),
            session: context.id,
            domain: domain.clone(),
            action,
            outcome: ActivityOutcome::Refused,
        });
    }
}
