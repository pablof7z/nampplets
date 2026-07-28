//! Cucumber scenario runner for the public runtime-ffi facade.

// Step modules live under `tests/bdd/` so Cargo does not also auto-discover
// them as standalone integration-test targets, where `super::` and
// `crate::support` would not resolve. This file is the target's crate root, so
// `mod` resolves against `tests/` and the path must be spelled out. Same shape
// as `crates/runtime-app/tests/bdd.rs`.
#[path = "bdd/boundary_refusal_steps.rs"]
mod boundary_refusal_steps;
#[path = "bdd/operator_relay_steps.rs"]
mod operator_relay_steps;
#[path = "bdd/receipt_steps.rs"]
mod receipt_steps;
mod support;

use cucumber::{World, given, then, when};
use nmp_native_runtime_ffi::{
    RuntimeGrantDecision, RuntimePermissionBatchUpdate, RuntimePermissionChangeRefusalCode,
    RuntimePermissionDecisionController, RuntimePermissionDecisionSelection,
    RuntimePermissionExistingDecision, RuntimePermissionReviewSnapshot, RuntimeReceiptSnapshot,
    RuntimeSnapshot,
};
use support::{PermissionReviewRig, ReceiptProjectionRig};

#[derive(Debug, Default, World)]
struct RuntimeFfiWorld {
    rig: Option<PermissionReviewRig>,
    review: Option<RuntimePermissionReviewSnapshot>,
    update: Option<RuntimePermissionBatchUpdate>,
    snapshot: Option<RuntimeSnapshot>,
    receipt_rig: Option<ReceiptProjectionRig>,
    receipt: Option<RuntimeReceiptSnapshot>,
    prior_receipt: Option<RuntimeReceiptSnapshot>,
    relays: operator_relay_steps::OperatorRelayLanes,
    refusals: boundary_refusal_steps::BoundaryRefusals,
}

impl RuntimeFfiWorld {
    fn rig(&self) -> &PermissionReviewRig {
        self.rig
            .as_ref()
            .expect("Given step must prepare the published exact build")
    }
}

#[given("a verified published manifest with no signed requires tags")]
fn given_verified_manifest_without_requires(world: &mut RuntimeFfiWorld) {
    let rig = PermissionReviewRig::new();
    assert!(
        rig.has_no_signed_requirements(),
        "the immutable manifest must not gain synthetic requires tags"
    );
    world.rig = Some(rig);
}

#[given("its hash-matching entry document declares bounded napplet requirements")]
fn given_hash_matching_entry_requirements(world: &mut RuntimeFfiWorld) {
    assert_eq!(world.rig().embedded_domains().len(), 6);
}

#[given(expr = "host policy manages the {string} permission")]
fn given_host_policy_manages(world: &mut RuntimeFfiWorld, domain: String) {
    world.rig().set_host_policy(&domain);
}

#[given("the caller has opened the current permission review")]
fn given_current_review_open(world: &mut RuntimeFfiWorld) {
    world.review = Some(world.rig().permission_review());
}

#[when("the exact build is requested through the runtime FFI permission facade")]
fn when_permission_review_requested(world: &mut RuntimeFfiWorld) {
    world.review = Some(world.rig().permission_review());
}

#[when(expr = "the caller allows only {string} against the current review")]
fn when_caller_allows_current_domain(world: &mut RuntimeFfiWorld, domain: String) {
    let review = world.rig().permission_review();
    world.update = Some(world.rig().apply_changes(
        review.revision,
        vec![RuntimePermissionDecisionSelection {
            domain,
            decision: RuntimeGrantDecision::AllowExactBuild,
        }],
    ));
}

#[when(expr = "host policy takes over {string} before the caller allows {string}")]
fn when_policy_changes_before_apply(
    world: &mut RuntimeFfiWorld,
    managed_domain: String,
    selected_domain: String,
) {
    let revision = world
        .review
        .as_ref()
        .expect("Given step must open the review")
        .revision
        .clone();
    world.rig().set_host_policy(&managed_domain);
    world.update = Some(world.rig().apply_changes(
        revision,
        vec![RuntimePermissionDecisionSelection {
            domain: selected_domain,
            decision: RuntimeGrantDecision::AllowExactBuild,
        }],
    ));
}

