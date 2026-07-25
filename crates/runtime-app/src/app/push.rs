//! Provider-to-component push delivery and provider operation completion.

use std::{
    sync::{Arc, Weak},
    thread,
};

use nmp_native_nap_bridge::{
    ProviderPush, ProviderPushBatch, ProviderPushError, ProviderPushObserver,
    ProviderPushTermination, SourceWindowId,
};
use nmp_native_runtime_core::{ResourceClass, SessionId, SessionState, WorkLease};

use super::{AppState, ProviderPushDelivery, RuntimeApp};
use crate::{
    commands::{PlatformEvent, ProviderOperationId},
    views::AppErrorCode,
};

impl RuntimeApp {
    pub(super) fn activate_push_delivery(
        self: &Arc<Self>,
        state: &mut AppState,
        session_id: SessionId,
    ) -> Result<(), Arc<str>> {
        let Some(entry) = state.sessions.get(&session_id) else {
            return Err(Arc::from("provider delivery session is no longer active"));
        };
        if entry.ready {
            return Ok(());
        }
        self.bridge
            .mark_session_ready(session_id)
            .map_err(|error| Arc::from(error.to_string()))?;
        let delivery_lease = self
            .resources
            .admit(session_id, None, ResourceClass::StateDelivery)
            .map_err(|error| Arc::from(error.to_string()))?;
        let entry = state
            .sessions
            .get_mut(&session_id)
            .expect("session was validated while holding the app lock");
        let observer = entry
            .push_observer
            .take()
            .ok_or_else(|| Arc::from("provider delivery observer is unavailable"))?;
        let source_window = entry.source_window;
        entry.ready = true;
        let app = Arc::downgrade(self);
        let maximum_batch = self.limits.maximum_provider_push_batch;
        let join = thread::Builder::new()
            .name(format!("nap-push-{}", session_id.0))
            .spawn(move || {
                run_provider_push_delivery(
                    app,
                    observer,
                    delivery_lease,
                    session_id,
                    source_window,
                    maximum_batch,
                );
            })
            .map_err(|error| Arc::from(error.to_string()))?;
        entry.push_delivery = Some(ProviderPushDelivery { join: Some(join) });
        Ok(())
    }

    pub(super) fn ingest_provider_push_batch(
        &self,
        session_id: SessionId,
        source_window: SourceWindowId,
        batch: ProviderPushBatch,
    ) -> bool {
        let now = self.clock.now_millis();
        let mut state = self.state.lock();
        let Some(entry) = state.sessions.get_mut(&session_id) else {
            return false;
        };
        if entry.source_window != source_window || !entry.ready {
            let principal = entry.context.principal.clone();
            self.refuse(
                &mut state,
                AppErrorCode::SessionIdentityMismatch,
                Some(principal),
                Some(session_id),
                "provider push source no longer matches the ready mapped session",
                now,
            );
            let _ = self.end_session(
                &mut state,
                session_id,
                SessionState::Crashed,
                Some(Arc::from("provider push source mismatch")),
                now,
            );
            self.publish(&mut state);
            return false;
        }

        let principal = entry.context.principal.clone();
        let domains = entry.plan.domains().clone();
        let mut accepted = Vec::with_capacity(batch.pushes.len());
        let mut invalid = None;
        for push in batch.pushes {
            if push.session != session_id
                || push.source_window != source_window
                || !domains.contains(&push.domain)
                || entry
                    .last_provider_sequence
                    .is_some_and(|sequence| push.sequence <= sequence)
            {
                invalid =
                    Some("provider push violated its fixed session, source, domain, or sequence");
                break;
            }
            if push.domain.as_str() != "shell"
                && !self
                    .grants
                    .decision(&principal, &push.domain)
                    .allows_without_prompt()
            {
                continue;
            }
            entry.last_provider_sequence = Some(push.sequence);
            entry.delivered_push_count = entry.delivered_push_count.saturating_add(1);
            accepted.push(push);
        }
        let closed = batch.closed;
        let termination = batch.termination;
        for push in accepted {
            self.project_provider_push(&mut state, push);
        }
        if let Some(detail) = invalid {
            self.refuse(
                &mut state,
                AppErrorCode::SessionIdentityMismatch,
                Some(principal),
                Some(session_id),
                detail,
                now,
            );
            self.push_event(
                &mut state,
                PlatformEvent::ProviderPushLaneClosed {
                    session: session_id,
                    source_window,
                    termination: Some(ProviderPushTermination::ProviderFailure),
                },
            );
            let _ = self.end_session(
                &mut state,
                session_id,
                SessionState::Crashed,
                Some(Arc::from("invalid provider push routing")),
                now,
            );
            self.publish(&mut state);
            return false;
        }
        if closed {
            self.push_event(
                &mut state,
                PlatformEvent::ProviderPushLaneClosed {
                    session: session_id,
                    source_window,
                    termination,
                },
            );
            let reason = match termination {
                Some(ProviderPushTermination::Backpressure) => {
                    "provider push lane terminated by backpressure"
                }
                Some(ProviderPushTermination::ProviderFailure) => {
                    "provider push lane terminated by provider failure"
                }
                None => "provider push lane closed unexpectedly",
            };
            let _ = self.end_session(
                &mut state,
                session_id,
                SessionState::Crashed,
                Some(Arc::from(reason)),
                now,
            );
            self.publish(&mut state);
            return false;
        }
        self.publish(&mut state);
        true
    }

