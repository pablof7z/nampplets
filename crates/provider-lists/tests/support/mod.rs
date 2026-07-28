//! Shared harness for every `provider-lists` test target.
//!
//! The `#[test]` suites and the Cucumber scenarios in `bdd.rs` both drive
//! this one fixture, so a scenario and a unit test exercise identical
//! registration, session-binding and dispatch paths.

#![allow(dead_code)]

use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicUsize, Ordering},
};

use nmp_native_nap_bridge::{
    BridgeLimits, MemoryActivitySink, ProviderRegistry, SessionContext, SourceWindowId,
};
// Re-exported so every test target gets the same vocabulary from one `use
// support::*;` rather than repeating bridge imports per file.
pub use nmp_native_nap_bridge::{Provider, ProviderCall, ProviderPushObserver, ProviderRequest};
pub use nmp_native_runtime_core::{
    AccountRef, BoundedJson, Cancellation, Principal, ReceiptSnapshot, WriteReceiptId,
};
use nmp_native_runtime_core::{
    Capability, ExecutionProfile, GrantDecision, GrantLedger, GrantLimits, ResourceClass,
    ResourceLimits, ResourceTracker, Sensitivity, SessionId,
};
use parking_lot::Mutex;
use serde_json::{Value, json};
pub use std::sync::Arc;

use nmp_native_provider_lists::*;

/// A list source that records exactly what the provider asked it to do, so a
/// test can prove the provider read the right list and drafted the right
/// entries — not merely that it returned a plausible envelope.
#[derive(Debug)]
pub struct FakeSource {
    account: Mutex<Option<AccountRef>>,
    snapshot: Mutex<ListSnapshot>,
    reads: AtomicUsize,
    drafts: Mutex<Vec<Vec<ListEntry>>>,
    read_selectors: Mutex<Vec<ListSelector>>,
    read_error: Mutex<Option<ListsDataError>>,
    draft_error: Mutex<Option<ListsDataError>>,
}

impl FakeSource {
    pub fn new(entries: Vec<ListEntry>) -> Arc<Self> {
        Arc::new(Self {
            account: Mutex::new(Some(AccountRef(Arc::from("a".repeat(64))))),
            snapshot: Mutex::new(ListSnapshot {
                exists: true,
                entries,
                retained: retained(),
            }),
            reads: AtomicUsize::new(0),
            drafts: Mutex::new(Vec::new()),
            read_selectors: Mutex::new(Vec::new()),
            read_error: Mutex::new(None),
            draft_error: Mutex::new(None),
        })
    }

    /// Replaces the current list contents, for a scenario that states them
    /// after the provider is already open.
    pub fn set_entries(&self, entries: Vec<ListEntry>) {
        self.snapshot.lock().entries = entries;
    }

    pub fn sign_out(&self) {
        *self.account.lock() = None;
    }

    pub fn fail_reads(&self, error: ListsDataError) {
        *self.read_error.lock() = Some(error);
    }

    pub fn fail_drafts(&self, error: ListsDataError) {
        *self.draft_error.lock() = Some(error);
    }

    pub fn drafted(&self) -> Vec<Vec<ListEntry>> {
        self.drafts.lock().clone()
    }

    pub fn read_selectors(&self) -> Vec<ListSelector> {
        self.read_selectors.lock().clone()
    }

    pub fn reads(&self) -> usize {
        self.reads.load(Ordering::Relaxed)
    }
}

fn retained() -> BoundedJson {
    BoundedJson::from_value(&json!({"content": "", "otherTags": []}), 4096)
        .expect("retained fixture fits")
}

impl ListsDataPlane for FakeSource {
    fn freeze_account(&self) -> Result<Option<AccountRef>, ListsDataError> {
        Ok(self.account.lock().clone())
    }

    fn read_list(
        &self,
        _account: &AccountRef,
        selector: &ListSelector,
        cancellation: &Cancellation,
        _limits: ListReadLimits,
    ) -> Result<ListSnapshot, ListsDataError> {
        if let Some(error) = self.read_error.lock().clone() {
            return Err(error);
        }
        if cancellation.is_cancelled() {
            return Err(ListsDataError::Cancelled);
        }
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.read_selectors.lock().push(selector.clone());
        Ok(self.snapshot.lock().clone())
    }

