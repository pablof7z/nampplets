use super::*;

#[test]
fn signed_artifact_crosses_only_as_sealed_handle_and_exact_reads() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp);
    let verified = controller.verify_artifact(
        EVENT.to_vec(),
        ArtifactCoordinate::Named {
            author: AUTHOR.to_owned(),
            d_tag: "good-morning".to_owned(),
        },
    );
    let artifact = verified.artifact.expect("published fixture verifies");
    assert!(verified.refusal.is_none());
    assert!(artifact.requires().is_empty());
    controller.install(Arc::clone(&artifact));
    controller.set_grant(
        Arc::clone(&artifact),
        "shell".to_owned(),
        RuntimeSensitivity::Ordinary,
        RuntimeGrantDecision::AllowExactBuild,
    );
    for domain in ["identity", "inc", "outbox"] {
        controller.set_grant(
            Arc::clone(&artifact),
            domain.to_owned(),
            RuntimeSensitivity::Sensitive,
            RuntimeGrantDecision::AllowExactBuild,
        );
    }
    controller.launch(artifact, RuntimeExecutionProfile::Legacy);
    let runtime_snapshot = controller.snapshot_value();
    assert_eq!(
        runtime_snapshot.sessions[0].domains,
        ["identity", "inc", "outbox", "shell"]
    );
    let session = runtime_snapshot.sessions[0].id;
    controller.mapped_envelope(session, br#"{"type":"shell.ready"}"#.to_vec());
    controller.mapped_envelope(
        session,
        br#"{"type":"identity.getPublicKey","id":"identity-1"}"#.to_vec(),
    );
    let identity_response = controller
        .app
        .events_after(0)
        .events
        .into_iter()
        .find_map(|event| match event.event {
            PlatformEvent::EnvelopeHandled {
                response: Some(response),
                ..
            } if response.decode().ok()?.get("type")? == "identity.getPublicKey.result" => {
                response.decode().ok()
            }
            _ => None,
        })
        .expect("registered identity provider responds through the runtime");
    assert_eq!(identity_response["id"], "identity-1");
    assert_eq!(identity_response["pubkey"], "");
    controller.mapped_envelope(
        session,
        br#"{"type":"inc.subscribe","id":"inc-1","topic":"profile:open"}"#.to_vec(),
    );
    let inc_response = controller
        .app
        .events_after(0)
        .events
        .into_iter()
        .find_map(|event| match event.event {
            PlatformEvent::EnvelopeHandled {
                response: Some(response),
                ..
            } if response.decode().ok()?.get("type")? == "inc.subscribe.result" => {
                response.decode().ok()
            }
            _ => None,
        })
        .expect("registered INC provider responds through the runtime");
    assert_eq!(inc_response["id"], "inc-1");
    match controller.read_verified(session, "/index.html".to_owned(), 1_024 * 1_024) {
        VerifiedRead::Bytes { bytes, .. } => assert_eq!(bytes, INDEX),
        VerifiedRead::Refused { refusal } => panic!("{refusal:?}"),
    }
    assert!(matches!(
        controller.read_verified(session, "/../secret".to_owned(), 1_024),
        VerifiedRead::Refused { .. }
    ));
    controller.close();
    assert!(controller.snapshot_value().closed);
    assert!(fs::metadata(temp.path().join("runtime.sqlite3")).is_ok());
}

#[test]
fn malformed_manifest_is_a_semantic_refusal() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp);
    let result = controller.verify_artifact(
        b"{}".to_vec(),
        ArtifactCoordinate::Named {
            author: "0".repeat(64),
            d_tag: "fixture".to_owned(),
        },
    );
    assert!(result.artifact.is_none());
    assert_eq!(result.refusal.unwrap().code, "artifact-verification");
}
