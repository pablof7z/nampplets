//! Steps for `features/boundary_refusal_visibility.feature`.
//!
//! These drive a real controller and read the same snapshot projection a
//! native consumer reads, so "the consumer can see it" means the projected
//! revision, not an internal counter.

use cucumber::{given, then, when};
use nmp_native_runtime_ffi::{
    RuntimeConfig, RuntimeController, RuntimeSnapshotProjection, RuntimeWorkspaceAxis,
    RuntimeWorkspaceDefinition,
};
use std::sync::Arc;
use tempfile::TempDir;

use crate::RuntimeFfiWorld;
use crate::support::NoArtifactSource;

#[derive(Debug, Default)]
pub struct BoundaryRefusals {
    pub temp: Option<Arc<TempDir>>,
    pub controller: Option<Arc<RuntimeController>>,
    /// The revision the consumer last drew at.
    pub observed_revision: u64,
}

fn snapshot(controller: &RuntimeController) -> nmp_native_runtime_ffi::RuntimeSnapshot {
    match controller.snapshot() {
        RuntimeSnapshotProjection::Snapshot { snapshot } => snapshot,
        RuntimeSnapshotProjection::Refused { refusal, .. } => {
            panic!("these scenarios never produce a refused snapshot: {refusal:?}")
        }
    }
}

#[given("a consumer has observed the runtime at its current revision")]
fn given_observed(world: &mut RuntimeFfiWorld) {
    let temp = Arc::new(TempDir::new().expect("a temporary store directory"));
    let controller = RuntimeController::open(
        RuntimeConfig {
            runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
            nmp_store_path: None,
            artifact_cache_path: temp.path().join("artifacts").display().to_string(),
            ..RuntimeConfig::default()
        },
        Box::new(NoArtifactSource),
    )
    .expect("the runtime opens");
    world.refusals.observed_revision = snapshot(&controller).revision;
    world.refusals.temp = Some(temp);
    world.refusals.controller = Some(controller);
}

/// Drives a real refusal through the public facade rather than a test-only
/// hook: an empty workspace id is refused by `workspace_record_from_ffi`, and
/// that path records a boundary refusal exactly as any other would.
fn record(world: &mut RuntimeFfiWorld, count: usize) {
    let controller = world
        .refusals
        .controller
        .clone()
        .expect("the runtime opened");
    for _ in 0..count {
        let update = controller.save_workspace(RuntimeWorkspaceDefinition {
            schema_version: 1,
            workspace_id: String::new(),
            axis: RuntimeWorkspaceAxis::Horizontal,
            slots: Vec::new(),
            focused_slot_id: None,
            activity_drawer_visible: false,
            preferences_json: String::new(),
            retained_receipt_ids: Vec::new(),
        });
        assert!(!update.accepted, "the malformed workspace must be refused");
        assert!(update.refusal.is_some(), "the refusal must be reported");
    }
}

#[when("the runtime records a boundary refusal")]
fn when_one_refusal(world: &mut RuntimeFfiWorld) {
    record(world, 1);
}

#[when(regex = r"^the runtime records (\d+) boundary refusals$")]
fn when_n_refusals(world: &mut RuntimeFfiWorld, count: usize) {
    record(world, count);
}

fn current(world: &RuntimeFfiWorld) -> nmp_native_runtime_ffi::RuntimeSnapshot {
    snapshot(
        world
            .refusals
            .controller
            .as_ref()
            .expect("the runtime opened"),
    )
}

#[then("the revision the consumer gates on has moved")]
fn then_revision_moved(world: &mut RuntimeFfiWorld) {
    let now = current(world).revision;
    assert!(
        now > world.refusals.observed_revision,
        "revision did not move: still {now} after a boundary refusal"
    );
}

#[then("the refusal is present in the snapshot at that revision")]
fn then_refusal_present(world: &mut RuntimeFfiWorld) {
    let snapshot = current(world);
    assert!(
        snapshot
            .boundary_refusals
            .iter()
            .any(|refusal| refusal.code == "invalid-workspace"),
        "the refusal is absent from the snapshot whose revision announced it"
    );
}

/// The consumer's real loop: redraw only when the revision moves.
#[then("a consumer redrawing only on a revision change still sees the refusal")]
fn then_gated_consumer_sees_it(world: &mut RuntimeFfiWorld) {
    let snapshot = current(world);
    let redrew = snapshot.revision > world.refusals.observed_revision;
    assert!(
        redrew,
        "a revision-gated consumer would have skipped this frame"
    );
    assert!(
        snapshot
            .boundary_refusals
            .iter()
            .any(|refusal| refusal.code == "invalid-workspace"),
        "the consumer redrew but the refusal was not there to read"
    );
}

#[then(regex = r"^the revision has moved at least (\d+) times$")]
fn then_revision_moved_n(world: &mut RuntimeFfiWorld, expected: u64) {
    let now = current(world).revision;
    assert!(
        now >= world.refusals.observed_revision + expected,
        "revision moved from {} to {now}; expected at least {expected} steps",
        world.refusals.observed_revision
    );
}

#[then(regex = r"^the snapshot carries (\d+) refusals$")]
fn then_snapshot_carries_n(world: &mut RuntimeFfiWorld, expected: usize) {
    let snapshot = current(world);
    let seen = snapshot
        .boundary_refusals
        .iter()
        .filter(|refusal| refusal.code == "invalid-workspace")
        .count();
    assert_eq!(seen, expected);
}
