use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use nmp_native_nap_bridge::{
    ProviderCall, ProviderDescriptor, ProviderError, ProviderPlatformAvailability,
    ProviderPushSender, ProviderRequest,
};
use nmp_native_runtime_core::{AccountRef, ApprovedWrite, Capability, Principal, SessionId};
use parking_lot::Mutex;
use thiserror::Error;

use crate::{
    DOMAIN, ListMutation, ListReadLimits, ListSelector, ListSnapshot, ListsDataError,
    ListsDataPlane, ListsProviderLimits, PINNED_NAP_PROTOCOL,
    validate::{
        ListRefusal, apply_add, apply_remove, parse_items, parse_selector, selector_value,
        validate_limits,
    },
    wire::{
        completed, correlation_id, denied, exact_payload, failed, mutation_result, refusal_result,
        supported_result,
    },
    write::{ListsAction, ListsWriteCompletion},
};

#[derive(Debug)]
pub struct ListsProvider {
    pub(crate) source: Arc<dyn ListsDataPlane>,
    pub(crate) limits: ListsProviderLimits,
    pub(crate) descriptor: ProviderDescriptor,
    pub(crate) state: Mutex<ListsState>,
}

#[derive(Debug, Default)]
pub(crate) struct ListsState {
    pub(crate) sessions: BTreeMap<SessionId, ListsSession>,
    pub(crate) closed: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ListsSession {
    pub(crate) principal: Principal,
    pub(crate) outbound: ProviderPushSender,
}

impl ListsProvider {
    pub fn new(
        source: Arc<dyn ListsDataPlane>,
        limits: ListsProviderLimits,
    ) -> Result<Arc<Self>, ListsProviderBuildError> {
        if !validate_limits(limits) {
            return Err(ListsProviderBuildError::InvalidLimits);
        }
        Ok(Arc::new(Self {
            source,
            limits,
            descriptor: ProviderDescriptor {
                domain: Capability::new(DOMAIN).expect("static lists capability is valid"),
                protocol_versions: BTreeSet::from([Arc::from(PINNED_NAP_PROTOCOL)]),
                actions: ["supported", "add", "remove"]
                    .into_iter()
                    .map(Arc::from)
                    .collect(),
                // Follow, mute and block membership is social-graph data, and
                // these actions change it under the user's own key.
                sensitive: true,
                dependencies: BTreeSet::new(),
                platform_availability: ProviderPlatformAvailability::Available,
            },
            state: Mutex::new(ListsState::default()),
        }))
    }

    pub fn close(&self) {
        let mut state = self.state.lock();
        state.closed = true;
        state.sessions.clear();
    }

    pub fn active_sessions(&self) -> usize {
        self.state.lock().sessions.len()
    }

    fn session_outbound(
        &self,
        request: &ProviderRequest,
    ) -> Result<ProviderPushSender, ProviderError> {
        let state = self.state.lock();
        let session = state
            .sessions
            .get(&request.session)
            .ok_or_else(|| denied(request, "provider session is not open"))?;
        if session.principal != request.principal {
            return Err(denied(request, "provider session principal does not match"));
        }
        Ok(session.outbound.clone())
    }

    pub(crate) fn supported(
        &self,
        request: ProviderRequest,
    ) -> Result<ProviderCall, ProviderError> {
        let id = correlation_id(&request, self.limits)?;
        exact_payload(&request, &[])?;
        completed(&supported_result(&id), self.limits, &request.action)
    }

