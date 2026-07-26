//! A session that launches without a domain its own content requires must say
//! so, in data, for as long as it runs.
//!
//! Launching anyway is deliberate rather than lax: `compatibility.lock` records
//! `supported_domains = []` for macOS, iOS and Android with all 22 domains
//! unsupported, so refusing on a missing required domain would refuse most real
//! napplets. The defect was never that the launch proceeds. It was that it
//! proceeded while reporting nothing had happened -- the shortfall existed only
//! as a comma-joined string inside one activity fact, and the session reported
//! `Running` as though it were whole.

mod support;

use std::collections::BTreeSet;

use nmp_native_runtime_app::{PlatformCommand, SessionDomainView};
use nmp_native_runtime_core::{Capability, ExecutionProfile};
use support::*;

/// `nip29-groups` is the build named in `intent_dispatch.rs` as the victim of
/// this path: its domains come from the `napplet-requires` meta in its verified
/// `/index.html`, and `lists` is among them. Nothing on this runtime serves
/// `lists`, so it is required, known, verified -- and unavailable.
fn lists() -> Capability {
    Capability::new("lists").unwrap()
}

fn launch_requiring(rig: &Rig, extra: BTreeSet<Capability>) -> nmp_native_runtime_core::Principal {
    let principal = principal('d');
    rig.install(principal.clone());
    rig.allow_runtime(principal.clone());
    let mut required = BTreeSet::from([canary()]);
    required.extend(extra);
    rig.app.dispatch(PlatformCommand::Launch {
        principal: principal.clone(),
        profile: ExecutionProfile::Legacy,
        required_domains: required,
    });
    principal
}

#[test]
fn a_required_domain_no_provider_serves_is_named_on_the_session() {
    let rig = Rig::new(false);
    launch_requiring(&rig, BTreeSet::from([lists()]));

    let snapshot = rig.app.snapshot();
    let session = snapshot.sessions.last().unwrap();
    let view = snapshot
        .session_domains
        .iter()
        .find(|view| view.session == session.id)
        .expect("the launched session must have a domain view");

    // The shortfall is a set the caller can act on, not prose to parse.
    assert_eq!(view.unavailable_domains, vec![lists()]);
    // And it is genuinely absent from what was negotiated, so a consumer
    // comparing the two sets sees the same gap the runtime saw.
    assert!(!view.domains.contains(&lists()));
}

/// The honest shape of a whole launch: an empty shortfall, not a missing one.
/// A consumer must not have to distinguish "no gap" from "nobody said".
#[test]
fn a_launch_with_every_domain_available_reports_an_empty_shortfall() {
    let rig = Rig::new(false);
    launch_requiring(&rig, BTreeSet::new());

    let snapshot = rig.app.snapshot();
    let session = snapshot.sessions.last().unwrap();
    let view = snapshot
        .session_domains
        .iter()
        .find(|view| view.session == session.id)
        .expect("the launched session must have a domain view");

    assert!(view.unavailable_domains.is_empty());
    assert!(view.domains.contains(&canary()));
}

/// One detail per domain. The previous fact carried
/// `"lists,upload"` in a single field, which no consumer could read without
/// splitting a string back apart, so in practice none did.
#[test]
fn the_shortfall_is_recorded_as_one_detail_per_domain() {
    let rig = Rig::new(false);
    let upload = Capability::new("upload").unwrap();
    launch_requiring(&rig, BTreeSet::from([lists(), upload.clone()]));

    let snapshot = rig.app.snapshot();
    let fact = snapshot
        .recent_activity
        .iter()
        .find(|fact| &*fact.operation == "required-domain-unavailable")
        .expect("an unavailable required domain must record a fact");

    let named = fact
        .details()
        .iter()
        .map(|detail| match detail.value() {
            nmp_native_runtime_app::ActivityDetailValue::Visible(value) => value.to_string(),
            other => panic!("the domain name is not a secret: {other:?}"),
        })
        .collect::<BTreeSet<_>>();

    assert_eq!(
        named,
        BTreeSet::from(["lists".to_owned(), "upload".to_owned()])
    );
    assert_eq!(fact.dropped_detail_count(), 0);
}

/// The shortfall survives for the life of the session, not just the launch
/// instant. A drawer opened a minute later has to be able to answer the same
/// question the launch could.
#[test]
fn the_shortfall_is_still_readable_after_the_session_is_ready() {
    let rig = Rig::new(false);
    launch_requiring(&rig, BTreeSet::from([lists()]));
    let session = rig.app.snapshot().sessions.last().unwrap().id;
    rig.ready(session);

    let snapshot = rig.app.snapshot();
    let view = snapshot
        .session_domains
        .iter()
        .find(|view| view.session == session)
        .expect("the session must still have a domain view");
    assert_eq!(view.unavailable_domains, vec![lists()]);
}

/// Guards the shape rather than a message: the view is a pair of sets, so
/// nothing downstream has to reconstruct the shortfall from text.
#[test]
fn the_domain_view_carries_both_sets_side_by_side() {
    let rig = Rig::new(false);
    launch_requiring(&rig, BTreeSet::from([lists()]));

    let snapshot = rig.app.snapshot();
    let session = snapshot.sessions.last().unwrap().id;
    assert_eq!(
        snapshot
            .session_domains
            .iter()
            .find(|view| view.session == session)
            .cloned(),
        Some(SessionDomainView {
            session,
            domains: vec![canary(), shell()],
            unavailable_domains: vec![lists()],
        })
    );
}
