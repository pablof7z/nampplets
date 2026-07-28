//! Steps for `features/operator_relay_lanes.feature`.
//!
//! These open a real controller against a temporary store, so the scenarios
//! exercise the same admission path a shipped bundle does rather than calling
//! the lane judge directly.

use cucumber::{given, then, when};
use nmp_native_runtime_ffi::{RuntimeConfig, RuntimeController, RuntimeOpenError};
use std::sync::Arc;
use tempfile::TempDir;

use crate::RuntimeFfiWorld;
use crate::support::{NoArtifactSource, relay_refusals};

#[derive(Debug, Default)]
pub struct OperatorRelayLanes {
    pub temp: Option<Arc<TempDir>>,
    pub indexer: Vec<String>,
    pub app: Vec<String>,
    pub controller: Option<Arc<RuntimeController>>,
    pub open_error: Option<RuntimeOpenError>,
}

fn lane(values: &str) -> Vec<String> {
    values
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[given(regex = r#"^the bundle configures indexer relays "([^"]*)"$"#)]
fn given_indexer_relays(world: &mut RuntimeFfiWorld, values: String) {
    world.relays.indexer = lane(&values);
}

#[given(regex = r#"^the bundle configures app relays "([^"]*)"$"#)]
fn given_app_relays(world: &mut RuntimeFfiWorld, values: String) {
    world.relays.app = lane(&values);
}

#[when("the runtime opens")]
fn when_runtime_opens(world: &mut RuntimeFfiWorld) {
    let temp = Arc::new(TempDir::new().expect("a temporary store directory"));
    let opened = RuntimeController::open(
        RuntimeConfig {
            runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
            nmp_store_path: None,
            artifact_cache_path: temp.path().join("artifacts").display().to_string(),
            indexer_relays: world.relays.indexer.clone(),
            app_relays: world.relays.app.clone(),
            ..RuntimeConfig::default()
        },
        Box::new(NoArtifactSource),
    );
    world.relays.temp = Some(temp);
    match opened {
        Ok(controller) => world.relays.controller = Some(controller),
        Err(error) => world.relays.open_error = Some(error),
    }
}

#[then("the runtime is open")]
fn then_runtime_is_open(world: &mut RuntimeFfiWorld) {
    assert!(
        world.relays.controller.is_some(),
        "expected the runtime to open, but it refused: {:?}",
        world.relays.open_error
    );
}

#[then(regex = r#"^the runtime refuses to open naming the emptied "([^"]+)" lane$"#)]
fn then_refuses_emptied_lane(world: &mut RuntimeFfiWorld, lane: String) {
    let Some(RuntimeOpenError::InvalidConfig { detail }) = &world.relays.open_error else {
        panic!(
            "expected an invalid-configuration refusal, got {:?}",
            world.relays.open_error
        );
    };
    assert!(detail.contains(&lane), "{detail}");
}

fn refusals(world: &RuntimeFfiWorld) -> Vec<String> {
    relay_refusals(
        world
            .relays
            .controller
            .as_ref()
            .expect("the runtime opened"),
    )
    .0
}

#[then("no operator relay is refused")]
fn then_no_relay_refused(world: &mut RuntimeFfiWorld) {
    assert!(refusals(world).is_empty());
}

#[then(regex = r"^exactly (\d+) operator relay is refused$")]
fn then_exact_refusal_count(world: &mut RuntimeFfiWorld, expected: usize) {
    assert_eq!(refusals(world).len(), expected);
}

#[then(regex = r#"^an operator relay refusal names "([^"]+)"$"#)]
fn then_refusal_names(world: &mut RuntimeFfiWorld, needle: String) {
    let refusals = refusals(world);
    assert!(
        refusals.iter().any(|detail| detail.contains(&needle)),
        "no refusal named {needle:?}; saw {refusals:?}"
    );
}

#[then(regex = r#"^the durable operator relay refusals name "([^"]+)"$"#)]
fn then_durable_refusal_names(world: &mut RuntimeFfiWorld, needle: String) {
    let durable = relay_refusals(
        world
            .relays
            .controller
            .as_ref()
            .expect("the runtime opened"),
    )
    .1;
    assert!(
        durable.iter().any(|detail| detail.contains(&needle)),
        "no durable refusal named {needle:?}; saw {durable:?}"
    );
}
