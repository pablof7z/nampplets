//! Bounds honesty: a bounded tail is never presented as a complete answer.
//!
//! Every bounded fact ring reports the exact number of facts it dropped, so a
//! consumer can always tell "these are all of them" from "these are the last
//! N of many". Each test runs the same workload twice — once with a cap large
//! enough that nothing is dropped, then with a small cap — and asserts the
//! dropped count equals exactly the difference.

mod support;

use std::{collections::BTreeSet, sync::Arc};

use nmp_native_runtime_app::{AppLimits, PlatformCommand, RuntimeApp};
use nmp_native_runtime_core::{ExecutionProfile, Principal};
use nmp_native_runtime_store::{InstalledBuild, RuntimeStore, StoreLimits};
use nmp_native_test_harness::FakeHostDataPlane;
use support::*;
use tempfile::TempDir;

struct Fixture {
    _directory: TempDir,
    app: Arc<RuntimeApp>,
}

fn open(limits: AppLimits) -> Fixture {
    let directory = TempDir::new().unwrap();
    let store = Arc::new(
        RuntimeStore::open(directory.path().join("runtime.db"), StoreLimits::default()).unwrap(),
    );
    let host = Arc::new(FakeHostDataPlane::new(16));
    let provider = Arc::new(CapturingProvider::new(false));
    let (app, _shell) = open_app_with_limits(store, host, provider, limits);
    Fixture {
        _directory: directory,
        app,
    }
}

fn numbered_principal(index: u32) -> Principal {
    Principal::new("a".repeat(64), "test-napplet", format!("{index:064x}")).unwrap()
}

/// Installs `count` distinct exact builds; each install records one activity
/// fact.
fn install_many(app: &Arc<RuntimeApp>, count: u32) {
    for index in 0..count {
        let principal = numbered_principal(index);
        app.dispatch(PlatformCommand::InstallVerified {
            build: InstalledBuild {
                principal: principal.clone(),
                title: Arc::from("Test napplet"),
                manifest_metadata: json(serde_json::json!({"kind": 34128})),
                capability_requests: Vec::new(),
            },
            artifact: Arc::new(TestArtifact {
                kind: 35_129,
                author: principal.manifest_author().to_owned(),
                d_tag: principal.d_tag().to_owned(),
                aggregate: principal.aggregate_hash().to_owned(),
            }),
        });
    }
}

/// Launches `count` builds that were never installed; each launch refuses and
/// records one error fact (and one platform event).
fn refuse_many(app: &Arc<RuntimeApp>, count: u32) {
    for index in 0..count {
        app.dispatch(PlatformCommand::Launch {
            principal: numbered_principal(index),
            profile: ExecutionProfile::Legacy,
            required_domains: BTreeSet::from([canary()]),
        });
    }
}

#[test]
fn activity_overflow_reports_the_exact_dropped_count() {
    const INSTALLS: u32 = 40;
    const CAP: usize = 8;

    let complete = open(AppLimits {
        maximum_activity_facts: 1_024,
        ..AppLimits::default()
    });
    install_many(&complete.app, INSTALLS);
    let complete = complete.app.snapshot();
    assert_eq!(
        complete.dropped_activity, 0,
        "an unfilled ring is a complete answer"
    );
    let total = complete.recent_activity.len();
    assert!(total > CAP, "the workload must overflow the small cap");

    let bounded = open(AppLimits {
        maximum_activity_facts: CAP,
        ..AppLimits::default()
    });
    install_many(&bounded.app, INSTALLS);
    let bounded = bounded.app.snapshot();
    assert_eq!(bounded.recent_activity.len(), CAP);
    assert_eq!(bounded.dropped_activity, (total - CAP) as u64);
    assert_eq!(
        bounded.recent_activity.as_slice(),
        &complete.recent_activity[total - CAP..],
        "the retained tail is the newest facts, and only the oldest were dropped"
    );
}

#[test]
fn error_overflow_reports_the_exact_dropped_count() {
    const REFUSALS: u32 = 20;
    const CAP: usize = 4;

    let complete = open(AppLimits::default());
    refuse_many(&complete.app, REFUSALS);
    let complete = complete.app.snapshot();
    assert_eq!(complete.dropped_errors, 0);
    let total = complete.recent_errors.len();
    assert_eq!(total, REFUSALS as usize);

    let bounded = open(AppLimits {
        maximum_error_facts: CAP,
        ..AppLimits::default()
    });
    refuse_many(&bounded.app, REFUSALS);
    let bounded = bounded.app.snapshot();
    assert_eq!(bounded.recent_errors.len(), CAP);
    assert_eq!(bounded.dropped_errors, (REFUSALS as usize - CAP) as u64);
}

#[test]
fn the_two_rings_count_their_own_drops_independently() {
    let fixture = open(AppLimits {
        maximum_activity_facts: 2,
        maximum_error_facts: 3,
        ..AppLimits::default()
    });
    install_many(&fixture.app, 5);
    refuse_many(&fixture.app, 10);
    let snapshot = fixture.app.snapshot();
    assert_eq!(snapshot.recent_activity.len(), 2);
    assert_eq!(snapshot.dropped_activity, 3);
    assert_eq!(snapshot.recent_errors.len(), 3);
    assert_eq!(snapshot.dropped_errors, 7);
}

#[test]
fn a_stale_event_cursor_reports_how_many_events_it_lost() {
    let fixture = open(AppLimits {
        maximum_platform_events: 4,
        ..AppLimits::default()
    });
    refuse_many(&fixture.app, 10);

    let live = fixture.app.events_after(9);
    assert!(!live.cursor_was_stale);
    assert_eq!(live.lost_before_batch, 0, "a live cursor lost nothing");

    let stale = fixture.app.events_after(0);
    assert!(stale.cursor_was_stale);
    assert_eq!(stale.oldest_available, 7);
    assert_eq!(stale.newest_available, 10);
    // Sequences 1..=6 were evicted before the cursor could read them.
    assert_eq!(stale.lost_before_batch, 6);
    assert_eq!(
        stale.lost_before_batch,
        stale.oldest_available - 1,
        "the count matches the documented consumer-side derivation"
    );
}
