use cucumber::{given, then, when};
use nmp_native_runtime_app::ReceiptDeliveryState;
use nmp_native_runtime_ffi::{RuntimeReceiptObservationLifecycle, RuntimeReceiptOutcome};

use super::RuntimeFfiWorld;
use crate::support::ReceiptProjectionRig;

#[given(regex = "^a canonical \"([^\"]+)\" durable receipt state$")]
fn given_canonical_receipt(world: &mut RuntimeFfiWorld, fixture: String) {
    world.receipt_rig = Some(ReceiptProjectionRig::named(&fixture));
}

#[given(regex = "^a \"([^\"]+)\" durable receipt projection$")]
fn given_receipt_projection(world: &mut RuntimeFfiWorld, fixture: String) {
    world.receipt_rig = Some(ReceiptProjectionRig::named(&fixture));
}

#[when("the runtime FFI projects the receipt while observation is active")]
fn when_receipt_is_projected(world: &mut RuntimeFfiWorld) {
    world.receipt = Some(
        world
            .receipt_rig
            .as_ref()
            .expect("Given step must select a receipt fixture")
            .project(ReceiptDeliveryState::Observing),
    );
}

#[when("the native observation closes after receiving that state")]
fn when_observation_closes(world: &mut RuntimeFfiWorld) {
    world.receipt = Some(
        world
            .receipt_rig
            .as_ref()
            .expect("Given step must select a receipt fixture")
            .close_after_latest(),
    );
}

#[when("the same canonical state is replayed after receipt reattachment")]
fn when_receipt_is_reattached(world: &mut RuntimeFfiWorld) {
    let (before, after) = world
        .receipt_rig
        .as_ref()
        .expect("Given step must select a receipt fixture")
        .reattach();
    world.prior_receipt = Some(before);
    world.receipt = Some(after);
}

#[then(regex = "^the durable outcome is \"([^\"]+)\"$")]
fn then_durable_outcome(world: &mut RuntimeFfiWorld, expected: String) {
    assert_eq!(
        world
            .receipt
            .as_ref()
            .expect("When step must project the receipt")
            .outcome,
        outcome(&expected)
    );
}

#[then(regex = "^the observation lifecycle is \"([^\"]+)\"$")]
fn then_observation_lifecycle(world: &mut RuntimeFfiWorld, expected: String) {
    assert_eq!(
        world
            .receipt
            .as_ref()
            .expect("When step must project the receipt")
            .observation_lifecycle,
        lifecycle(&expected)
    );
}

#[then("the exact canonical state remains available as raw evidence")]
fn then_raw_state_is_preserved(world: &mut RuntimeFfiWorld) {
    assert_eq!(
        world
            .receipt
            .as_ref()
            .expect("When step must project the receipt")
            .latest_state_json
            .as_deref(),
        world
            .receipt_rig
            .as_ref()
            .expect("Given step must select a receipt fixture")
            .raw()
    );
}

#[then("reattachment preserves the last durable outcome and evidence")]
fn then_reattachment_preserves_outcome(world: &mut RuntimeFfiWorld) {
    let before = world
        .prior_receipt
        .as_ref()
        .expect("When step must retain the pre-restart receipt");
    let after = world
        .receipt
        .as_ref()
        .expect("When step must project the reattached receipt");
    assert_eq!(after.outcome, before.outcome);
    assert_eq!(after.outcome_detail, before.outcome_detail);
    assert_eq!(after.latest_state_json, before.latest_state_json);
}

fn outcome(name: &str) -> RuntimeReceiptOutcome {
    match name {
        "in-progress" => RuntimeReceiptOutcome::InProgress,
        "delivered" => RuntimeReceiptOutcome::Delivered,
        "partial" => RuntimeReceiptOutcome::PartialDelivery,
        "exhausted" => RuntimeReceiptOutcome::Exhausted,
        "ambiguous" => RuntimeReceiptOutcome::Ambiguous,
        "refused" => RuntimeReceiptOutcome::Refused,
        "failed" => RuntimeReceiptOutcome::Failed,
        "cancelled" => RuntimeReceiptOutcome::Cancelled,
        "conflict" => RuntimeReceiptOutcome::Conflict,
        "unavailable" => RuntimeReceiptOutcome::Unavailable,
        other => panic!("unknown expected receipt outcome {other:?}"),
    }
}

fn lifecycle(name: &str) -> RuntimeReceiptObservationLifecycle {
    match name {
        "observing" => RuntimeReceiptObservationLifecycle::Observing,
        "not-found" => RuntimeReceiptObservationLifecycle::NotFound,
        "closed" => RuntimeReceiptObservationLifecycle::Closed,
        other => panic!("unknown expected observation lifecycle {other:?}"),
    }
}
