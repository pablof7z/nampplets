use std::sync::Arc;

use nmp_native_nap_bridge::{ProviderDescriptor, ProviderPlatformAvailability};
use nmp_native_runtime_core::{Capability, GrantDecision, Sensitivity};
use nmp_native_runtime_store::PermissionDefaultPreference;

use crate::views::{PermissionDecisionOption, PermissionPlatformAvailability};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PermissionDecisionPolicy {
    pub requested: Option<GrantDecision>,
    pub recommended: Option<GrantDecision>,
    pub options: Vec<PermissionDecisionOption>,
}

pub(super) fn permission_decision_policy(
    current: GrantDecision,
    availability: &PermissionPlatformAvailability,
    permission_default: PermissionDefaultPreference,
    use_profile_default: bool,
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
    } else if use_profile_default {
        match permission_default {
            PermissionDefaultPreference::AskEveryTime => GrantDecision::AskEveryTime,
            PermissionDefaultPreference::AllowSession => GrantDecision::AllowSession,
            PermissionDefaultPreference::AllowExactBuild => GrantDecision::AllowExactBuild,
        }
    } else {
        current
    };
    let options = user_decisions
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
        .collect::<Vec<_>>();
    PermissionDecisionPolicy {
        requested: Some(requested),
        recommended: Some(recommended_decision(&options)),
        options,
    }
}

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