    /// One `add`/`remove`. Every decision — which list, which items, what the
    /// resulting set is, and whether a write is needed at all — is made here
    /// before anything is proposed for approval.
    pub(crate) fn mutate(
        &self,
        action: ListsAction,
        request: ProviderRequest,
    ) -> Result<ProviderCall, ProviderError> {
        let id = correlation_id(&request, self.limits)?;
        let outbound = self.session_outbound(&request)?;
        let payload = exact_payload(&request, &["list", "items"])?;

        let refuse = |refusal: &ListRefusal| {
            completed(
                &refusal_result(action, &id, refusal),
                self.limits,
                &request.action,
            )
        };

        let (supported, selector) = match parse_selector(payload.get("list"), self.limits) {
            Ok(parsed) => parsed,
            Err(refusal) => return refuse(&refusal),
        };
        let items = match parse_items(payload.get("items"), supported, self.limits) {
            Ok(items) => items,
            Err(refusal) => return refuse(&refusal),
        };
        let account = match self.source.freeze_account() {
            Ok(Some(account)) => account,
            Ok(None) => return refuse(&ListRefusal::NoAccount),
            Err(error) => return Err(failed(&request.action, error.to_string())),
        };
        if request.work.cancellation().is_cancelled() {
            return Err(failed(&request.action, "the request was cancelled"));
        }
        let snapshot = match self.source.read_list(
            &account,
            &selector,
            request.work.cancellation(),
            ListReadLimits {
                maximum_entries: self.limits.maximum_list_entries,
                maximum_frame_bytes: self.limits.maximum_draft_bytes,
            },
        ) {
            Ok(snapshot) => snapshot,
            Err(error) => return Err(failed(&request.action, error.to_string())),
        };
        let mutation = match action {
            ListsAction::Add => match apply_add(&snapshot.entries, &items, self.limits) {
                Ok(mutation) => mutation,
                Err(refusal) => return refuse(&refusal),
            },
            ListsAction::Remove => apply_remove(&snapshot.entries, &items),
        };
        if mutation.is_noop() {
            // Nothing changes, so nothing is written. Republishing an
            // identical list would burn a durable write and move the
            // replaceable event's timestamp for no reason.
            return completed(
                &mutation_result(action, &id, 0, mutation.skipped),
                self.limits,
                &request.action,
            );
        }
        self.propose(
            action, id, request, account, selector, snapshot, mutation, outbound,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn propose(
        &self,
        action: ListsAction,
        id: Arc<str>,
        request: ProviderRequest,
        account: AccountRef,
        selector: ListSelector,
        snapshot: ListSnapshot,
        mutation: ListMutation,
        outbound: ProviderPushSender,
    ) -> Result<ProviderCall, ProviderError> {
        let draft = self
            .source
            .draft_replacement(
                &account,
                &selector,
                &snapshot,
                &mutation.entries,
                self.limits.maximum_draft_bytes,
            )
            .map_err(|error| match error {
                ListsDataError::DraftTooLarge => {
                    failed(&request.action, ListsDataError::DraftTooLarge.to_string())
                }
                error => failed(&request.action, error.to_string()),
            })?;
        let write = ApprovedWrite {
            approval_id: approval_id(action, &selector, &id),
            origin_principal: request.principal.clone(),
            origin_session: request.session,
            account,
            draft,
        };
        let completion = Box::new(ListsWriteCompletion {
            action,
            id,
            changed: mutation.changed,
            skipped: mutation.skipped,
            outbound,
            maximum_response_bytes: self.limits.maximum_response_bytes,
        });
        Ok(ProviderCall::proposed_write(
            None,
            write,
            completion,
            request.work,
        ))
    }
}

/// Names the approval after the exact list it changes, so a native reviewer
/// sees which list is at stake rather than an opaque id.
fn approval_id(action: ListsAction, selector: &ListSelector, id: &str) -> Arc<str> {
    let selector = selector_value(selector);
    let identifier = selector
        .get("identifier")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    Arc::from(format!(
        "lists.{}:{}:{}:{}",
        match action {
            ListsAction::Add => "add",
            ListsAction::Remove => "remove",
        },
        selector
            .get("kind")
            .map(ToString::to_string)
            .unwrap_or_default(),
        identifier,
        id
    ))
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ListsProviderBuildError {
    #[error("lists provider limits must be finite and non-zero")]
    InvalidLimits,
}