    fn draft_replacement(
        &self,
        _account: &AccountRef,
        _selector: &ListSelector,
        _snapshot: &ListSnapshot,
        entries: &[ListEntry],
        maximum_draft_bytes: usize,
    ) -> Result<BoundedJson, ListsDataError> {
        if let Some(error) = self.draft_error.lock().clone() {
            return Err(error);
        }
        self.drafts.lock().push(entries.to_vec());
        BoundedJson::from_value(
            &json!({
                "entries": entries
                    .iter()
                    .map(|entry| json!([entry.tag.wire(), entry.value.as_ref()]))
                    .collect::<Vec<_>>(),
            }),
            maximum_draft_bytes,
        )
        .map_err(|_| ListsDataError::DraftTooLarge)
    }
}

pub fn principal() -> Principal {
    Principal::new("1".repeat(64), "profile", "2".repeat(64)).unwrap()
}

pub fn provider_with(entries: Vec<ListEntry>) -> (Arc<ListsProvider>, Arc<FakeSource>) {
    let source = FakeSource::new(entries);
    let erased: Arc<dyn ListsDataPlane> = source.clone();
    let provider = ListsProvider::new(erased, ListsProviderLimits::default()).unwrap();
    (provider, source)
}

/// Registers the provider on a real bridge so session binding, grants and the
/// outbound lane are the production ones rather than a test double.
pub fn opened_session(provider: Arc<ListsProvider>) -> (ProviderRegistry, ProviderPushObserver) {
    let resources = Arc::new(ResourceTracker::new(ResourceLimits::default()).unwrap());
    let grants =
        Arc::new(GrantLedger::new(GrantLimits::default(), Arc::clone(&resources)).unwrap());
    let activity = Arc::new(MemoryActivitySink::bounded(32));
    let mut registry = ProviderRegistry::new(
        BridgeLimits::default(),
        resources,
        Arc::clone(&grants),
        activity,
    )
    .unwrap();
    grants
        .set(
            principal(),
            Capability::new(DOMAIN).unwrap(),
            Sensitivity::Sensitive,
            GrantDecision::AllowExactBuild,
        )
        .unwrap();
    registry.register(provider).unwrap();
    let context = SessionContext {
        id: SessionId(7),
        principal: principal(),
        profile: ExecutionProfile::Legacy,
    };
    let plan = registry
        .negotiate(
            &context.principal,
            context.profile,
            &BTreeSet::from([Capability::new(DOMAIN).unwrap()]),
        )
        .unwrap();
    let observer = registry
        .open_session_bound(&context, &plan, SourceWindowId(11), 0)
        .unwrap();
    registry.mark_session_ready(context.id).unwrap();
    (registry, observer)
}

pub fn request(action: &str, payload: Value) -> ProviderRequest {
    let resources = ResourceTracker::new(ResourceLimits::default()).unwrap();
    let work = resources
        .admit(
            SessionId(7),
            Some(Capability::new(DOMAIN).unwrap()),
            ResourceClass::ProviderCall,
        )
        .unwrap();
    ProviderRequest {
        principal: principal(),
        session: SessionId(7),
        action: Arc::from(action),
        correlation_id: Some(Arc::from("request-1")),
        payload,
        work,
    }
}

pub fn call(provider: &ListsProvider, action: &str, payload: Value) -> ProviderCall {
    provider.call(request(action, payload)).unwrap()
}

/// The immediate response envelope, for calls that answer without a write.
pub fn response(provider: &ListsProvider, action: &str, payload: Value) -> Value {
    call(provider, action, payload)
        .response
        .expect("action answers immediately")
        .decode()
        .unwrap()
}

pub fn drain(observer: &ProviderPushObserver) -> Vec<Value> {
    observer
        .drain(16)
        .unwrap()
        .pushes
        .into_iter()
        .map(|push| push.envelope.decode().unwrap())
        .collect()
}

pub fn p(value: &str) -> Value {
    json!({"type": "p", "value": value})
}

pub fn pubkey(seed: &str) -> String {
    seed.repeat(64 / seed.len())
}

pub fn follows() -> Value {
    json!({"kind": 3})
}

pub fn entry(seed: &str) -> ListEntry {
    ListEntry::new(ListItemTag::P, pubkey(seed))
}

/// One NMP receipt frame in the given delivery state.
pub fn receipt(state: &str) -> ReceiptSnapshot {
    ReceiptSnapshot {
        receipt_id: WriteReceiptId(Arc::from("receipt-1")),
        state: BoundedJson::from_value(&json!({"state": state}), 1024).unwrap(),
    }
}
