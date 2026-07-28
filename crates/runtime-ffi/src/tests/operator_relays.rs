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

fn relays(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn open_with(temp: &TempDir, indexer: &[&str], app: &[&str]) -> Arc<RuntimeController> {
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
    .unwrap()
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
    assert_eq!(controller.snapshot_value().boundary_refusals.len(), 1);
    assert_eq!(refusals(&controller).len(), 1);
}