    pub(super) fn provider_push_observation_failed(
        &self,
        session_id: SessionId,
        source_window: SourceWindowId,
        error: ProviderPushError,
    ) {
        let now = self.clock.now_millis();
        let mut state = self.state.lock();
        let Some(entry) = state.sessions.get(&session_id) else {
            return;
        };
        if entry.source_window != source_window {
            return;
        }
        let principal = entry.context.principal.clone();
        self.refuse(
            &mut state,
            AppErrorCode::Bridge,
            Some(principal),
            Some(session_id),
            error.to_string(),
            now,
        );
        self.push_event(
            &mut state,
            PlatformEvent::ProviderPushLaneClosed {
                session: session_id,
                source_window,
                termination: Some(ProviderPushTermination::ProviderFailure),
            },
        );
        let _ = self.end_session(
            &mut state,
            session_id,
            SessionState::Crashed,
            Some(Arc::from("provider push observation failed")),
            now,
        );
        self.publish(&mut state);
    }

    pub(super) fn project_provider_push(&self, state: &mut AppState, push: ProviderPush) {
        self.push_event(
            state,
            PlatformEvent::ProviderPush {
                session: push.session,
                source_window: push.source_window,
                provider_sequence: push.sequence,
                domain: push.domain,
                envelope: push.envelope,
            },
        );
    }

    pub(super) fn complete_operation(
        &self,
        state: &mut AppState,
        operation_id: ProviderOperationId,
        now: u64,
    ) {
        let Some(operation) = state.operations.remove(&operation_id) else {
            self.refuse(
                state,
                AppErrorCode::Bridge,
                None,
                None,
                "unknown provider operation",
                now,
            );
            return;
        };
        if operation.proposal.is_some() {
            operation.cancel(Arc::from("pending write requires an approval decision"));
            self.refuse(
                state,
                AppErrorCode::Bridge,
                None,
                None,
                "a pending provider write cannot be completed without approval",
                now,
            );
            return;
        }
        operation.complete();
        self.push_event(
            state,
            PlatformEvent::ProviderOperationFinished {
                operation: operation_id,
            },
        );
    }
}

pub(super) fn run_provider_push_delivery(
    app: Weak<RuntimeApp>,
    mut observer: ProviderPushObserver,
    delivery_lease: WorkLease,
    session_id: SessionId,
    source_window: SourceWindowId,
    maximum_batch: usize,
) {
    let mut delivery_lease = Some(delivery_lease);
    let runtime = match tokio::runtime::Builder::new_current_thread().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            drop(delivery_lease.take());
            if let Some(app) = app.upgrade() {
                app.provider_push_observation_failed(
                    session_id,
                    source_window,
                    ProviderPushError::Malformed(Arc::from(error.to_string())),
                );
            }
            return;
        }
    };
    runtime.block_on(async {
        loop {
            match observer.changed(maximum_batch).await {
                Ok(batch) => {
                    if batch.closed {
                        drop(delivery_lease.take());
                    }
                    let Some(app) = app.upgrade() else {
                        break;
                    };
                    let closed = batch.closed;
                    if !app.ingest_provider_push_batch(session_id, source_window, batch) || closed {
                        break;
                    }
                }
                Err(ProviderPushError::Closed) => break,
                Err(error) => {
                    drop(delivery_lease.take());
                    if let Some(app) = app.upgrade() {
                        app.provider_push_observation_failed(session_id, source_window, error);
                    }
                    break;
                }
            }
        }
    });
}
