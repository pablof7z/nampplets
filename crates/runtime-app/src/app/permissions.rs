//! Grant decisions, permission review projection, and persistent grant
//! restoration.

mod batch;

use std::sync::Arc;

use nmp_native_nap_bridge::{ProviderDescriptor, ProviderPlatformAvailability};
use nmp_native_runtime_core::{Capability, GrantDecision, Principal, Sensitivity};
use nmp_native_runtime_store::StoreError;

use super::{AppState, RuntimeApp};
use crate::{
    commands::PlatformEvent,
    views::{
        AppErrorCode, PermissionCapabilityView, PermissionDecisionOption,
        PermissionPlatformAvailability, PermissionReviewError, PermissionReviewView,
    },
};

impl RuntimeApp {
    /// Builds one bounded exact-build permission review from Rust-owned
    /// installation requests, provider metadata, live session grants, and
    /// durable grant rows. Missing provider metadata stays explicitly unknown.
    pub fn permission_review(
        &self,
        principal: &Principal,
    ) -> Result<PermissionReviewView, PermissionReviewError> {
        let build = self
            .state
            .lock()
            .installed
            .get(principal)
            .cloned()
            .ok_or(PermissionReviewError::NotInstalled)?;
        let mut capabilities = Vec::with_capacity(build.capability_requests.len());
        for request in &build.capability_requests {
            let persistent = self
                .store
                .grant(principal, &request.capability)
                .map_err(|error| PermissionReviewError::Store {
                    detail: Arc::from(error.to_string()),
                })?;
            let current_decision = self
                .grants
                .decision_entry(principal, &request.capability)
                .unwrap_or(persistent);
            let descriptor = self.bridge.permission_descriptor(&request.capability);
            let (sensitivity, dependencies, platform_availability) =
                permission_provider_projection(descriptor);
            let policy = permission_decision_policy(current_decision, &platform_availability);
            capabilities.push(PermissionCapabilityView {
                capability: request.capability.clone(),
                requirement: request.requirement,
                sensitivity,
                dependencies,
                platform_availability,
                current_decision,
                is_granted: current_decision.allows_without_prompt(),
                requested_decision: policy.requested,
                recommended_decision: policy.recommended,
                decision_options: policy.options,
            });
        }
        Ok(PermissionReviewView {
            principal: principal.clone(),
            title: build.title,
            capabilities,
        })
    }

    pub(super) fn set_grant(
        &self,
        state: &mut AppState,
        principal: Principal,
        capability: Capability,
        sensitivity: Sensitivity,
        decision: GrantDecision,
        now: u64,
    ) {
        if !state.installed.contains_key(&principal) {
            self.refuse(
                state,
                AppErrorCode::NotInstalled,
                Some(principal),
                None,
                "grant target is not an installed exact build",
                now,
            );
            return;
        }
        if capability.as_str() == "shell" {
            self.refuse(
                state,
                AppErrorCode::Grant,
                Some(principal),
                None,
                "foundational shell is mandatory and is not grant-controlled",
                now,
            );
            return;
        }
        let previous = self.grants.decision(&principal, &capability);
        if let Err(error) =
            self.grants
                .set(principal.clone(), capability.clone(), sensitivity, decision)
        {
            self.refuse(
                state,
                AppErrorCode::Grant,
                Some(principal),
                None,
                error.to_string(),
                now,
            );
            return;
        }
        let persistent = decision != GrantDecision::AllowSession;
        if persistent && let Err(error) = self.store.set_grant(&principal, &capability, decision) {
            let _ = self
                .grants
                .set(principal.clone(), capability.clone(), sensitivity, previous);
            self.refuse_store(state, Some(principal), None, error, now);
            return;
        }
        self.record_activity(
            state,
            &principal,
            "grant",
            capability.as_str(),
            grant_outcome(decision),
            now,
        );
        self.push_event(
            state,
            PlatformEvent::GrantChanged {
                principal,
                capability,
                decision,
            },
        );
    }

    pub(super) fn current_grant_decision(
        &self,
        principal: &Principal,
        capability: &Capability,
    ) -> Result<GrantDecision, StoreError> {
        match self.grants.decision_entry(principal, capability) {
            Some(decision) => Ok(decision),
            None => self.store.grant(principal, capability),
        }
    }

    pub(super) fn revoke(
        &self,
        state: &mut AppState,
        principal: Principal,
        capability: Capability,
        now: u64,
    ) {
        if capability.as_str() == "shell" {
            self.refuse(
                state,
                AppErrorCode::Grant,
                Some(principal),
                None,
                "foundational shell is mandatory and is not grant-controlled",
                now,
            );
            return;
        }
        if let Err(error) = self
            .store
            .set_grant(&principal, &capability, GrantDecision::Denied)
        {
            self.refuse_store(state, Some(principal), None, error, now);
            return;
        }
        self.bridge.revoke(&principal, &capability);
        let operations = state
            .operations
            .iter()
            .filter_map(|(id, operation)| {
                (operation.principal == principal && operation.domain == capability)
                    .then_some((*id, operation.session))
            })
            .collect::<Vec<_>>();
        for (id, _) in operations {
            if let Some(operation) = state.operations.remove(&id) {
                operation.cancel(Arc::from("session ended"));
            }
        }
        self.record_activity(
            state,
            &principal,
            "grant",
            capability.as_str(),
            "revoked",
            now,
        );
        self.push_event(
            state,
            PlatformEvent::GrantChanged {
                principal,
                capability,
                decision: GrantDecision::Denied,
            },
        );
    }

