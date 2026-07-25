//! Private surface bindings and approved-write acceptance.

use std::sync::Arc;

use nmp_native_runtime_core::{
    ApprovedWrite, BindingRequest, ReceiptEventSink, ReceiptReattachment, WriteReceiptId,
};
use nmp_native_surface::Binding;

use super::{AppState, BindingOwner, RuntimeApp};
use crate::activity::ActivityDetail;
use crate::{
    commands::{PlatformEvent, ProviderOperationId},
    receipt::{AppReceipt, ReceiptFanout},
    views::AppErrorCode,
};

impl RuntimeApp {
    pub(super) fn open_binding(&self, state: &mut AppState, request: BindingRequest, now: u64) {
        if state.bindings.contains_key(&request.workspace_binding_id) {
            self.refuse(
                state,
                AppErrorCode::Binding,
                None,
                None,
                "workspace binding id is already open",
                now,
            );
            return;
        }
        if state.bindings.len() >= self.limits.maximum_bindings {
            self.refuse(
                state,
                AppErrorCode::Capacity,
                None,
                None,
                "binding capacity is full",
                now,
            );
            return;
        }
        let binding = match Binding::new(
            Arc::clone(&request.workspace_binding_id),
            Arc::clone(&request.schema),
            self.binding_limits,
        ) {
            Ok(binding) => binding,
            Err(error) => {
                self.refuse_binding(state, error, now);
                return;
            }
        };
        let source = match self
            .data_plane
            .open_binding(request.clone(), binding.clone())
        {
            Ok(source) => source,
            Err(error) => {
                self.refuse(
                    state,
                    AppErrorCode::HostData,
                    None,
                    None,
                    error.to_string(),
                    now,
                );
                return;
            }
        };
        let logical_source_id: Arc<str> = Arc::from(source.logical_id());
        if let Err(error) = binding.attach_source(source) {
            self.refuse_binding(state, error, now);
            return;
        }
        let binding_id = Arc::clone(&request.workspace_binding_id);
        state
            .bindings
            .insert(Arc::clone(&binding_id), BindingOwner { request, binding });
        self.push_event(
            state,
            PlatformEvent::BindingOpened {
                binding_id,
                logical_source_id,
            },
        );
    }

    pub(super) fn close_binding(&self, state: &mut AppState, binding_id: &Arc<str>, now: u64) {
        let Some(owner) = state.bindings.remove(binding_id) else {
            self.refuse(
                state,
                AppErrorCode::Binding,
                None,
                None,
                "unknown binding",
                now,
            );
            return;
        };
        owner.binding.close();
        self.push_event(
            state,
            PlatformEvent::BindingClosed {
                binding_id: Arc::clone(binding_id),
            },
        );
    }

    pub(super) fn approve_write(&self, state: &mut AppState, write: ApprovedWrite, now: u64) {
        self.accept_approved_write(state, write, None, now);
    }

    pub(super) fn decide_provider_write(
        &self,
        state: &mut AppState,
        operation_id: ProviderOperationId,
        approve: bool,
        now: u64,
    ) {
        let Some(mut operation) = state.operations.remove(&operation_id) else {
            self.refuse(
                state,
                AppErrorCode::Bridge,
                None,
                None,
                "unknown provider write proposal",
                now,
            );
            return;
        };
        let Some(proposal) = operation.proposal.take() else {
            let principal = operation.principal.clone();
            let session = operation.session;
            operation.complete();
            self.refuse(
                state,
                AppErrorCode::Bridge,
                Some(principal),
                Some(session),
                "provider operation is not awaiting a write decision",
                now,
            );
            return;
        };
        if !approve {
            proposal.refuse(Arc::from("native approval was denied"));
            if let Some(handle) = operation.handle {
                handle.cancel();
            }
            self.push_event(
                state,
                PlatformEvent::ProviderOperationFinished {
                    operation: operation_id,
                },
            );
            return;
        }
        let (write, completion, work) = proposal.into_parts();
        let provider_sink = completion.into_receipt_sink();
        self.accept_approved_write(state, write, Some(provider_sink), now);
        drop(work);
        self.push_event(
            state,
            PlatformEvent::ProviderOperationFinished {
                operation: operation_id,
            },
        );
    }

