//! The runtime decides what is diagnostic, not the shell.
//!
//! A sandboxed napplet mirrors its own console output to the host as a
//! `debug.console` envelope. That message carries no NAP domain authority --
//! but *deciding* it carries none is a protocol-membership judgement, and this
//! crate owns those. Before this, the Apple shell compared the caller-supplied
//! `type` against a hardcoded literal and returned before Rust ever saw the
//! message, so untrusted webview content could keep a message out of the
//! runtime's sight by naming it well.
//!
//! Here the classification is the kernel's: a diagnostic envelope produces a
//! typed, bounded fact, is never dispatched to a provider, and is never
//! silently dropped.

mod support;

use std::sync::Arc;

use nmp_native_runtime_app::{NappletDiagnosticLevel, PlatformCommand, PlatformEvent};
use support::*;

fn diagnostics(rig: &Rig) -> Vec<(NappletDiagnosticLevel, String)> {
    rig.app
        .events_after(0)
        .events
        .into_iter()
        .filter_map(|item| match item.event {
            PlatformEvent::NappletDiagnostic { level, message, .. } => Some((level, message)),
            _ => None,
        })
        .collect()
}

fn ignored_count(rig: &Rig) -> usize {
    rig.app
        .events_after(0)
        .events
        .into_iter()
        .filter(|item| matches!(item.event, PlatformEvent::EnvelopeIgnored { .. }))
        .count()
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
fn a_console_envelope_becomes_one_typed_diagnostic_fact() {
    let rig = Rig::new(false);
    let session = running_session(&rig);

    send(
        &rig,
        session,
        br#"{"type":"debug.console","level":"warn","message":"intent payload missing"}"#,
    );

    assert_eq!(
        diagnostics(&rig),
        vec![(
            NappletDiagnosticLevel::Warn,
            "intent payload missing".to_owned()
        )]
    );
}

/// The shell used to swallow this envelope. The kernel must see it, and must
/// still not treat it as protocol traffic.
#[test]
fn a_diagnostic_is_not_reported_as_an_ignored_protocol_envelope() {
    let rig = Rig::new(false);
    let session = running_session(&rig);

    send(
        &rig,
        session,
        br#"{"type":"debug.console","level":"log","message":"hello"}"#,
    );

    assert_eq!(diagnostics(&rig).len(), 1);
    assert_eq!(
        ignored_count(&rig),
        0,
        "a diagnostic is classified, not merely unrecognised"
    );
}

/// The level is a closed set the runtime owns. A napplet naming its own level
/// must not widen it.
#[test]
fn an_unrecognised_level_is_normalised_rather_than_passed_through() {
    let rig = Rig::new(false);
    let session = running_session(&rig);

    send(
        &rig,
        session,
        br#"{"type":"debug.console","level":"catastrophe","message":"x"}"#,
    );

    assert_eq!(
        diagnostics(&rig),
        vec![(NappletDiagnosticLevel::Unknown, "x".to_owned())]
    );
}

/// The old shell path returned before recording anything when the parse
/// failed, so a malformed diagnostic left no trace at all.
#[test]
fn a_malformed_diagnostic_is_recorded_rather_than_dropped() {
    let rig = Rig::new(false);
    let session = running_session(&rig);

    send(&rig, session, br#"{"type":"debug.console"}"#);
    send(&rig, session, br#"{"type":"debug.console","message":42}"#);

    let snapshot = rig.app.snapshot();
    let facts = &snapshot.recent_activity;
    let refusals = facts
        .iter()
        .filter(|fact| {
            fact.category.as_ref() == "envelope" && fact.operation.as_ref() == "diagnostic"
        })
        .count();
    assert!(
        refusals >= 2,
        "a diagnostic that cannot be read still leaves a trace"
    );
    assert!(
        diagnostics(&rig).is_empty(),
        "an unreadable diagnostic must not be invented into a fact"
    );
}

#[test]
fn a_diagnostic_message_is_bounded_against_the_napplet() {
    let rig = Rig::new(false);
    let session = running_session(&rig);

    let huge = "a".repeat(64 * 1024);
    let envelope = format!(r#"{{"type":"debug.console","level":"error","message":"{huge}"}}"#);
    send(&rig, session, envelope.as_bytes());

    let facts = diagnostics(&rig);
    assert_eq!(facts.len(), 1);
    assert!(
        facts[0].1.len() < huge.len(),
        "the napplet does not choose how much of the event ring it occupies"
    );
}

/// `debug` is not a negotiated capability, so nothing about this envelope may
/// reach a provider.
#[test]
fn a_diagnostic_never_reaches_a_provider() {
    let rig = Rig::new(false);
    let session = running_session(&rig);
    let before = rig.provider.seen.lock().len();

    send(
        &rig,
        session,
        br#"{"type":"debug.console","level":"info","message":"x"}"#,
    );

    assert_eq!(rig.provider.seen.lock().len(), before);
}

/// The trusted shell caps its own console wrapper, but that wrapper runs
/// inside the sandboxed frame. A napplet posting the envelope directly never
/// meets it, so the kernel keeps the bound that actually holds.
#[test]
fn a_session_may_mirror_only_a_finite_number_of_diagnostics() {
    let rig = Rig::new(false);
    let session = running_session(&rig);

    for index in 0..600 {
        send(
            &rig,
            session,
            format!(r#"{{"type":"debug.console","level":"log","message":"{index}"}}"#).as_bytes(),
        );
    }

    assert_eq!(
        diagnostics(&rig).len(),
        500,
        "the napplet does not choose how much of the event ring it occupies"
    );
}

#[test]
fn reaching_the_diagnostic_budget_is_said_once_rather_than_going_quiet() {
    let rig = Rig::new(false);
    let session = running_session(&rig);

    for index in 0..600 {
        send(
            &rig,
            session,
            format!(r#"{{"type":"debug.console","level":"log","message":"{index}"}}"#).as_bytes(),
        );
    }

    let snapshot = rig.app.snapshot();
    let exhausted = snapshot
        .recent_activity
        .iter()
        .filter(|fact| fact.outcome.as_ref() == "budget-exhausted")
        .count();
    assert_eq!(exhausted, 1, "a console that simply stops explains nothing");
}
