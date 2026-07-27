//! Restart-shaped proof that the NAP-INTENT handler registry survives
//! reopening the runtime.
//!
//! These tests deliberately live apart from `tests::intent`, which covers
//! dispatch inside one session. What they exercise is not dispatch but
//! `controller::intent_restore` -- and the only way to exercise it honestly
//! is to close a runtime and open another one against the same store.

use super::*;

/// Regression test for Bug A: the NAP-INTENT handler registry lives only in
/// process memory, and its only two writers were `install()` and
/// `reacquire_installed_artifact()` -- neither of which runs when the runtime
/// opens. A user could install a handler napplet, quit, reopen, and find its
/// archetype unresolvable, with the installation still listed, the grants
/// still held and the signed archetype declaration still in the store. The
/// only cure was reinstalling, and nothing on screen said so.
///
/// This is deliberately shaped as a **restart**, not as a call to the restore
/// function: what failed was that startup never called it. A test that
/// invokes the restore directly would prove the function works and say
/// nothing about whether opening a runtime reaches it -- the same gap #237
/// had, where three tests exercised a parse function no failure path
/// traversed.
///
/// Nothing in the second session installs, verifies or fetches the handler.
/// Its artifact source is deliberately empty, so any attempt to reach the
/// network for the handler's bytes fails rather than papering over a restore
/// that did not happen.
#[test]
fn an_installed_intent_handler_still_resolves_after_the_runtime_is_reopened() {
    let temp = TempDir::new().unwrap();
    let handler_index = b"<html>restarted handler</html>";
    let caller_index = b"<html>restarted caller</html>";
    let (handler_event, handler_author, handler_digest) = signed_manifest_event(
        "restart-handler",
        handler_index,
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
        "restart-caller",
        caller_index,
        vec![vec!["requires".to_owned(), "intent".to_owned()]],
    );
    let config = || RuntimeConfig {
        runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
        nmp_store_path: None,
        artifact_cache_path: temp.path().join("artifacts").display().to_string(),
        ..RuntimeConfig::default()
    };

    // First session: install and grant both napplets, then quit. The handler
    // is never launched -- only installed, exactly as a user who installs a
    // group-chat handler and then closes the app leaves it.
    let caller_coordinate = {
        let controller = RuntimeController::open(
            config(),
            Box::new(FixtureSource(BTreeMap::from([
                (handler_digest, handler_index.to_vec()),
                (caller_digest, caller_index.to_vec()),
            ]))),
        )
        .unwrap();
        let handler_artifact = controller
            .verify_artifact(
                handler_event,
                ArtifactCoordinate::Named {
                    author: handler_author,
                    d_tag: "restart-handler".to_owned(),
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
                    d_tag: "restart-caller".to_owned(),
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
        let coordinate = exact_coordinate(&caller_artifact);
        controller.close();
        coordinate
    };

    // Second session: a brand new process-equivalent against the same store
    // and the same sealed cache, with nothing to fetch from.
    let controller = RuntimeController::open(config(), Box::new(FixtureSource(BTreeMap::new())))
        .expect("the runtime reopens against the existing store");

    // The handler's sealed bytes are attached without anyone asking for them,
    // and the caller's -- which declares no archetype -- are not. The restore
    // reopens handlers, not the whole library: reopening re-hashes every
    // sealed byte, and this is what keeps that cost proportional to the
    // number of handlers rather than to the size of the library.
    let availability = |d_tag: &str| {
        controller
            .snapshot_value()
            .installed_library
            .builds
            .into_iter()
            .find(|build| build.coordinate.d_tag == d_tag)
            .unwrap_or_else(|| panic!("{d_tag} survived the restart as an installation"))
            .availability
    };
    assert_eq!(
        availability("restart-handler"),
        RuntimeInstalledBuildAvailability::SealedExactBytesReady,
        "the restore must attach the handler's artifact: without it the \
         dispatcher's own launch would be refused for want of a handle"
    );
    assert_eq!(
        availability("restart-caller"),
        RuntimeInstalledBuildAvailability::MetadataOnly,
        "a build declaring no archetype has no handler to restore, and must \
         not be reopened at startup for nothing"
    );

    // Launching the caller is the one thing the native shell does do, and it
    // is the only reacquisition in this session. It touches the caller alone.
    let caller_artifact = controller
        .reacquire_installed_artifact(caller_coordinate)
        .artifact
        .expect("the caller reopens offline from the sealed cache");
    controller.launch(caller_artifact, RuntimeExecutionProfile::Legacy);
    let caller_session = controller.snapshot_value().sessions[0].id;
    controller.mapped_envelope(caller_session, br#"{"type":"shell.ready"}"#.to_vec());

    controller.mapped_envelope(
        caller_session,
        serde_json::to_vec(&serde_json::json!({
            "type": "intent.invoke",
            "id": "invoke-after-restart",
            "request": {
                "archetype": "nip29-group",
                "convention": "napplet:nip29-group/open",
                "payload": {"group": "abc"}
            }
        }))
        .unwrap(),
    );

    // Before the restore existed, this is where the journey ended: no handler
    // was registered for `nip29-group`, so nothing was ever launched and the
    // caller's own napplet reported a failure it could not explain.
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
        assert!(
            Instant::now() < deadline,
            "the handler installed before the restart was never launched, so \
             its archetype did not survive reopening the runtime"
        );
        thread::sleep(Duration::from_millis(20));
    };

    controller.mapped_envelope(handler_session, br#"{"type":"shell.ready"}"#.to_vec());
    controller.mapped_envelope(
        handler_session,
        serde_json::to_vec(&serde_json::json!({
            "type": "inc.subscribe",
            "id": "sub-after-restart",
            "topic": "napplet:nip29-group/open"
        }))
        .unwrap(),
    );

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
    assert_eq!(result["id"], "invoke-after-restart");
    assert_eq!(result["result"]["ok"], true);
    assert_eq!(result["result"]["handled"], true);
    assert_eq!(result["result"]["archetype"], "nip29-group");
}

/// One installation the restore cannot read must cost only its own handlers.
///
/// Aborting the restore loop on the first failure would let a single
/// corrupted cache entry silently unregister every other napplet's
/// archetypes -- the same invisible breakage the restore exists to end, with
/// a wider blast radius. Skipping it without a word would be the other half
/// of that failure: the user would be back to a handler that stops working
/// for a reason nothing reports.
#[test]
fn one_unreadable_installation_does_not_cost_the_other_handlers_their_registration() {
    let temp = TempDir::new().unwrap();
    let archetype_tags = |archetype: &str| {
        vec![
            vec!["requires".to_owned(), "intent".to_owned()],
            vec![
                "archetype".to_owned(),
                archetype.to_owned(),
                format!("napplet:{archetype}/open"),
            ],
        ]
    };
    let survivor_index = b"<html>survivor</html>";
    let (survivor_event, survivor_author, survivor_digest) = signed_manifest_event(
        "surviving-handler",
        survivor_index,
        archetype_tags("nip29-group"),
    );
    // `installed_builds()` orders by author and every fixture is signed with
    // a freshly generated key, so which build the restore loop reaches first
    // would otherwise be a coin flip -- and the claim under test is about
    // what happens to the builds reached *after* a broken one. Regenerating
    // until the broken fixture sorts first is what makes this a test rather
    // than a test half the time.
    let broken_index = b"<html>broken</html>";
    let (broken_event, broken_author, broken_digest) = loop {
        let fixture = signed_manifest_event(
            "broken-handler",
            broken_index,
            archetype_tags("nip05-profile"),
        );
        if fixture.1 < survivor_author {
            break fixture;
        }
    };
    let config = || RuntimeConfig {
        runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
        nmp_store_path: None,
        artifact_cache_path: temp.path().join("artifacts").display().to_string(),
        ..RuntimeConfig::default()
    };

    let broken_aggregate = {
        let controller = RuntimeController::open(
            config(),
            Box::new(FixtureSource(BTreeMap::from([
                (survivor_digest, survivor_index.to_vec()),
                (broken_digest, broken_index.to_vec()),
            ]))),
        )
        .unwrap();
        let survivor = controller
            .verify_artifact(
                survivor_event,
                ArtifactCoordinate::Named {
                    author: survivor_author,
                    d_tag: "surviving-handler".to_owned(),
                },
            )
            .artifact
            .expect("survivor manifest verifies");
        controller.install(survivor);
        let broken = controller
            .verify_artifact(
                broken_event,
                ArtifactCoordinate::Named {
                    author: broken_author,
                    d_tag: "broken-handler".to_owned(),
                },
            )
            .artifact
            .expect("broken-to-be manifest verifies");
        let aggregate = exact_coordinate(&broken).aggregate_hash;
        controller.install(broken);
        controller.close();
        aggregate
    };

    // Destroy exactly one build's sealed bytes between sessions. Its
    // installation row, its retained signed manifest and its archetype
    // declaration all survive -- only the artifact it would execute is gone.
    fs::remove_dir_all(temp.path().join("artifacts").join(&broken_aggregate)).unwrap();

    let controller = RuntimeController::open(config(), Box::new(FixtureSource(BTreeMap::new())))
        .expect("an unreadable sealed artifact must not stop the runtime opening");

    let builds = controller.snapshot_value().installed_library.builds;
    let availability = |d_tag: &str| {
        builds
            .iter()
            .find(|build| build.coordinate.d_tag == d_tag)
            .unwrap_or_else(|| panic!("{d_tag} survived the restart as an installation"))
            .availability
    };
    assert_eq!(
        availability("broken-handler"),
        RuntimeInstalledBuildAvailability::MetadataOnly,
        "a build whose sealed bytes are gone must not be published as ready"
    );
    assert_eq!(
        availability("surviving-handler"),
        RuntimeInstalledBuildAvailability::SealedExactBytesReady,
        "the restore reached this build only by continuing past the broken \
         one that sorts ahead of it"
    );

    // …and it says which build lost its handlers, rather than leaving the
    // user with a napplet that stopped answering for no stated reason. The
    // registration itself is proven end-to-end by the restart test above;
    // what this pins is that one casualty stays one casualty, out loud.
    let refusal = controller
        .snapshot_value()
        .boundary_refusals
        .into_iter()
        .find(|refusal| refusal.code == "intent-handler-restore")
        .expect("the restore must record the installation it could not read");
    assert!(
        refusal.detail.contains("broken-handler"),
        "the refusal must name the build that lost its handlers, got {:?}",
        refusal.detail
    );
    assert!(
        !refusal.detail.contains("surviving-handler"),
        "only the unreadable build may be reported, got {:?}",
        refusal.detail
    );
}
