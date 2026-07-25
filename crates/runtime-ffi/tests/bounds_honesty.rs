//! Bounds honesty at the FFI boundary: the boundary-refusal ring reports the
//! exact number of refusals it dropped, so a consumer never mistakes the
//! retained tail for the whole history.
//!
//! All three refusal recorders (`workspace_refusal`, `provider_refusal`,
//! `record_refusal`) share one bounded-append path, so they share one counter.

use nmp_native_runtime_ffi::{
    ArtifactFetchRequest, ArtifactFetchResponse, ArtifactSource, NativeAppearanceSnapshot,
    RuntimeConfig, RuntimeController, RuntimeExactBuildCoordinate,
};
use std::sync::Arc;
use tempfile::TempDir;

struct EmptySource;

impl ArtifactSource for EmptySource {
    fn fetch(&self, request: ArtifactFetchRequest) -> ArtifactFetchResponse {
        ArtifactFetchResponse::Body {
            source_url: request.candidate_urls.first().cloned().unwrap_or_default(),
            http_status: 404,
            bytes: Vec::new(),
        }
    }
}

fn controller(temp: &TempDir, maximum_boundary_events: u64) -> Arc<RuntimeController> {
    RuntimeController::open(
        RuntimeConfig {
            runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
            nmp_store_path: None,
            artifact_cache_path: temp.path().join("artifacts").display().to_string(),
            maximum_boundary_events,
            ..RuntimeConfig::default()
        },
        Box::new(EmptySource),
    )
    .unwrap()
}

fn invalid_coordinate() -> RuntimeExactBuildCoordinate {
    RuntimeExactBuildCoordinate {
        manifest_author: "NOT-HEX".to_owned(),
        d_tag: "good-morning".to_owned(),
        aggregate_hash: "also-not-hex".to_owned(),
    }
}

/// Drives one refusal through each of the three recorders, so the round is
/// three refusals wide and covers every call site that used to drop silently.
fn refusal_round(controller: &Arc<RuntimeController>) {
    // `record_refusal` via an unparseable exact-build coordinate.
    controller.uninstall_build(invalid_coordinate());
    // `workspace_refusal` via an invalid workspace identifier.
    controller.assign_build_to_workspace("\n".to_owned(), invalid_coordinate());
    // `provider_refusal` via an appearance push with no registered source.
    controller.update_appearance(NativeAppearanceSnapshot {
        dark: true,
        increased_contrast: false,
        reduced_transparency: false,
        accent_red: 88,
        accent_green: 166,
        accent_blue: 255,
    });
}

#[test]
fn an_unfilled_refusal_ring_is_a_complete_answer() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp, 256);
    refusal_round(&controller);

    let snapshot = controller.snapshot();
    assert_eq!(snapshot.boundary_refusals.len(), 3);
    assert_eq!(snapshot.dropped_boundary_refusals, 0);
    controller.close();
}

#[test]
fn refusal_overflow_reports_the_exact_dropped_count() {
    const ROUNDS: u64 = 7;
    const CAP: u64 = 4;

    let temp = TempDir::new().unwrap();
    let controller = controller(&temp, CAP);
    for _ in 0..ROUNDS {
        refusal_round(&controller);
    }

    let snapshot = controller.snapshot();
    assert_eq!(snapshot.boundary_refusals.len() as u64, CAP);
    assert_eq!(snapshot.dropped_boundary_refusals, ROUNDS * 3 - CAP);
    // The retained tail is the newest round, in recording order.
    let codes: Vec<&str> = snapshot
        .boundary_refusals
        .iter()
        .map(|refusal| refusal.code.as_str())
        .collect();
    assert_eq!(
        codes,
        vec![
            "theme-unavailable",
            "invalid-exact-build-coordinate",
            "invalid-workspace-assignment",
            "theme-unavailable",
        ]
    );
    controller.close();
}

#[test]
fn the_snapshot_carries_the_app_side_dropped_counts_across_the_boundary() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp, 256);
    let snapshot = controller.snapshot();
    assert_eq!(snapshot.dropped_activity, 0);
    assert_eq!(snapshot.dropped_errors, 0);
    assert_eq!(snapshot.dropped_boundary_refusals, 0);
    controller.close();
}
