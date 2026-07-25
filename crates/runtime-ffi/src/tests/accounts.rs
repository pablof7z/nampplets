use super::*;

#[test]
fn local_account_lifecycle_is_explicit_typed_and_stale_safe() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp);

    let invalid = controller.register_local_account("not-a-secret".to_owned());
    assert!(!invalid.accepted);
    assert_eq!(
        invalid.failure,
        Some(RuntimeAccountFailure::InvalidSecretKey)
    );

    let registered = controller.register_local_account(format!("{:064x}", 7_u8));
    assert!(registered.accepted);
    let first = registered.handle.unwrap();
    assert_eq!(first.kind, RuntimeAccountKind::LocalSigner);
    assert_eq!(
        registered.snapshot.unwrap().active_public_key,
        None,
        "registration must not silently switch identity"
    );

    let activated = controller.activate_local_account(first.clone());
    assert!(activated.accepted);
    assert_eq!(
        activated.snapshot.unwrap().active_public_key.as_deref(),
        Some(first.public_key.as_str())
    );

    let replacement = controller
        .register_local_account(format!("{:064x}", 7_u8))
        .handle
        .unwrap();
    assert_ne!(first.installation_id, replacement.installation_id);
    let stale = controller.remove_local_account(first);
    assert!(!stale.accepted);
    assert_eq!(
        stale.failure,
        Some(RuntimeAccountFailure::StaleInstallation)
    );

    let activated = controller.activate_local_account(replacement.clone());
    assert!(activated.accepted);
    assert_eq!(
        controller
            .logout_local_account()
            .snapshot
            .unwrap()
            .active_public_key,
        None
    );
    let removed = controller.remove_local_account(replacement);
    assert!(removed.accepted);
    assert!(removed.snapshot.unwrap().local_accounts.is_empty());

    controller.close();
    assert_eq!(
        controller.account_snapshot().failure,
        Some(RuntimeAccountFailure::Closed)
    );
}

#[test]
fn read_only_account_lifecycle_is_keyless_typed_and_explicit() {
    let temp = TempDir::new().unwrap();
    let controller = controller(&temp);
    let npub = "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6";

    let registered = controller.register_read_only_account(npub.to_owned());
    assert!(registered.accepted);
    let handle = registered.handle.unwrap();
    assert_eq!(handle.kind, RuntimeAccountKind::ReadOnly);
    assert_eq!(handle.public_key.len(), 64);
    assert_eq!(
        registered.snapshot.unwrap().active_public_key,
        None,
        "read-only registration must not silently switch identity"
    );

    let activated = controller.activate_local_account(handle.clone());
    assert!(activated.accepted);
    assert_eq!(
        activated.snapshot.unwrap().active_public_key.as_deref(),
        Some(handle.public_key.as_str())
    );
    assert_eq!(
        controller
            .logout_local_account()
            .snapshot
            .unwrap()
            .active_public_key,
        None
    );
    assert!(controller.remove_local_account(handle).accepted);
    assert!(
        controller
            .account_snapshot()
            .snapshot
            .unwrap()
            .local_accounts
            .is_empty()
    );

    let nip05 = controller.register_read_only_account("pablo@example.com".to_owned());
    assert_eq!(
        nip05.failure,
        Some(RuntimeAccountFailure::Nip05ResolutionUnavailable)
    );
    let invalid = controller.register_read_only_account("not-a-key".to_owned());
    assert_eq!(
        invalid.failure,
        Some(RuntimeAccountFailure::InvalidPublicKey)
    );
}
