//! Operator relay lanes are judged by the runtime, not by the host that ships
//! them.
//!
//! These lanes arrive from the application bundle, so a mistyped relay cannot
//! be corrected at runtime. Every host used to filter them itself, which put
//! the scheme, credential and duplicate rules in the shell -- where a second
//! host has to reproduce them exactly or route differently -- and dropped what
//! failed without saying so.

use tempfile::TempDir;

use super::*;
use crate::RuntimeOpenError;

fn relays(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn try_open(
    temp: &TempDir,
    indexer: &[&str],
    app: &[&str],
) -> Result<Arc<RuntimeController>, RuntimeOpenError> {
    RuntimeController::open(
        RuntimeConfig {
            runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
            nmp_store_path: None,
            artifact_cache_path: temp.path().join("artifacts").display().to_string(),
            indexer_relays: relays(indexer),
            app_relays: relays(app),
            ..RuntimeConfig::default()
        },
        Box::new(FixtureSource(BTreeMap::new())),
    )
}

fn open_with(temp: &TempDir, indexer: &[&str], app: &[&str]) -> Arc<RuntimeController> {
    try_open(temp, indexer, app).unwrap()
}

fn refusals(controller: &Arc<RuntimeController>) -> Vec<String> {
    controller
        .snapshot_value()
        .boundary_refusals
        .into_iter()
        .filter(|refusal| refusal.code == "operator-relay-refused")
        .map(|refusal| refusal.detail)
        .collect()
}

#[test]
fn a_usable_operator_lane_opens_with_no_refusal() {
    let temp = TempDir::new().unwrap();
    let controller = open_with(&temp, &["wss://indexer.example"], &["wss://app.example"]);

    assert!(refusals(&controller).is_empty());
}

/// The behaviour this replaces: the host filtered these out and the runtime
/// never knew they had been configured.
#[test]
fn an_insecure_operator_relay_is_dropped_and_named() {
    let temp = TempDir::new().unwrap();
    let controller = open_with(
        &temp,
        &["ws://plaintext.example", "wss://indexer.example"],
        &["wss://app.example"],
    );

    let refusals = refusals(&controller);
    assert_eq!(refusals.len(), 1);
    assert!(refusals[0].contains("ws://plaintext.example"));
    assert!(refusals[0].contains("wss://"));
    assert!(refusals[0].starts_with("indexer relay"));
}

#[test]
fn an_operator_relay_carrying_credentials_is_dropped_and_named() {
    let temp = TempDir::new().unwrap();
    let controller = open_with(
        &temp,
        &["wss://indexer.example"],
        &["wss://user:secret@app.example", "wss://app.example"],
    );

    let refusals = refusals(&controller);
    assert_eq!(refusals.len(), 1);
    assert!(refusals[0].starts_with("app relay"));
    assert!(refusals[0].contains("credentials"));
}

#[test]
fn a_repeated_operator_relay_is_admitted_once_and_the_repeat_is_named() {
    let temp = TempDir::new().unwrap();
    let controller = open_with(
        &temp,
        &["wss://indexer.example", "wss://indexer.example"],
        &["wss://app.example"],
    );

    let refusals = refusals(&controller);
    assert_eq!(refusals.len(), 1);
    assert!(refusals[0].contains("already in this lane"));
}

/// One mistyped relay in a shipped bundle must not stop the runtime opening
/// for everybody running that build.
#[test]
fn a_partly_unusable_lane_still_opens_on_what_is_left() {
    let temp = TempDir::new().unwrap();
    let controller = open_with(
        &temp,
        &["ws://wrong.example", "wss://right.example"],
        &["wss://app.example"],
    );

    // Opening at all is the assertion: a partly unusable lane degrades
    // instead of taking the runtime down.
    assert_eq!(refusals(&controller).len(), 1);
}

/// Degrading a lane is survivable; emptying it is not. A runtime routing
/// through no relays at all, while every other signal reads healthy, is the
/// failure this whole change exists to stop.
#[test]
fn a_lane_whose_every_entry_is_refused_stops_the_runtime_opening() {
    let temp = TempDir::new().unwrap();
    let error = try_open(
        &temp,
        &["ws://one.example", "wss://user:pass@two.example"],
        &["wss://app.example"],
    )
    .unwrap_err();

    let RuntimeOpenError::InvalidConfig { detail } = error else {
        panic!("an emptied lane refuses the open as invalid configuration");
    };
    assert!(detail.contains("indexer"));
    assert!(detail.contains("ws://one.example"), "{detail}");
    assert!(detail.contains("credentials"), "{detail}");
}

/// A lane nobody configured is not an emptied lane.
#[test]
fn an_unconfigured_lane_is_not_treated_as_emptied() {
    let temp = TempDir::new().unwrap();
    assert!(try_open(&temp, &[], &[]).is_ok());
}

/// Whitespace reaches the runtime now that the shell stopped trimming it, so
/// the runtime has to name it rather than let it vanish.
#[test]
fn a_whitespace_only_operator_relay_is_refused_by_name() {
    let temp = TempDir::new().unwrap();
    let controller = open_with(
        &temp,
        &["   ", "wss://indexer.example"],
        &["wss://app.example"],
    );

    let refusals = refusals(&controller);
    assert_eq!(refusals.len(), 1);
    assert!(refusals[0].starts_with("indexer relay"));
}

/// The ordinary refusal ring evicts. A relay the deployment configured and
/// this runtime would not admit stays true for the whole process, so it is
/// also retained where nothing can push it out.
#[test]
fn refused_operator_relays_survive_where_the_bounded_ring_would_evict() {
    let temp = TempDir::new().unwrap();
    let controller = open_with(
        &temp,
        &["ws://plaintext.example", "wss://indexer.example"],
        &["wss://app.example"],
    );

    let durable = controller.snapshot_value().refused_operator_relays;
    assert_eq!(durable.len(), 1);
    assert_eq!(durable[0].code, "operator-relay-refused");
    assert!(durable[0].detail.contains("ws://plaintext.example"));
}
