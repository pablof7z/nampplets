mod support;

use nmp_native_runtime_ffi::{RuntimePermissionRequirement, RuntimePermissionSensitivity};
use support::PermissionReviewRig;

#[test]
fn published_exact_build_review_and_launch_refusal_cross_the_ffi_facade() {
    let rig = PermissionReviewRig::new();
    assert!(
        rig.has_no_signed_requirements(),
        "the immutable manifest must not gain synthetic requires tags"
    );

    let review = rig.permission_review();
    assert_eq!(&review.coordinate, rig.coordinate());
    let mut reviewed_domains = review
        .capabilities
        .iter()
        .map(|capability| capability.domain.clone())
        .collect::<Vec<_>>();
    reviewed_domains.sort();
    let mut embedded_domains = rig.embedded_domains().to_vec();
    embedded_domains.sort();
    assert_eq!(reviewed_domains, embedded_domains);
    // The inventory is the artifact's own embedded declaration and nothing
    // else. No build's identity selects a profile, so nothing is softened to
    // optional on the strength of an author/d-tag/aggregate match -- every
    // domain the napplet declared is required.
    assert!(
        review
            .capabilities
            .iter()
            .all(|capability| capability.requirement == RuntimePermissionRequirement::Required),
        "a declared domain must not be downgraded to optional: {:?}",
        review
            .capabilities
            .iter()
            .map(|capability| (capability.domain.as_str(), capability.requirement))
            .collect::<Vec<_>>()
    );
    assert!(!review.launch_permitted);
    assert_eq!(
        review
            .capabilities
            .iter()
            .find(|capability| capability.domain == "outbox")
            .expect("outbox permission")
            .sensitivity,
        RuntimePermissionSensitivity::Sensitive
    );

    rig.launch_without_grants();
    let snapshot = rig.snapshot();
    assert!(snapshot.sessions.is_empty());
    let refusal = snapshot
        .recent_errors
        .last()
        .expect("launch refusal evidence");
    assert_eq!(refusal.code, "bridge");
    assert_eq!(
        (
            refusal.author.as_deref(),
            refusal.d_tag.as_deref(),
            refusal.aggregate_hash.as_deref(),
        ),
        (
            Some(rig.coordinate().manifest_author.as_str()),
            Some(rig.coordinate().d_tag.as_str()),
            Some(rig.coordinate().aggregate_hash.as_str()),
        )
    );
}
