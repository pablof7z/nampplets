//! NAP-SHELL handshake, session plan enforcement, and session lifecycle.

mod support;

use std::{collections::BTreeSet, sync::Arc};

use nmp_native_nap_bridge::ProviderSessionEnd;
use nmp_native_runtime_app::{AppErrorCode, PlatformCommand};
use nmp_native_runtime_core::{
    BindingRequest, ExecutionProfile, GrantDecision, Sensitivity, SessionId, SessionState,
};
use nmp_native_runtime_store::{InstalledBuild, RuntimeStore, StoreLimits};
use nmp_native_test_harness::FakeHostDataPlane;
use support::*;
use tempfile::TempDir;

#[test]
fn nap_shell_gates_capabilities_and_emits_exactly_one_uncorrelated_init() {
    let rig = Rig::new(false);
    let principal = principal('b');
    rig.install(principal.clone());
    rig.allow_runtime(principal.clone());
    let session = rig.launch(principal);
    assert_eq!(
        rig.app.snapshot().session_domains,
        vec![nmp_native_runtime_app::SessionDomainView {
            session,
            domains: vec![canary(), shell()],
            // Every required domain was served, so the shortfall is empty
            // rather than absent.
            unavailable_domains: Vec::new(),
        }]
    );

    for unknown in [
        serde_json::json!({"type": "future.unknown"}),
        serde_json::json!({"type": "canary.future"}),
    ] {
        rig.app.dispatch(PlatformCommand::MappedEnvelope {
            session,
            bytes: mapped(unknown),
        });
    }
    rig.app.dispatch(PlatformCommand::MappedEnvelope {
        session,
        bytes: ping(serde_json::json!({})),
    });
    assert!(rig.provider.seen.lock().is_empty());
    assert!(!rig.shell_provider.is_ready(session));
    assert_eq!(
        rig.app.snapshot().recent_errors.last().unwrap().code,
        AppErrorCode::Bridge
    );
    assert_eq!(
        rig.app
            .events_after(0)
            .events
            .into_iter()
            .filter(|item| matches!(
                item.event,
                nmp_native_runtime_app::PlatformEvent::EnvelopeIgnored { .. }
            ))
            .count(),
        2,
        "unknown well-formed messages remain forward-compatible before readiness"
    );

    rig.ready(session);
    assert!(rig.shell_provider.is_ready(session));
    let first_init = rig
        .app
        .events_after(0)
        .events
        .into_iter()
        .filter_map(|item| match item.event {
            nmp_native_runtime_app::PlatformEvent::EnvelopeHandled {
                session: handled_session,
                response: Some(response),
                ..
            } if handled_session == session => Some(response.decode().unwrap()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        first_init,
        vec![serde_json::json!({
            "type": "shell.init",
            "capabilities": {"domains": ["canary", "shell"]},
            "services": ["settings"]
        })]
    );
    assert!(
        first_init[0].get("id").is_none(),
        "shell.init is uncorrelated"
    );

    rig.ready(session);
    let init_count = rig
        .app
        .events_after(0)
        .events
        .into_iter()
        .filter(|item| {
            matches!(
                &item.event,
                nmp_native_runtime_app::PlatformEvent::EnvelopeHandled {
                    response: Some(response),
                    ..
                } if response.decode().unwrap()["type"] == "shell.init"
            )
        })
        .count();
    assert_eq!(init_count, 1, "a replay must not resend shell.init");

    rig.app.dispatch(PlatformCommand::MappedEnvelope {
        session,
        bytes: ping(serde_json::json!({})),
    });
    assert_eq!(rig.provider.seen.lock().len(), 1);
}

#[test]
fn unroutable_envelope_type_is_recorded_not_silently_dropped() {
    let rig = Rig::new(false);
    let principal = principal('c');
    rig.install(principal.clone());
    rig.allow_runtime(principal.clone());
    let session = rig.launch(principal);

    // `Relay.query` fails `Capability::new` (uppercase domain), so
    // `envelope_route` returns `None`. Before this fix, that carried no
    // trace anywhere: a napplet posting this would see its call never
    // resolve, with nothing in the activity ring explaining why.
    rig.app.dispatch(PlatformCommand::MappedEnvelope {
        session,
        bytes: mapped(serde_json::json!({"type": "Relay.query"})),
    });

    assert!(
        rig.app.snapshot().recent_activity.iter().any(|fact| {
            fact.category.as_ref() == "envelope"
                && fact.operation.as_ref() == "route"
                && fact.outcome.as_ref() == "unroutable"
        }),
        "an unroutable envelope type must leave a bounded activity fact"
    );
}

#[test]
fn nap_shell_rejects_id_extra_fields_and_payload_identity_claims() {
    let rig = Rig::new(false);
    let real = principal('b');
    let forged = principal('c');
    rig.install(real.clone());
    rig.allow_runtime(real.clone());
    let session = rig.launch(real);

    for invalid in [
        serde_json::json!({"type": "shell.ready", "id": "forbidden"}),
        serde_json::json!({"type": "shell.ready", "id": null}),
        serde_json::json!({"type": "shell.ready", "capabilities": ["storage"]}),
        serde_json::json!({
            "type": "shell.ready",
            "principal": forged,
            "session": 9_999
        }),
    ] {
        rig.app.dispatch(PlatformCommand::MappedEnvelope {
            session,
            bytes: mapped(invalid),
        });
        assert!(!rig.shell_provider.is_ready(session));
        assert_eq!(
            rig.app.snapshot().recent_errors.last().unwrap().code,
            AppErrorCode::Bridge
        );
    }

    rig.app.dispatch(PlatformCommand::MappedEnvelope {
        session,
        bytes: ping(serde_json::json!({})),
    });
    assert!(
        rig.provider.seen.lock().is_empty(),
        "invalid readiness never opens another capability"
    );
}

#[test]
fn nap_shell_state_is_closed_and_never_reused_by_a_relaunch() {
    let rig = Rig::new(false);
    let principal = principal('b');
    rig.install(principal.clone());
    rig.allow_runtime(principal.clone());
    let first = rig.launch(principal.clone());
    rig.ready(first);
    assert!(rig.shell_provider.is_ready(first));

    rig.app.dispatch(PlatformCommand::Stop { session: first });
    assert!(!rig.shell_provider.is_ready(first));
    rig.app.dispatch(PlatformCommand::MappedEnvelope {
        session: first,
        bytes: ready(),
    });
    assert_eq!(
        rig.app.snapshot().recent_errors.last().unwrap().code,
        AppErrorCode::UnknownSession
    );
    assert!(!rig.shell_provider.is_ready(first));

    let second = rig.launch(principal);
    assert!(second.0 > first.0);
    assert!(!rig.shell_provider.is_ready(second));
    rig.app.dispatch(PlatformCommand::MappedEnvelope {
        session: second,
        bytes: ping(serde_json::json!({})),
    });
    assert!(rig.provider.seen.lock().is_empty());
    rig.ready(second);
    assert!(rig.shell_provider.is_ready(second));

    rig.app.dispatch(PlatformCommand::Close);
    assert!(!rig.shell_provider.is_ready(second));
    assert_eq!(
        rig.provider.closed.lock().last().unwrap().1,
        ProviderSessionEnd::RuntimeClosed
    );
}

#[test]
fn launch_is_refused_when_shell_environment_differs_from_the_session_plan() {
    let directory = TempDir::new().unwrap();
    let store = Arc::new(
        RuntimeStore::open(directory.path().join("runtime.db"), StoreLimits::default()).unwrap(),
    );
    let host = Arc::new(FakeHostDataPlane::new(16));
    let provider = Arc::new(CapturingProvider::new(false));
    let (app, shell_provider) =
        open_app_with_shell_domains(store, host, provider.clone(), BTreeSet::from([shell()]));
    let principal = principal('b');
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
    app.dispatch(PlatformCommand::SetGrant {
        principal: principal.clone(),
        capability: canary(),
        sensitivity: Sensitivity::Ordinary,
        decision: GrantDecision::AllowExactBuild,
    });
    app.dispatch(PlatformCommand::Launch {
        principal,
        profile: ExecutionProfile::Legacy,
        required_domains: BTreeSet::from([canary()]),
    });
    assert!(app.snapshot().sessions.is_empty());
    assert!(!shell_provider.is_ready(SessionId(1)));
    assert_eq!(
        app.snapshot().recent_errors.last().unwrap().detail.as_ref(),
        "shell environment does not equal the exact negotiated capability set"
    );
    assert!(!app.events_after(0).events.into_iter().any(|item| matches!(
        item.event,
        nmp_native_runtime_app::PlatformEvent::EnvelopeHandled {
            response: Some(_),
            ..
        }
    )));
}

#[test]
fn stop_crash_and_revoke_return_session_resources_without_closing_binding() {
    let rig = Rig::new(true);
    let principal = principal('b');
    rig.install(principal.clone());
    rig.allow_runtime(principal.clone());
    rig.app.dispatch(PlatformCommand::OpenBinding {
        request: BindingRequest {
            workspace_binding_id: Arc::from("feed"),
            family: Arc::from("event.collection"),
            schema: Arc::from("nostr.events.collection/1"),
            parameters: json(serde_json::json!({"authors": [principal.manifest_author()]})),
            maximum_rows: 50,
            maximum_frame_bytes: 64 * 1024,
        },
    });

    let first = rig.launch(principal.clone());
    assert_eq!(rig.app.snapshot().resources.admitted, 1);
    rig.ready(first);
    rig.app.dispatch(PlatformCommand::MappedEnvelope {
        session: first,
        bytes: ping(serde_json::json!({})),
    });
    assert_eq!(
        rig.app.snapshot().resources.admitted,
        3,
        "webview, provider delivery, and active provider operation are charged"
    );

    rig.app.dispatch(PlatformCommand::Revoke {
        principal: principal.clone(),
        capability: canary(),
    });
    assert_eq!(
        rig.app.snapshot().resources.admitted,
        2,
        "revocation cancels the domain operation while the session delivery lane remains"
    );
    rig.app.dispatch(PlatformCommand::Crash {
        session: first,
        reason: Arc::from("web-content-process-exited"),
    });
    assert_eq!(rig.app.snapshot().resources.admitted, 0);
    assert_eq!(rig.host.binding_count(), 1);
    assert!(rig.app.binding("feed").is_some());

    rig.allow_runtime(principal.clone());
    let second = rig.launch(principal);
    assert!(second.0 > first.0);
    rig.app.dispatch(PlatformCommand::Stop { session: second });
    assert_eq!(rig.app.snapshot().resources.admitted, 0);
    assert_eq!(rig.host.binding_count(), 1);

    rig.app.dispatch(PlatformCommand::Close);
    assert_eq!(rig.host.binding_count(), 0);
}

#[test]
fn mapped_payload_identity_is_ignored_and_stale_session_is_refused() {
    let rig = Rig::new(false);
    let real = principal('b');
    let forged = principal('c');
    rig.install(real.clone());
    rig.allow_runtime(real.clone());
    let session = rig.launch(real.clone());
    rig.ready(session);

    rig.app.dispatch(PlatformCommand::MappedEnvelope {
        session,
        bytes: ping(serde_json::json!({
            "principal": forged,
            "session": 9_999,
            "profile": "hybrid"
        })),
    });
    let seen = rig.provider.seen.lock();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, real);
    assert_eq!(seen[0].1, session);
    drop(seen);

    rig.app.dispatch(PlatformCommand::Stop { session });
    rig.app.dispatch(PlatformCommand::MappedEnvelope {
        session,
        bytes: ping(serde_json::json!({})),
    });
    assert_eq!(rig.provider.seen.lock().len(), 1);
    assert_eq!(
        rig.app.snapshot().recent_errors.last().unwrap().code,
        AppErrorCode::UnknownSession
    );

    rig.allow_runtime(real.clone());
    let replacement = rig.launch(real);
    assert!(
        replacement.0 > session.0,
        "session ids are never caller-reused"
    );
}

#[test]
fn suspend_resume_is_typed_and_stale_session_handles_remain_inert() {
    let rig = Rig::new(false);
    let principal = principal('b');
    rig.install(principal.clone());
    rig.allow_runtime(principal.clone());
    let session = rig.launch(principal);

    rig.app.dispatch(PlatformCommand::Suspend { session });
    assert_eq!(
        rig.app.snapshot().sessions[0].state,
        SessionState::Suspended
    );
    rig.app.dispatch(PlatformCommand::MappedEnvelope {
        session,
        bytes: ready(),
    });
    assert_eq!(
        rig.app.snapshot().recent_errors.last().unwrap().code,
        AppErrorCode::InvalidLifecycle
    );

    rig.app.dispatch(PlatformCommand::Resume { session });
    assert_eq!(rig.app.snapshot().sessions[0].state, SessionState::Running);
    rig.ready(session);
    assert!(rig.shell_provider.is_ready(session));

    rig.app.dispatch(PlatformCommand::Stop { session });
    rig.app.dispatch(PlatformCommand::Resume { session });
    assert_eq!(
        rig.app.snapshot().recent_errors.last().unwrap().code,
        AppErrorCode::UnknownSession
    );
}
