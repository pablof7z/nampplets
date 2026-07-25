//! Provider-to-component push authority, source binding, and lane termination.

mod support;

use std::sync::Arc;

use nmp_native_nap_bridge::{ProviderPushError, ProviderSessionEnd};
use nmp_native_runtime_app::PlatformCommand;
use support::*;

#[test]
fn provider_lifecycle_is_source_bound_and_conflated_pushes_start_after_ready() {
    let rig = Rig::new(false);
    let principal = principal('b');
    rig.install(principal.clone());
    rig.allow_runtime(principal.clone());
    let session = rig.launch(principal.clone());

    let opened = rig.provider.opened.lock().clone();
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].principal, principal);
    assert_eq!(opened[0].session, session);
    assert_eq!(rig.app.snapshot().provider_push_lanes.len(), 1);
    assert!(!rig.app.snapshot().provider_push_lanes[0].ready);
    assert_eq!(
        rig.app.snapshot().provider_push_lanes[0].source_window,
        opened[0].source_window
    );

    let sender = rig.provider.sender(session);
    let first = sender
        .push(
            "canary.state",
            serde_json::Map::from_iter([("value".to_owned(), serde_json::json!(1))]),
            Some("current"),
        )
        .unwrap();
    let second = sender
        .push(
            "canary.state",
            serde_json::Map::from_iter([("value".to_owned(), serde_json::json!(2))]),
            Some("current"),
        )
        .unwrap();
    assert!(second > first);
    assert!(!rig.app.events_after(0).events.into_iter().any(|event| {
        matches!(
            event.event,
            nmp_native_runtime_app::PlatformEvent::ProviderPush { .. }
        )
    }));

    rig.ready(session);
    let event = wait_for_event(&rig.app, |event| {
        matches!(
            event,
            nmp_native_runtime_app::PlatformEvent::ProviderPush {
                session: pushed_session,
                ..
            } if *pushed_session == session
        )
    });
    let nmp_native_runtime_app::PlatformEvent::ProviderPush {
        source_window,
        provider_sequence,
        domain,
        envelope,
        ..
    } = event
    else {
        unreachable!()
    };
    assert_eq!(source_window, opened[0].source_window);
    assert_eq!(provider_sequence, second);
    assert_eq!(domain, canary());
    assert_eq!(
        envelope.decode().unwrap(),
        serde_json::json!({"type": "canary.state", "value": 2})
    );
    assert_eq!(rig.provider.ready.lock().as_slice(), &[opened[0].clone()]);
    let lane = &rig.app.snapshot().provider_push_lanes[0];
    assert!(lane.ready);
    assert_eq!(lane.last_provider_sequence, Some(second));
    assert_eq!(lane.delivered_count, 1);

    rig.ready(session);
    assert_eq!(
        rig.provider.ready.lock().len(),
        1,
        "ready lifecycle is idempotent"
    );
    rig.app.dispatch(PlatformCommand::Stop { session });
    assert_eq!(
        rig.provider.closed.lock().as_slice(),
        &[(opened[0].clone(), ProviderSessionEnd::Stopped)]
    );
    assert_eq!(
        sender.push("canary.state", serde_json::Map::new(), None),
        Err(ProviderPushError::Closed)
    );
    assert_eq!(rig.app.snapshot().resources.admitted, 0);
}

#[test]
fn provider_push_authority_spoof_revoke_and_termination_fail_closed() {
    let rig = Rig::new(false);
    let principal = principal('b');
    rig.install(principal.clone());
    rig.allow_runtime(principal.clone());
    let session = rig.launch(principal.clone());
    let sender = rig.provider.sender(session);

    assert_eq!(
        sender.push(
            "canary.state",
            serde_json::Map::from_iter([(
                "principal".to_owned(),
                serde_json::json!(principal.clone())
            )]),
            None,
        ),
        Err(ProviderPushError::AuthorityField)
    );
    assert_eq!(
        sender.push("other.state", serde_json::Map::new(), None),
        Err(ProviderPushError::DomainMismatch)
    );

    rig.ready(session);
    rig.app.dispatch(PlatformCommand::Revoke {
        principal: principal.clone(),
        capability: canary(),
    });
    assert_eq!(
        sender.push("canary.state", serde_json::Map::new(), None),
        Err(ProviderPushError::Revoked)
    );
    assert_eq!(rig.provider.revoked.lock().len(), 1);
    assert_eq!(rig.provider.revoked.lock()[0].session, session);
    assert!(!rig.app.events_after(0).events.into_iter().any(|event| {
        matches!(
            event.event,
            nmp_native_runtime_app::PlatformEvent::ProviderPush { .. }
        )
    }));
    rig.app.dispatch(PlatformCommand::Crash {
        session,
        reason: Arc::from("test crash"),
    });
    assert_eq!(
        rig.provider.closed.lock().last().unwrap().1,
        ProviderSessionEnd::Crashed
    );
    assert_eq!(rig.app.snapshot().resources.admitted, 0);

    rig.allow_runtime(principal.clone());
    let replacement = rig.launch(principal);
    let replacement_sender = rig.provider.sender(replacement);
    rig.ready(replacement);
    replacement_sender.terminate(nmp_native_nap_bridge::ProviderPushTermination::ProviderFailure);
    let _ = wait_for_event(&rig.app, |event| {
        matches!(
            event,
            nmp_native_runtime_app::PlatformEvent::ProviderPushLaneClosed {
                session: closed_session,
                termination: Some(
                    nmp_native_nap_bridge::ProviderPushTermination::ProviderFailure
                ),
                ..
            } if *closed_session == replacement
        )
    });
    assert!(rig.app.snapshot().sessions.is_empty());
    assert_eq!(rig.app.snapshot().resources.admitted, 0);
    assert_eq!(
        rig.provider.closed.lock().last().unwrap().1,
        ProviderSessionEnd::Crashed
    );
}