    pub(super) fn restore_persistent_grants(
        &self,
        principal: &Principal,
    ) -> Result<(), StoreError> {
        for capability in self.bridge.advertised_domains() {
            let decision = self.store.grant(principal, &capability)?;
            if decision != GrantDecision::Denied {
                self.grants
                    .set(
                        principal.clone(),
                        capability,
                        Sensitivity::Sensitive,
                        decision,
                    )
                    .map_err(|error| StoreError::Corrupt(error.to_string()))?;
            }
        }
        Ok(())
    }
}

pub(super) fn grant_outcome(decision: GrantDecision) -> &'static str {
    match decision {
        GrantDecision::Denied => "denied",
        GrantDecision::AskEveryTime => "ask-every-time",
        GrantDecision::AllowSession => "allowed-session",
        GrantDecision::AllowExactBuild => "allowed-exact-build",
        GrantDecision::Managed => "managed",
    }
}

pub(super) fn permission_provider_projection(
    descriptor: Option<ProviderDescriptor>,
) -> (
    Option<Sensitivity>,
    Vec<Capability>,
    PermissionPlatformAvailability,
) {
    match descriptor {
        Some(descriptor) => {
            let sensitivity = Some(if descriptor.sensitive {
                Sensitivity::Sensitive
            } else {
                Sensitivity::Ordinary
            });
            let dependencies = descriptor.dependencies.into_iter().collect();
            let availability = match descriptor.platform_availability {
                ProviderPlatformAvailability::Available => {
                    PermissionPlatformAvailability::Available
                }
                ProviderPlatformAvailability::Unavailable { reason } => {
                    PermissionPlatformAvailability::Unavailable { reason }
                }
            };
            (sensitivity, dependencies, availability)
        }
        None => (
            None,
            Vec::new(),
            PermissionPlatformAvailability::Unknown {
                reason: Arc::from(
                    "no provider metadata is registered for this capability on this runtime",
                ),
            },
        ),
    }
}

/// One capability's complete decision policy, decided once by Rust.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PermissionDecisionPolicy {
    pub requested: Option<GrantDecision>,
    pub recommended: Option<GrantDecision>,
    pub options: Vec<PermissionDecisionOption>,
}

pub(super) fn permission_decision_policy(
    current: GrantDecision,
    availability: &PermissionPlatformAvailability,
) -> PermissionDecisionPolicy {
    let user_decisions = [
        GrantDecision::Denied,
        GrantDecision::AskEveryTime,
        GrantDecision::AllowSession,
        GrantDecision::AllowExactBuild,
    ];
    if current == GrantDecision::Managed {
        let reason: Arc<str> = Arc::from("this capability is managed by host policy");
        return PermissionDecisionPolicy {
            requested: None,
            recommended: None,
            options: user_decisions
                .into_iter()
                .map(|decision| PermissionDecisionOption {
                    decision,
                    valid: false,
                    invalid_reason: Some(Arc::clone(&reason)),
                })
                .collect(),
        };
    }
    let unavailable_reason = match availability {
        PermissionPlatformAvailability::Available => None,
        PermissionPlatformAvailability::Unknown { reason }
        | PermissionPlatformAvailability::Unavailable { reason } => Some(Arc::clone(reason)),
    };
    let requested = if unavailable_reason.is_some() {
        GrantDecision::Denied
    } else if current == GrantDecision::Denied {
        GrantDecision::AskEveryTime
    } else {
        current
    };
    let options: Vec<PermissionDecisionOption> = user_decisions
        .into_iter()
        .map(|decision| {
            let invalid_reason = (decision != GrantDecision::Denied)
                .then(|| unavailable_reason.clone())
                .flatten();
            PermissionDecisionOption {
                decision,
                valid: invalid_reason.is_none(),
                invalid_reason,
            }
        })
        .collect();
    PermissionDecisionPolicy {
        requested: Some(requested),
        recommended: Some(recommended_decision(&options)),
        options,
    }
}

/// The broadest affirmative decision this runtime currently accepts, falling
/// back to `Denied` when the platform offers no affirmative option at all.
/// Breadth is declared by `runtime-core`, never by a caller.
fn recommended_decision(options: &[PermissionDecisionOption]) -> GrantDecision {
    GrantDecision::AFFIRMATIVE_BY_BREADTH
        .into_iter()
        .find(|affirmative| {
            options
                .iter()
                .any(|option| option.decision == *affirmative && option.valid)
        })
        .unwrap_or(GrantDecision::Denied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_capability_recommends_the_broadest_affirmative_decision() {
        let policy = permission_decision_policy(
            GrantDecision::AskEveryTime,
            &PermissionPlatformAvailability::Available,
        );

        assert_eq!(policy.recommended, Some(GrantDecision::AllowExactBuild));
        assert!(
            policy
                .recommended
                .is_some_and(GrantDecision::allows_without_prompt)
        );
    }

    #[test]
    fn unavailable_capability_recommends_denied_because_nothing_affirmative_is_valid() {
        let policy = permission_decision_policy(
            GrantDecision::AllowExactBuild,
            &PermissionPlatformAvailability::Unavailable {
                reason: Arc::from("no provider on this platform"),
            },
        );

        assert_eq!(policy.recommended, Some(GrantDecision::Denied));
        assert!(
            policy
                .options
                .iter()
                .all(|option| { option.decision == GrantDecision::Denied || !option.valid })
        );
    }

    #[test]
    fn managed_capability_recommends_nothing_because_the_user_decides_nothing() {
        let policy = permission_decision_policy(
            GrantDecision::Managed,
            &PermissionPlatformAvailability::Available,
        );

        assert_eq!(policy.requested, None);
        assert_eq!(policy.recommended, None);
        assert!(policy.options.iter().all(|option| !option.valid));
    }
}
