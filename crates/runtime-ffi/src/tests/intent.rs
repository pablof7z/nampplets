//! End-to-end NAP-INTENT dispatch across the UniFFI boundary.

use super::*;

/// End-to-end proof that NAP-INTENT dispatch is real, not hardcoded: an
/// installed-but-never-launched handler napplet gets launched by the
/// dispatcher itself in reaction to a caller's `intent.invoke`, and once
/// the (test-simulated) handler subscribes to its declared convention
/// topic, receives the invocation payload as a real `inc.event` push and
/// the caller receives a matching `ok:true` `intent.invoke.result`.
#[test]
fn intent_invoke_launches_a_registered_handler_and_delivers_the_payload_via_inc() {
    let temp = TempDir::new().unwrap();
    let (handler_event, handler_author, handler_digest) = signed_manifest_event(
        "nip29-chat-test",
        b"<html>handler</html>",
        vec![
            vec!["requires".to_owned(), "intent".to_owned()],
            vec!["requires".to_owned(), "inc".to_owned()],
            vec![
                "archetype".to_owned(),
                "nip29-group".to_owned(),
                "napplet:nip29-group/open".to_owned(),
            ],
        ],
    );
    let (caller_event, caller_author, caller_digest) = signed_manifest_event(
        "nip29-groups-test",
        b"<html>caller</html>",
        vec![vec!["requires".to_owned(), "intent".to_owned()]],
    );

    let controller = RuntimeController::open(
        RuntimeConfig {
            runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
            nmp_store_path: None,
            artifact_cache_path: temp.path().join("artifacts").display().to_string(),
            ..RuntimeConfig::default()
        },
        Box::new(FixtureSource(BTreeMap::from([
            (handler_digest, b"<html>handler</html>".to_vec()),
            (caller_digest, b"<html>caller</html>".to_vec()),
        ]))),
    )
    .unwrap();

    // Install the handler but never launch it -- the dispatcher itself
    // must be the one that launches it.
    let handler_artifact = controller
        .verify_artifact(
            handler_event,
            ArtifactCoordinate::Named {
                author: handler_author,
                d_tag: "nip29-chat-test".to_owned(),
            },
        )
        .artifact
        .expect("handler manifest verifies");
    controller.install(Arc::clone(&handler_artifact));
    for domain in ["intent", "inc"] {
        controller.set_grant(
            Arc::clone(&handler_artifact),
            domain.to_owned(),
            RuntimeSensitivity::Sensitive,
            RuntimeGrantDecision::AllowExactBuild,
        );
    }

    // Install, grant, and launch the caller.
    let caller_artifact = controller
        .verify_artifact(
            caller_event,
            ArtifactCoordinate::Named {
                author: caller_author,
                d_tag: "nip29-groups-test".to_owned(),
            },
        )
        .artifact
        .expect("caller manifest verifies");
    controller.install(Arc::clone(&caller_artifact));
    controller.set_grant(
        Arc::clone(&caller_artifact),
        "intent".to_owned(),
        RuntimeSensitivity::Sensitive,
        RuntimeGrantDecision::AllowExactBuild,
    );
    controller.launch(
        Arc::clone(&caller_artifact),
        RuntimeExecutionProfile::Legacy,
    );
    let caller_session = controller.snapshot_value().sessions[0].id;
    controller.mapped_envelope(caller_session, br#"{"type":"shell.ready"}"#.to_vec());

    controller.mapped_envelope(
        caller_session,
        serde_json::to_vec(&serde_json::json!({
            "type": "intent.invoke",
            "id": "invoke-1",
            "request": {
                "archetype": "nip29-group",
                "convention": "napplet:nip29-group/open",
                "payload": {"group": "abc"}
            }
        }))
        .unwrap(),
    );

    // The dispatcher launches the handler on a background thread; poll
    // for its session to appear.
    let deadline = Instant::now() + Duration::from_secs(5);
    let handler_session = loop {
        if let Some(session) = controller
            .snapshot_value()
            .sessions
            .iter()
            .find(|session| session.id != caller_session)
        {
            break session.id;
        }
        assert!(Instant::now() < deadline, "handler session never launched");
        thread::sleep(Duration::from_millis(20));
    };

    // Simulate the handler napplet's own JS boot: ready, then subscribe
    // to the exact convention it declared in its manifest.
    controller.mapped_envelope(handler_session, br#"{"type":"shell.ready"}"#.to_vec());
    controller.mapped_envelope(
        handler_session,
        serde_json::to_vec(&serde_json::json!({
            "type": "inc.subscribe",
            "id": "sub-1",
            "topic": "napplet:nip29-group/open"
        }))
        .unwrap(),
    );

    // The dispatcher's poll loop should now deliver the payload as a
    // real `inc.event` push and resolve the caller's invocation.
    let deadline = Instant::now() + Duration::from_secs(5);
    let event = loop {
        if let Some(event) = controller
            .app
            .events_after(0)
            .events
            .into_iter()
            .find_map(|event| match event.event {
                PlatformEvent::ProviderPush {
                    session, envelope, ..
                } if session == SessionId(handler_session)
                    && envelope.decode().ok()?.get("type")? == "inc.event" =>
                {
                    envelope.decode().ok()
                }
                _ => None,
            })
        {
            break event;
        }
        assert!(
            Instant::now() < deadline,
            "handler never received the inc.event push"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(event["topic"], "napplet:nip29-group/open");
    assert_eq!(event["sender"], "nip29-groups-test");
    assert_eq!(event["payload"], serde_json::json!({"group": "abc"}));

    // `intent.invoke.result` is delivered asynchronously as a provider push
    // to the caller's session (mirroring `inc.event` above), not as a
    // synchronous `EnvelopeHandled` response to the original `intent.invoke`
    // call -- `IntentProvider::invoke` returns immediately with no response
    // and only pushes the result once `complete()` runs.
    let deadline = Instant::now() + Duration::from_secs(5);
    let result = loop {
        if let Some(result) = controller
            .app
            .events_after(0)
            .events
            .into_iter()
            .find_map(|event| match event.event {
                PlatformEvent::ProviderPush {
                    session, envelope, ..
                } if session == SessionId(caller_session)
                    && envelope.decode().ok()?.get("type")? == "intent.invoke.result" =>
                {
                    envelope.decode().ok()
                }
                _ => None,
            })
        {
            break result;
        }
        assert!(
            Instant::now() < deadline,
            "caller never received intent.invoke.result"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(result["id"], "invoke-1");
    assert_eq!(result["result"]["ok"], true);
    assert_eq!(result["result"]["handled"], true);
    assert_eq!(result["result"]["archetype"], "nip29-group");
}

/// Regression test for the defect this integration fixed: an intent launch
/// must apply the *same* required-domain precondition an interactive launch
/// applies, including for a handler whose domains are declared only by the
/// `napplet-requires` meta in its verified `/index.html`.
///
/// `required_domains` is a fail-closed precondition, not an injection list:
/// `Registry::negotiate` injects whatever the principal has been granted and
/// refuses with `MissingRequiredDomains` when a required domain is not among
/// them. `intent_dispatch::launch_handler` used to derive that set from
/// signed `requires` tags alone, so a meta-declaring handler -- which
/// `nip29-groups` is -- produced an *empty* required set and therefore
/// launched happily with none of the capabilities its own content needs,
/// instead of refusing. It now uses the same
/// `installation_capability_requests` an interactive launch uses, so an
/// ungranted handler fails closed here exactly as it would there.
#[test]
fn intent_launch_applies_the_same_required_domains_precondition_as_an_interactive_launch() {
    let temp = TempDir::new().unwrap();
    let handler_index =
        b"<html><head><meta name=\"napplet-requires\" content=\"inc,intent\"></head></html>";
    let (handler_event, handler_author, handler_digest) = signed_manifest_event(
        "meta-declared-handler",
        handler_index,
        // Deliberately no `requires` tags: the archetype tag alone makes it a
        // discoverable handler, and the domains live only in the entry
        // document, which the signed path digest and aggregate still pin.
        vec![vec![
            "archetype".to_owned(),
            "nip29-group".to_owned(),
            "napplet:nip29-group/open".to_owned(),
        ]],
    );
    let (caller_event, caller_author, caller_digest) = signed_manifest_event(
        "meta-declared-caller",
        b"<html>caller</html>",
        vec![vec!["requires".to_owned(), "intent".to_owned()]],
    );

    let controller = RuntimeController::open(
        RuntimeConfig {
            runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
            nmp_store_path: None,
            artifact_cache_path: temp.path().join("artifacts").display().to_string(),
            ..RuntimeConfig::default()
        },
        Box::new(FixtureSource(BTreeMap::from([
            (handler_digest, handler_index.to_vec()),
            (caller_digest, b"<html>caller</html>".to_vec()),
        ]))),
    )
    .unwrap();

    let handler_artifact = controller
        .verify_artifact(
            handler_event,
            ArtifactCoordinate::Named {
                author: handler_author,
                d_tag: "meta-declared-handler".to_owned(),
            },
        )
        .artifact
        .expect("handler manifest verifies");
    assert!(
        handler_artifact.requires().is_empty(),
        "the fixture must carry no signed `requires` tags, or it proves nothing"
    );
    controller.install(Arc::clone(&handler_artifact));
    // The install-time review is what the meta declaration produces; the
    // dispatcher must reach the same inventory.
    let review = controller
        .permission_review(exact_coordinate(&handler_artifact))
        .review
        .expect("installed handler has a permission review");
    assert_eq!(
        review
            .capabilities
            .iter()
            .map(|capability| capability.domain.as_str())
            .collect::<Vec<_>>(),
        ["inc", "intent"],
        "the meta declaration is the handler's capability inventory"
    );
    // Deliberately grant the handler nothing. An interactive launch refuses
    // this build; the dispatcher must refuse it the same way.
    controller.launch(
        Arc::clone(&handler_artifact),
        RuntimeExecutionProfile::Legacy,
    );
    assert!(
        controller.snapshot_value().sessions.is_empty(),
        "an interactive launch must refuse a handler whose required domains \
         are ungranted -- this is the behavior intent dispatch has to match"
    );

    let caller_artifact = controller
        .verify_artifact(
            caller_event,
            ArtifactCoordinate::Named {
                author: caller_author,
                d_tag: "meta-declared-caller".to_owned(),
            },
        )
        .artifact
        .expect("caller manifest verifies");
    controller.install(Arc::clone(&caller_artifact));
    controller.set_grant(
        Arc::clone(&caller_artifact),
        "intent".to_owned(),
        RuntimeSensitivity::Sensitive,
        RuntimeGrantDecision::AllowExactBuild,
    );
    controller.launch(
        Arc::clone(&caller_artifact),
        RuntimeExecutionProfile::Legacy,
    );
    let caller_session = controller.snapshot_value().sessions[0].id;
    controller.mapped_envelope(caller_session, br#"{"type":"shell.ready"}"#.to_vec());

    controller.mapped_envelope(
        caller_session,
        serde_json::to_vec(&serde_json::json!({
            "type": "intent.invoke",
            "id": "invoke-meta-1",
            "request": {
                "archetype": "nip29-group",
                "convention": "napplet:nip29-group/open",
                "payload": {"group": "abc"}
            }
        }))
        .unwrap(),
    );

    // Give the dispatcher's background thread real time to reach its first
    // `launch_handler` call and for the app to process the command.
    let settle = Instant::now() + Duration::from_secs(2);
    while Instant::now() < settle {
        thread::sleep(Duration::from_millis(20));
    }

    // The concrete regression. With a signed-tags-only derivation the
    // required set was empty, `negotiate` had nothing to check the empty
    // grant set against, and this handler came up as a live session holding
    // only the foundational `shell` domain -- a napplet running without the
    // capabilities its own content declares. It must be refused instead.
    let sessions = controller.snapshot_value().sessions;
    let handler_session = sessions.iter().find(|session| session.id != caller_session);
    assert!(
        handler_session.is_none(),
        "an intent-launched handler whose meta-declared domains are ungranted \
         must fail closed, not launch degraded; got a session with domains {:?}",
        handler_session.map(|session| session.domains.clone())
    );
    // …and it must say why, naming the domains it could not inject, rather
    // than failing silently.
    let refusal = controller
        .snapshot_value()
        .recent_errors
        .into_iter()
        .find(|fact| fact.code == "bridge" && fact.detail.contains("intent"))
        .expect("the refusal must name the missing required domains");
    assert!(
        refusal.detail.contains("inc"),
        "the refusal must report every missing meta-declared domain, got {:?}",
        refusal.detail
    );
}

/// The retry loop must distinguish "the handler never subscribed to the
/// convention it's dispatched for" from a bare `Failed`: before this fix,
/// every failure of `intent.invoke` -- launch refused, never subscribed,
/// session ended, push refused -- reported the identical fixed string
/// `"invoke failed"`, so a napplet dispatching an intent could never tell
/// these apart. Simulates the handler launching and reaching `shell.ready`
/// but never sending `inc.subscribe`, and asserts the caller's eventual
/// `intent.invoke.result` names that specific cause.
#[test]
fn intent_invoke_reports_why_it_failed_when_the_handler_never_subscribes() {
    let temp = TempDir::new().unwrap();
    let (handler_event, handler_author, handler_digest) = signed_manifest_event(
        "nip29-chat-test",
        b"<html>handler</html>",
        vec![
            vec!["requires".to_owned(), "intent".to_owned()],
            vec!["requires".to_owned(), "inc".to_owned()],
            vec![
                "archetype".to_owned(),
                "nip29-group".to_owned(),
                "napplet:nip29-group/open".to_owned(),
            ],
        ],
    );
    let (caller_event, caller_author, caller_digest) = signed_manifest_event(
        "nip29-groups-test",
        b"<html>caller</html>",
        vec![vec!["requires".to_owned(), "intent".to_owned()]],
    );

    let controller = RuntimeController::open(
        RuntimeConfig {
            runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
            nmp_store_path: None,
            artifact_cache_path: temp.path().join("artifacts").display().to_string(),
            ..RuntimeConfig::default()
        },
        Box::new(FixtureSource(BTreeMap::from([
            (handler_digest, b"<html>handler</html>".to_vec()),
            (caller_digest, b"<html>caller</html>".to_vec()),
        ]))),
    )
    .unwrap();

    let handler_artifact = controller
        .verify_artifact(
            handler_event,
            ArtifactCoordinate::Named {
                author: handler_author,
                d_tag: "nip29-chat-test".to_owned(),
            },
        )
        .artifact
        .expect("handler manifest verifies");
    controller.install(Arc::clone(&handler_artifact));
    for domain in ["intent", "inc"] {
        controller.set_grant(
            Arc::clone(&handler_artifact),
            domain.to_owned(),
            RuntimeSensitivity::Sensitive,
            RuntimeGrantDecision::AllowExactBuild,
        );
    }

    let caller_artifact = controller
        .verify_artifact(
            caller_event,
            ArtifactCoordinate::Named {
                author: caller_author,
                d_tag: "nip29-groups-test".to_owned(),
            },
        )
        .artifact
        .expect("caller manifest verifies");
    controller.install(Arc::clone(&caller_artifact));
    controller.set_grant(
        Arc::clone(&caller_artifact),
        "intent".to_owned(),
        RuntimeSensitivity::Sensitive,
        RuntimeGrantDecision::AllowExactBuild,
    );
    controller.launch(
        Arc::clone(&caller_artifact),
        RuntimeExecutionProfile::Legacy,
    );
    let caller_session = controller.snapshot_value().sessions[0].id;
    controller.mapped_envelope(caller_session, br#"{"type":"shell.ready"}"#.to_vec());

    controller.mapped_envelope(
        caller_session,
        serde_json::to_vec(&serde_json::json!({
            "type": "intent.invoke",
            "id": "invoke-1",
            "request": {
                "archetype": "nip29-group",
                "convention": "napplet:nip29-group/open",
                "payload": {"group": "abc"}
            }
        }))
        .unwrap(),
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    let handler_session = loop {
        if let Some(session) = controller
            .snapshot_value()
            .sessions
            .iter()
            .find(|session| session.id != caller_session)
        {
            break session.id;
        }
        assert!(Instant::now() < deadline, "handler session never launched");
        thread::sleep(Duration::from_millis(20));
    };

    // Simulate the handler napplet's own JS boot reaching `shell.ready`
    // but deliberately never sending `inc.subscribe` -- the exact case
    // `NativeIntentFailureReason::HandlerNeverSubscribed` names.
    controller.mapped_envelope(handler_session, br#"{"type":"shell.ready"}"#.to_vec());

    // The retry loop's full poll budget is 40 * 250ms = 10s.
    let deadline = Instant::now() + Duration::from_secs(15);
    let result = loop {
        if let Some(result) = controller
            .app
            .events_after(0)
            .events
            .into_iter()
            .find_map(|event| match event.event {
                PlatformEvent::ProviderPush {
                    session, envelope, ..
                } if session == SessionId(caller_session)
                    && envelope.decode().ok()?.get("type")? == "intent.invoke.result" =>
                {
                    envelope.decode().ok()
                }
                _ => None,
            })
        {
            break result;
        }
        assert!(
            Instant::now() < deadline,
            "caller never received intent.invoke.result"
        );
        thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(result["result"]["ok"], false);
    assert_eq!(result["result"]["handled"], false);
    assert_eq!(
        result["result"]["error"],
        "handler launched but never subscribed to the requested convention",
        "a bare, undifferentiated failure must not still be reported here"
    );
}