#[then("the changed-domain permission update is applied")]
fn then_changed_domain_update_applied(world: &mut RuntimeFfiWorld) {
    let update = world
        .update
        .as_ref()
        .expect("When step must apply permission changes");
    assert!(update.applied);
    assert!(update.changed);
    assert!(update.refusal.is_none());
}

#[then(expr = "the {string} permission remains controlled by host policy")]
fn then_permission_remains_managed(world: &mut RuntimeFfiWorld, domain: String) {
    let review = world.rig().permission_review();
    let capability = review
        .capabilities
        .iter()
        .find(|capability| capability.domain == domain)
        .expect("reviewed capability");
    assert_eq!(
        capability.controller,
        RuntimePermissionDecisionController::HostPolicy {
            reason: "this capability is managed by host policy".to_owned()
        }
    );
    assert_eq!(
        capability.existing_decision,
        RuntimePermissionExistingDecision::Managed
    );
}

#[then(expr = "the {string} permission is allowed for the exact build")]
fn then_permission_allowed(world: &mut RuntimeFfiWorld, domain: String) {
    let review = world.rig().permission_review();
    assert_eq!(
        review
            .capabilities
            .iter()
            .find(|capability| capability.domain == domain)
            .expect("reviewed capability")
            .existing_decision,
        RuntimePermissionExistingDecision::AllowExactBuild
    );
}

#[then("the FFI returns a typed stale-review refusal with the current review")]
fn then_stale_refusal_has_current_review(world: &mut RuntimeFfiWorld) {
    let update = world
        .update
        .as_ref()
        .expect("When step must apply permission changes");
    assert!(!update.applied);
    assert!(!update.changed);
    assert_eq!(
        update.refusal.as_ref().expect("typed refusal").code,
        RuntimePermissionChangeRefusalCode::StaleReview
    );
    assert!(update.review.is_some());
}

#[then(expr = "the {string} permission remains denied")]
fn then_permission_remains_denied(world: &mut RuntimeFfiWorld, domain: String) {
    let review = world.rig().permission_review();
    assert_eq!(
        review
            .capabilities
            .iter()
            .find(|capability| capability.domain == domain)
            .expect("reviewed capability")
            .existing_decision,
        RuntimePermissionExistingDecision::Denied
    );
}

#[then("the review contains exactly the authenticated normalized domains")]
fn then_review_contains_authenticated_domains(world: &mut RuntimeFfiWorld) {
    let mut actual = world
        .review
        .as_ref()
        .expect("When step must request a permission review")
        .capabilities
        .iter()
        .map(|capability| capability.domain.clone())
        .collect::<Vec<_>>();
    actual.sort();
    let mut expected = world.rig().embedded_domains().to_vec();
    expected.sort();
    assert_eq!(actual, expected);
}

#[then("the review principal is bound to manifest author, dTag, and aggregateHash")]
fn then_review_principal_is_exact(world: &mut RuntimeFfiWorld) {
    assert_eq!(
        &world
            .review
            .as_ref()
            .expect("When step must request a permission review")
            .coordinate,
        world.rig().coordinate()
    );
}

#[when("launch is attempted without granting the required domains")]
fn when_launch_attempted_without_grants(world: &mut RuntimeFfiWorld) {
    world.rig().launch_without_grants();
    world.snapshot = Some(world.rig().snapshot());
}

#[then("no session crosses the runtime FFI boundary")]
fn then_no_session_crosses_ffi(world: &mut RuntimeFfiWorld) {
    assert!(
        world
            .snapshot
            .as_ref()
            .expect("When step must attempt launch")
            .sessions
            .is_empty()
    );
}

#[then("the exact build receives typed bridge refusal evidence")]
fn then_exact_build_receives_refusal(world: &mut RuntimeFfiWorld) {
    let refusal = world
        .snapshot
        .as_ref()
        .expect("When step must attempt launch")
        .recent_errors
        .last()
        .expect("launch refusal evidence");
    let coordinate = world.rig().coordinate();
    assert_eq!(refusal.code, "bridge");
    assert_eq!(
        refusal.author.as_deref(),
        Some(coordinate.manifest_author.as_str())
    );
    assert_eq!(refusal.d_tag.as_deref(), Some(coordinate.d_tag.as_str()));
    assert_eq!(
        refusal.aggregate_hash.as_deref(),
        Some(coordinate.aggregate_hash.as_str())
    );
}

#[tokio::main]
async fn main() {
    RuntimeFfiWorld::run("tests/features").await;
}
