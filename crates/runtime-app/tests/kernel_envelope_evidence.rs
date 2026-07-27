//! An ignored envelope names what was sent, and the bound on that name holds
//! against input the napplet controls.
//!
//! `DispatchOutcome::IgnoredUnknown` produces no reply and no refusal. Before
//! this, the resulting `EnvelopeIgnored` carried only a session id, so a
//! napplet whose call vanished -- `link.open`, say, which routes correctly and
//! is then ignored for want of a provider -- left nothing that named it.

mod support;

use std::sync::Arc;

use nmp_native_runtime_app::{PlatformCommand, PlatformEvent};
use support::*;

fn ignored_types(rig: &Rig) -> Vec<Option<String>> {
    rig.app
        .events_after(0)
        .events
        .into_iter()
        .filter_map(|item| match item.event {
            PlatformEvent::EnvelopeIgnored { message_type, .. } => Some(message_type),
            _ => None,
        })
        .collect()
}

fn send(rig: &Rig, session: nmp_native_runtime_core::SessionId, bytes: &[u8]) {
    rig.app.dispatch(PlatformCommand::MappedEnvelope {
        session,
        bytes: Arc::from(bytes),
    });
}

fn running_session(rig: &Rig) -> nmp_native_runtime_core::SessionId {
    let principal = principal('a');
    rig.install(principal.clone());
    rig.allow_runtime(principal.clone());
    let session = rig.launch(principal);
    rig.ready(session);
    session
}

#[test]
fn an_ignored_envelope_names_the_type_the_napplet_sent() {
    let rig = Rig::new(false);
    let session = running_session(&rig);

    send(&rig, session, br#"{"type":"link.open"}"#);

    assert!(
        ignored_types(&rig).contains(&Some("link.open".to_owned())),
        "the one field a napplet needs back is the call it made"
    );
}

/// A napplet sending `type: "<malformed-json>"` must not be reported the same
/// way as an envelope that genuinely was malformed.
#[test]
fn nothing_to_read_is_absent_rather_than_a_string() {
    let rig = Rig::new(false);
    let session = running_session(&rig);

    send(&rig, session, b"{ not json");
    send(&rig, session, br#"{"type":42}"#);
    send(&rig, session, br#"{"type":"<malformed-json>"}"#);

    let seen = ignored_types(&rig);
    // Established by running it: genuinely malformed input never reaches
    // `IgnoredUnknown` at all -- the bridge refuses it first -- so the only
    // envelopes ignored here are well-formed ones with a string type.
    assert_eq!(
        seen,
        vec![Some("<malformed-json>".to_owned())],
        "the sole ignored envelope is the one whose napplet chose that string"
    );
}

/// The bound is on bytes, and a napplet picks the characters.
///
/// Taking 128 *chars* does not bound 128 *bytes*: four-byte characters give
/// four bytes each, so the recorded value overran the documented maximum by
/// 4x on input the napplet chooses.
#[test]
fn the_recorded_type_stays_inside_its_byte_bound_for_multibyte_input() {
    let rig = Rig::new(false);
    let session = running_session(&rig);
    let hostile = "\u{1F600}".repeat(400);

    send(
        &rig,
        session,
        format!(r#"{{"type":"{hostile}"}}"#).as_bytes(),
    );

    let recorded = ignored_types(&rig)
        .into_iter()
        .flatten()
        .find(|value| value.starts_with('\u{1F600}'))
        .expect("the hostile envelope was ignored and recorded");

    assert!(
        recorded.len() <= 128,
        "recorded {} bytes against a 128-byte bound",
        recorded.len()
    );
    assert!(recorded.ends_with('…'), "truncation stays visible");
}

/// The same bound governs the activity fact, which is the other consumer.
#[test]
fn the_unroutable_activity_fact_is_bounded_too() {
    let rig = Rig::new(false);
    let session = running_session(&rig);
    let hostile = "\u{1F600}".repeat(400);

    send(
        &rig,
        session,
        format!(r#"{{"type":"{hostile}"}}"#).as_bytes(),
    );

    let snapshot = rig.app.snapshot();
    let fact = snapshot
        .recent_activity
        .iter()
        .find(|fact| &*fact.operation == "route")
        .expect("an unroutable envelope records a fact");
    let details = fact.details();
    for detail in details {
        if let nmp_native_runtime_app::ActivityDetailValue::Visible(value) = detail.value() {
            assert!(
                value.len() <= 128,
                "activity detail recorded {} bytes against a 128-byte bound",
                value.len()
            );
        }
    }
}
