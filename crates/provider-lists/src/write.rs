use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use nmp_native_nap_bridge::{ProviderPushSender, ProviderWriteCompletion};
use nmp_native_runtime_core::{BoundedJson, ReceiptEventSink, ReceiptSinkError, ReceiptSnapshot};
use serde_json::{Map, Value};

/// The two mutating actions, and the pinned field each reports its count
/// under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListsAction {
    Add,
    Remove,
}

impl ListsAction {
    pub(crate) const fn result_type(self) -> &'static str {
        match self {
            Self::Add => "lists.add.result",
            Self::Remove => "lists.remove.result",
        }
    }

    pub(crate) const fn changed_field(self) -> &'static str {
        match self {
            Self::Add => "added",
            Self::Remove => "removed",
        }
    }
}

/// Carries the counts Rust already decided across native approval, so the
/// napplet's result reports what the write actually did rather than what was
/// requested.
pub(crate) struct ListsWriteCompletion {
    pub(crate) action: ListsAction,
    pub(crate) id: Arc<str>,
    pub(crate) changed: usize,
    pub(crate) skipped: usize,
    pub(crate) outbound: ProviderPushSender,
    pub(crate) maximum_response_bytes: usize,
}

impl fmt::Debug for ListsWriteCompletion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ListsWriteCompletion")
            .field("action", &self.action)
            .field("id", &self.id)
            .field("changed", &self.changed)
            .finish_non_exhaustive()
    }
}

impl ProviderWriteCompletion for ListsWriteCompletion {
    fn into_receipt_sink(self: Box<Self>) -> Arc<dyn ReceiptEventSink> {
        Arc::new(ListsReceiptSink {
            action: self.action,
            id: self.id,
            changed: self.changed,
            skipped: self.skipped,
            outbound: self.outbound,
            maximum_response_bytes: self.maximum_response_bytes,
            delivered: AtomicBool::new(false),
        })
    }

    fn refused(self: Box<Self>, reason: Arc<str>) {
        let sink = self.into_receipt_sink();
        sink.close(Some(reason));
    }
}

struct ListsReceiptSink {
    action: ListsAction,
    id: Arc<str>,
    changed: usize,
    skipped: usize,
    outbound: ProviderPushSender,
    maximum_response_bytes: usize,
    delivered: AtomicBool,
}

impl fmt::Debug for ListsReceiptSink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ListsReceiptSink")
            .field("action", &self.action)
            .field("id", &self.id)
            .field("delivered", &self.delivered.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl ListsReceiptSink {
    /// Emits exactly one result for this mutation, whatever path gets here
    /// first.
    fn deliver(&self, ok: bool, error: Option<String>) {
        if self.delivered.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut envelope = Map::new();
        envelope.insert("type".to_owned(), Value::from(self.action.result_type()));
        envelope.insert("id".to_owned(), Value::from(self.id.as_ref()));
        envelope.insert("ok".to_owned(), Value::from(ok));
        envelope.insert(
            self.action.changed_field().to_owned(),
            Value::from(if ok { self.changed } else { 0 }),
        );
        envelope.insert(
            "skipped".to_owned(),
            Value::from(if ok { self.skipped } else { 0 }),
        );
        if let Some(error) = error {
            envelope.insert("error".to_owned(), Value::from(error));
        }
        let Ok(message) =
            BoundedJson::from_value(&Value::Object(envelope), self.maximum_response_bytes)
        else {
            return;
        };
        let _ = self
            .outbound
            .push_envelope(&message, Some(self.action.result_type()));
    }
}

impl ReceiptEventSink for ListsReceiptSink {
    fn push_latest(&self, snapshot: ReceiptSnapshot) -> Result<(), ReceiptSinkError> {
        if self.delivered.load(Ordering::Acquire) {
            return Ok(());
        }
        let value = snapshot
            .state
            .decode()
            .map_err(|_| ReceiptSinkError::FrameTooLarge)?;
        let state = value
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let ok = match state {
            "delivered" | "partial_delivery" => true,
            "exhausted" | "failed" | "cancelled" | "replaceable_conflict" => false,
            // Still in flight. The list is not changed until NMP says so, and
            // the napplet is told nothing before then.
            _ => return Ok(()),
        };
        let error = (!ok).then(|| {
            value
                .get("failure")
                .and_then(Value::as_str)
                .unwrap_or("NMP did not durably write the list change")
                .to_owned()
        });
        self.deliver(ok, error);
        Ok(())
    }

    fn close(&self, reason: Option<Arc<str>>) {
        self.deliver(
            false,
            Some(
                reason
                    .map(|reason| reason.to_string())
                    .unwrap_or_else(|| "the list change was not accepted".to_owned()),
            ),
        );
    }
}
