use super::*;
use nmp_native_runtime_store::PermissionDefaultPreference;

use crate::views::PermissionPlatformAvailability;

#[test]
fn available_capability_recommends_the_broadest_affirmative_decision() {
    let policy = permission_decision_policy(
        GrantDecision::AskEveryTime,
        &PermissionPlatformAvailability::Available,
        PermissionDefaultPreference::AskEveryTime,
        false,
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
        PermissionDefaultPreference::AllowExactBuild,
        false,
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
        PermissionDefaultPreference::AllowSession,
        false,
    );

    assert_eq!(policy.requested, None);
    assert_eq!(policy.recommended, None);
    assert!(policy.options.iter().all(|option| !option.valid));
}

#[test]
fn new_capability_uses_the_profile_default_without_skipping_review() {
    for (permission_default, requested) in [
        (
            PermissionDefaultPreference::AskEveryTime,
            GrantDecision::AskEveryTime,
        ),
        (
            PermissionDefaultPreference::AllowSession,
            GrantDecision::AllowSession,
        ),
        (
            PermissionDefaultPreference::AllowExactBuild,
            GrantDecision::AllowExactBuild,
        ),
    ] {
        let policy = permission_decision_policy(
            GrantDecision::Denied,
            &PermissionPlatformAvailability::Available,
            permission_default,
            true,
        );
        assert_eq!(policy.requested, Some(requested));
        assert_eq!(
            policy.options.iter().filter(|option| option.valid).count(),
            4,
            "the default selects an offered decision; it does not apply a grant"
        );
    }
}

#[test]
fn explicit_denial_is_not_replaced_by_the_profile_default() {
    let policy = permission_decision_policy(
        GrantDecision::Denied,
        &PermissionPlatformAvailability::Available,
        PermissionDefaultPreference::AllowExactBuild,
        false,
    );

    assert_eq!(policy.requested, Some(GrantDecision::Denied));
}
