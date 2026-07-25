use super::*;

#[test]
fn native_capabilities_are_absent_unless_supplied() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp);
    let (_, session) = install_and_launch(&controller, &["theme", "config"]);
    let snapshot = controller.snapshot();
    let domains = &snapshot
        .sessions
        .iter()
        .find(|candidate| candidate.id == session)
        .unwrap()
        .domains;
    assert!(!domains.iter().any(|domain| domain == "theme"));
    assert!(!domains.iter().any(|domain| domain == "config"));
}

#[test]
fn native_theme_and_settings_cross_the_exact_build_boundary() {
    let temp = TempDir::new().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let controller = controller_with_native_capabilities(&temp, Arc::clone(&requests));
    let (_, session) = install_and_launch(&controller, &["theme", "config"]);
    let domains = &controller.snapshot().sessions[0].domains;
    assert!(domains.iter().any(|domain| domain == "theme"));
    assert!(domains.iter().any(|domain| domain == "config"));

    controller.mapped_envelope(session, br#"{"type":"theme.get","id":"theme-1"}"#.to_vec());
    assert_eq!(
        response_of_type(&controller, "theme.get.result")["theme"]["colors"],
        serde_json::json!({
            "background": "#1c1c1e",
            "text": "#ffffff",
            "primary": "#58a6ff"
        })
    );
    let changed = controller.update_appearance(NativeAppearanceSnapshot {
        dark: false,
        increased_contrast: true,
        reduced_transparency: true,
        accent_red: 0,
        accent_green: 102,
        accent_blue: 204,
    });
    assert!(changed.accepted);
    assert_eq!(changed.attempted, 1);
    assert_eq!(changed.delivered, 1);

    let schema = serde_json::json!({
        "$version": 1,
        "type": "object",
        "properties": {
            "mode": {
                "type": "string",
                "enum": ["quiet", "loud"],
                "default": "quiet",
                "x-napplet-section": "appearance"
            },
            "enabled": {"type": "boolean", "default": true}
        },
        "additionalProperties": false
    });
    controller.mapped_envelope(
        session,
        serde_json::to_vec(&serde_json::json!({
            "type": "config.registerSchema",
            "id": "schema-1",
            "schema": schema,
            "version": 1
        }))
        .unwrap(),
    );
    assert_eq!(
        response_of_type(&controller, "config.registerSchema.result")["ok"],
        true
    );
    controller.mapped_envelope(session, br#"{"type":"config.subscribe"}"#.to_vec());
    controller.mapped_envelope(
        session,
        br#"{"type":"config.openSettings","section":"appearance"}"#.to_vec(),
    );
    let request = requests.lock().pop().expect("native settings request");
    assert_eq!(request.manifest_author, AUTHOR);
    assert_eq!(request.d_tag, "good-morning");
    assert_eq!(request.session_id, session);
    assert_eq!(request.section.as_deref(), Some("appearance"));
    assert!(request.schema_json.len() <= 192 * 1_024);
    assert!(request.values_json.len() <= 192 * 1_024);

    let commit = controller.commit_config_values(NativeConfigCommit {
        manifest_author: request.manifest_author,
        d_tag: request.d_tag,
        aggregate_hash: request.aggregate_hash,
        session_id: request.session_id,
        values_json: r#"{"enabled":false,"mode":"loud"}"#.to_owned(),
    });
    assert!(commit.accepted);
    assert_eq!(commit.attempted, 1);
    assert_eq!(commit.delivered, 1);
    controller.mapped_envelope(session, br#"{"type":"config.get","id":"get-1"}"#.to_vec());
    assert_eq!(
        response_of_type(&controller, "config.values")["values"],
        serde_json::json!({"enabled": false, "mode": "loud"})
    );

    controller.stop(session);
    let refused = controller.commit_config_values(NativeConfigCommit {
        manifest_author: AUTHOR.to_owned(),
        d_tag: "good-morning".to_owned(),
        aggregate_hash: "828a6df02afd56782ea20f805084acce65c53f7c37554948c1e0a64aa5a2b0a8"
            .to_owned(),
        session_id: session,
        values_json: r#"{"enabled":true,"mode":"quiet"}"#.to_owned(),
    });
    assert!(!refused.accepted);
    assert_eq!(refused.refusal.unwrap().code, "settings-session-closed");

    controller.close();
    drop(controller);
    let reopened = controller_with_native_capabilities(&temp, Arc::new(Mutex::new(Vec::new())));
    let (_, reopened_session) = install_and_launch(&reopened, &["config"]);
    reopened.mapped_envelope(
        reopened_session,
        br#"{"type":"config.get","id":"get-after-restart"}"#.to_vec(),
    );
    assert_eq!(
        response_of_type(&reopened, "config.values")["values"],
        serde_json::json!({"enabled": false, "mode": "loud"})
    );
}

#[test]
fn native_inc_actions_cross_ffi_with_trusted_origin_and_teardown() {
    let temp = TempDir::new().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let ends = Arc::new(Mutex::new(Vec::new()));
    let controller = controller_with_all_native_capabilities(
        &temp,
        Arc::clone(&requests),
        Arc::clone(&ends),
        NativeIncActionEnqueueResult::Accepted,
    );
    let (_, session) = install_and_launch(&controller, &["inc"]);
    controller.mapped_envelope(
        session,
        serde_json::to_vec(&serde_json::json!({
            "type": "inc.emit",
            "topic": "profile:open",
            "payload": {"pubkey": AUTHOR}
        }))
        .unwrap(),
    );
    let request = requests.lock().pop().expect("native action request");
    assert_eq!(request.manifest_author, AUTHOR);
    assert_eq!(request.d_tag, "good-morning");
    assert_eq!(request.session_id, session);
    assert_eq!(request.source_window_id, session);
    assert_eq!(request.kind, "profile-open");
    assert_eq!(
        serde_json::from_str::<Value>(&request.payload_json).unwrap(),
        serde_json::json!({"pubkey": AUTHOR})
    );

    controller.stop(session);
    let end = ends.lock().pop().expect("session teardown callback");
    assert_eq!(end.session_id, session);
    assert!(end.reason.starts_with("closed-"));
}

#[test]
fn native_inc_action_backpressure_is_an_exact_provider_refusal() {
    let temp = TempDir::new().unwrap();
    let controller = controller_with_all_native_capabilities(
        &temp,
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(Mutex::new(Vec::new())),
        NativeIncActionEnqueueResult::Backpressure,
    );
    let (_, session) = install_and_launch(&controller, &["inc"]);
    controller.mapped_envelope(
        session,
        serde_json::to_vec(&serde_json::json!({
            "type": "inc.emit",
            "topic": "profile:open",
            "payload": {"pubkey": AUTHOR}
        }))
        .unwrap(),
    );
    let error = controller
        .snapshot()
        .recent_errors
        .into_iter()
        .last()
        .expect("provider refusal fact");
    assert_eq!(error.code, "bridge");
    assert!(
        error.detail.contains("native action capacity is full"),
        "unexpected refusal detail: {}",
        error.detail
    );
}
