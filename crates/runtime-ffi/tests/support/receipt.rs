use std::sync::Arc;

use nmp_native_runtime_app::{ReceiptDeliveryState, ReceiptView};
use nmp_native_runtime_core::{BoundedJson, ReceiptSnapshot, WriteReceiptId};
use nmp_native_runtime_ffi::{RuntimeReceiptSnapshot, project_receipt};

const RECEIPT_ID: &str = "receipt-bdd-42";
const MAXIMUM_TEST_FRAME_BYTES: usize = 64 * 1_024;

#[derive(Debug)]
pub struct ReceiptProjectionRig {
    raw: Option<String>,
}

impl ReceiptProjectionRig {
    pub fn named(name: &str) -> Self {
        Self {
            raw: match name {
                "missing" => None,
                "malformed" => Some("[]".to_owned()),
                "unknown" => Some(state("future_state", serde_json::json!({}))),
                "oversized" => Some(
                    serde_json::json!({
                        "schema": "nostr.write.receipt/1",
                        "state": "failed",
                        "failure": "x".repeat(17 * 1_024),
                        "relays": {},
                    })
                    .to_string(),
                ),
                "in-progress" => Some(state("accepted", serde_json::json!({}))),
                "delivered" => Some(state(
                    "delivered",
                    serde_json::json!({"wss://one.example": relay("acked")}),
                )),
                "partial" => Some(state(
                    "partial_delivery",
                    serde_json::json!({
                        "wss://one.example": relay("acked"),
                        "wss://two.example": relay("rejected"),
                    }),
                )),
                "exhausted" => Some(state(
                    "exhausted",
                    serde_json::json!({"wss://one.example": relay("gave_up")}),
                )),
                "ambiguous" => Some(state(
                    "exhausted",
                    serde_json::json!({"wss://one.example": relay("outcome_unknown")}),
                )),
                "refused" => Some(state(
                    "exhausted",
                    serde_json::json!({"wss://one.example": relay("rejected")}),
                )),
                "failed" => Some(
                    serde_json::json!({
                        "schema": "nostr.write.receipt/1",
                        "state": "failed",
                        "failure": "signer refused",
                        "relays": {},
                    })
                    .to_string(),
                ),
                "cancelled" => Some(state("cancelled", serde_json::json!({}))),
                "conflict" => Some(
                    serde_json::json!({
                        "schema": "nostr.write.receipt/1",
                        "state": "replaceable_conflict",
                        "conflict": {"expected": "a", "actual": "b"},
                        "relays": {},
                    })
                    .to_string(),
                ),
                other => panic!("unknown receipt fixture {other:?}"),
            },
        }
    }

    pub fn raw(&self) -> Option<&str> {
        self.raw.as_deref()
    }

    pub fn project(&self, lifecycle: ReceiptDeliveryState) -> RuntimeReceiptSnapshot {
        project_receipt(&ReceiptView {
            receipt_id: receipt_id(),
            delivery: lifecycle,
            latest: self.snapshot(),
        })
    }

    pub fn close_after_latest(&self) -> RuntimeReceiptSnapshot {
        self.project(ReceiptDeliveryState::Closed)
    }

    pub fn reattach(&self) -> (RuntimeReceiptSnapshot, RuntimeReceiptSnapshot) {
        (
            self.project(ReceiptDeliveryState::Observing),
            self.project(ReceiptDeliveryState::Observing),
        )
    }

    fn snapshot(&self) -> Option<ReceiptSnapshot> {
        self.raw.as_ref().map(|raw| ReceiptSnapshot {
            receipt_id: receipt_id(),
            state: BoundedJson::from_raw(raw, MAXIMUM_TEST_FRAME_BYTES)
                .expect("receipt fixture is bounded valid JSON"),
        })
    }
}

fn receipt_id() -> WriteReceiptId {
    WriteReceiptId(Arc::from(RECEIPT_ID))
}

fn relay(state: &str) -> serde_json::Value {
    serde_json::json!({"state": state, "terminal": true})
}

fn state(name: &str, relays: serde_json::Value) -> String {
    serde_json::json!({
        "schema": "nostr.write.receipt/1",
        "state": name,
        "relays": relays,
    })
    .to_string()
}