    pub(super) fn accept_approved_write(
        &self,
        state: &mut AppState,
        write: ApprovedWrite,
        provider_sink: Option<Arc<dyn ReceiptEventSink>>,
        now: u64,
    ) {
        let Some(session) = state.sessions.get(&write.origin_session) else {
            if let Some(sink) = provider_sink.as_ref() {
                sink.close(Some(Arc::from("origin session is no longer active")));
            }
            self.refuse(
                state,
                AppErrorCode::UnknownSession,
                Some(write.origin_principal),
                Some(write.origin_session),
                "write approval names a stale or stopped origin session",
                now,
            );
            return;
        };
        if session.context.principal != write.origin_principal {
            if let Some(sink) = provider_sink.as_ref() {
                sink.close(Some(Arc::from("origin session identity changed")));
            }
            self.refuse(
                state,
                AppErrorCode::SessionIdentityMismatch,
                Some(write.origin_principal),
                Some(write.origin_session),
                "write approval principal does not match the fixed origin session",
                now,
            );
            return;
        }
        if state.receipts.len() >= self.limits.maximum_receipts {
            if let Some(sink) = provider_sink.as_ref() {
                sink.close(Some(Arc::from("receipt ownership capacity is full")));
            }
            self.refuse(
                state,
                AppErrorCode::Capacity,
                Some(write.origin_principal),
                Some(write.origin_session),
                "receipt ownership capacity is full before write acceptance",
                now,
            );
            return;
        }
        let principal = write.origin_principal.clone();
        let origin_session = write.origin_session;
        let expected_account = write.account.clone();
        let receipt = Arc::new(AppReceipt::unassigned(
            self.limits.maximum_receipt_frame_bytes,
        ));
        let receipt_sink: Arc<dyn ReceiptEventSink> = match provider_sink {
            Some(provider) => Arc::new(ReceiptFanout {
                app: Arc::clone(&receipt),
                provider,
            }),
            None => receipt.clone(),
        };
        let accepted = match self.data_plane.accept_write(write, receipt_sink.clone()) {
            Ok(accepted) => accepted,
            Err(error) => {
                receipt_sink.close(Some(Arc::from(error.to_string())));
                self.refuse(
                    state,
                    AppErrorCode::HostData,
                    Some(principal),
                    Some(origin_session),
                    error.to_string(),
                    now,
                );
                return;
            }
        };
        if let Err(detail) = receipt.assign(accepted.receipt_id.clone()) {
            receipt_sink.close(Some(Arc::clone(&detail)));
            self.refuse(
                state,
                AppErrorCode::Receipt,
                Some(principal.clone()),
                Some(origin_session),
                detail,
                now,
            );
        }
        if accepted.frozen_account != expected_account {
            receipt_sink.close(Some(Arc::from(
                "host data plane returned a different frozen account",
            )));
            self.refuse(
                state,
                AppErrorCode::Receipt,
                Some(principal.clone()),
                Some(origin_session),
                "host data plane returned a different frozen account",
                now,
            );
        }
        state.receipts.insert(accepted.receipt_id.clone(), receipt);
        // The runtime classifies each detail where it produces it. The receipt
        // id and the frozen account are runtime-owned identifiers and are safe
        // to display. The approved draft is component-authored content the
        // user reviewed once in the approval sheet; the runtime does not
        // republish it into the activity surface, so it is classified secret
        // and its bytes stop here rather than travelling and being filtered
        // later.
        self.record_activity_with_details(
            state,
            &principal,
            "write",
            "accept",
            "durable-obligation",
            vec![
                ActivityDetail::visible("receipt-id", accepted.receipt_id.0.as_ref()),
                ActivityDetail::visible("frozen-account", accepted.frozen_account.0.as_ref()),
                ActivityDetail::secret("approved-draft"),
            ],
            now,
        );
        self.push_event(
            state,
            PlatformEvent::WriteAccepted {
                receipt_id: accepted.receipt_id,
                frozen_account: accepted.frozen_account,
            },
        );
    }

    pub(super) fn reattach_receipt(
        &self,
        state: &mut AppState,
        receipt_id: WriteReceiptId,
        now: u64,
    ) {
        let receipt = Arc::new(AppReceipt::assigned(
            receipt_id.clone(),
            self.limits.maximum_receipt_frame_bytes,
        ));
        match self
            .data_plane
            .reattach_receipt(receipt_id.clone(), receipt.clone())
        {
            Ok(ReceiptReattachment::Attached(observation)) => {
                if observation.receipt_id() != &receipt_id {
                    observation.stop_delivery();
                    receipt.set_closed();
                    self.refuse(
                        state,
                        AppErrorCode::Receipt,
                        None,
                        None,
                        "receipt observation identity mismatch",
                        now,
                    );
                } else {
                    receipt.attach_observation(observation);
                    state.receipts.insert(receipt_id.clone(), receipt);
                    self.push_event(state, PlatformEvent::ReceiptReattached { receipt_id });
                }
            }
            Ok(ReceiptReattachment::NotFound) => {
                receipt.set_not_found();
                state.receipts.insert(receipt_id.clone(), receipt);
                self.push_event(state, PlatformEvent::ReceiptNotFound { receipt_id });
            }
            Err(error) => {
                self.refuse(
                    state,
                    AppErrorCode::HostData,
                    None,
                    None,
                    error.to_string(),
                    now,
                );
            }
        }
    }
}
