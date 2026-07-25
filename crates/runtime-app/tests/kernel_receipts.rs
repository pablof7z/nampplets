//! Durable write acceptance and receipt reattachment across restarts.

mod support;

use std::sync::Arc;

use nmp_native_runtime_app::{AppErrorCode, PlatformCommand};
use nmp_native_runtime_core::{AccountRef, ApprovedWrite};
use nmp_native_runtime_store::WorkspaceRecord;
use support::*;

#[test]
fn accepted_durable_receipt_outlives_origin_session_and_keeps_frozen_account() {
    let rig = Rig::new(false);
    let principal = principal('b');
    rig.install(principal.clone());
    rig.allow_runtime(principal.clone());
    let session = rig.launch(principal.clone());
    let account = AccountRef(Arc::from("account-a"));

    rig.app.dispatch(PlatformCommand::ApproveWrite {
        write: ApprovedWrite {
            approval_id: Arc::from("approval-1"),
            origin_principal: principal,
            origin_session: session,
            account: account.clone(),
            draft: json(serde_json::json!({
                "author": "account-a",
                "kind": 1,
                "content": "hello"
            })),
        },
    });
    let receipt_id = rig.app.snapshot().receipts[0].receipt_id.clone();
    assert_eq!(rig.host.receipt_count(), 1);
    assert_eq!(
        rig.app
            .receipt(&receipt_id)
            .unwrap()
            .view()
            .unwrap()
            .latest
            .unwrap()
            .state
            .decode()
            .unwrap()["stage"],
        "accepted"
    );

    rig.app.dispatch(PlatformCommand::Stop { session });
    assert_eq!(rig.app.snapshot().resources.admitted, 0);
    assert!(
        rig.app.receipt(&receipt_id).is_some(),
        "receipt ownership belongs to the application, not its origin session"
    );
    let accepted_event = rig
        .app
        .events_after(0)
        .events
        .into_iter()
        .find_map(|item| match item.event {
            nmp_native_runtime_app::PlatformEvent::WriteAccepted {
                receipt_id,
                frozen_account,
            } => Some((receipt_id, frozen_account)),
            _ => None,
        })
        .unwrap();
    assert_eq!(accepted_event.0, receipt_id);
    assert_eq!(accepted_event.1, account);
}

#[test]
fn write_with_caller_selected_principal_is_refused_before_acceptance() {
    let rig = Rig::new(false);
    let real = principal('b');
    let forged = principal('c');
    rig.install(real.clone());
    rig.allow_runtime(real.clone());
    let session = rig.launch(real);

    rig.app.dispatch(PlatformCommand::ApproveWrite {
        write: ApprovedWrite {
            approval_id: Arc::from("forged-approval"),
            origin_principal: forged,
            origin_session: session,
            account: AccountRef(Arc::from("account-a")),
            draft: json(serde_json::json!({"kind": 1, "content": "forged"})),
        },
    });
    assert_eq!(rig.host.receipt_count(), 0);
    assert!(rig.app.snapshot().receipts.is_empty());
    assert_eq!(
        rig.app.snapshot().recent_errors.last().unwrap().code,
        AppErrorCode::SessionIdentityMismatch
    );
}

#[test]
fn restoration_reattaches_only_explicit_receipt_ids_not_workspace_json() {
    let rig = Rig::new(false);
    let principal = principal('b');
    rig.install(principal.clone());
    rig.allow_runtime(principal.clone());
    let session = rig.launch(principal.clone());
    rig.app.dispatch(PlatformCommand::ApproveWrite {
        write: ApprovedWrite {
            approval_id: Arc::from("approval-1"),
            origin_principal: principal,
            origin_session: session,
            account: AccountRef(Arc::from("account-a")),
            draft: json(serde_json::json!({"kind": 1, "content": "restore me"})),
        },
    });
    let retained = rig.app.snapshot().receipts[0].receipt_id.clone();
    let injected = WriteReceiptIdForTest::value();
    rig.app.dispatch(PlatformCommand::SaveWorkspace {
        workspace: WorkspaceRecord {
            id: Arc::from("main"),
            definition: json(serde_json::json!({
                "slots": ["feed"],
                "receipt_id": injected.0.as_ref()
            })),
            retained_receipts: vec![retained.clone()],
        },
    });
    rig.app.dispatch(PlatformCommand::Close);

    let (restored, _) = open_app(
        Arc::clone(&rig.store),
        rig.host.clone(),
        rig.provider.clone(),
    );
    restored.dispatch(PlatformCommand::RestoreWorkspaces);
    let snapshot = restored.snapshot();
    assert_eq!(snapshot.workspaces.len(), 1);
    assert_eq!(snapshot.receipts.len(), 1);
    assert_eq!(snapshot.receipts[0].receipt_id, retained);
    assert!(restored.receipt(&injected).is_none());
}
