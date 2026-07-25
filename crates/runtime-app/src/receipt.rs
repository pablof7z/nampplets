//! App-owned receipt projection over NMP's durable write obligations.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use nmp_native_nap_bridge::{ActivitySink, ProviderActivity};
use nmp_native_runtime_core::{
    ReceiptEventSink, ReceiptObservation, ReceiptSinkError, ReceiptSnapshot, WriteReceiptId,
};
use parking_lot::Mutex;
use tokio::sync::watch;

use crate::{
    app::ObservationClosed,
    views::{ReceiptDeliveryState, ReceiptView},
};

#[derive(Debug)]
pub struct AppReceipt {
    maximum_frame_bytes: usize,
    inner: Mutex<AppReceiptState>,
    closed: AtomicBool,
    snapshots: watch::Sender<Option<ReceiptSnapshot>>,
}

#[derive(Debug)]
struct AppReceiptState {
    receipt_id: Option<WriteReceiptId>,
    delivery: ReceiptDeliveryState,
    latest: Option<ReceiptSnapshot>,
    observation: Option<Arc<dyn ReceiptObservation>>,
}

#[derive(Debug)]
pub struct ReceiptObserver {
    receiver: watch::Receiver<Option<ReceiptSnapshot>>,
}

impl ReceiptObserver {
    pub fn latest(&self) -> Option<ReceiptSnapshot> {
        self.receiver.borrow().clone()
    }

    pub async fn changed(&mut self) -> Result<ReceiptSnapshot, ObservationClosed> {
        loop {
            self.receiver
                .changed()
                .await
                .map_err(|_| ObservationClosed)?;
            if let Some(snapshot) = self.receiver.borrow_and_update().clone() {
                return Ok(snapshot);
            }
        }
    }
}

impl AppReceipt {
    pub(crate) fn unassigned(maximum_frame_bytes: usize) -> Self {
        Self::new(None, maximum_frame_bytes)
    }

    pub(crate) fn assigned(receipt_id: WriteReceiptId, maximum_frame_bytes: usize) -> Self {
        Self::new(Some(receipt_id), maximum_frame_bytes)
    }

    pub(crate) fn new(receipt_id: Option<WriteReceiptId>, maximum_frame_bytes: usize) -> Self {
        let (snapshots, _) = watch::channel(None);
        Self {
            maximum_frame_bytes,
            inner: Mutex::new(AppReceiptState {
                receipt_id,
                delivery: ReceiptDeliveryState::Observing,
                latest: None,
                observation: None,
            }),
            closed: AtomicBool::new(false),
            snapshots,
        }
    }

    pub(crate) fn assign(&self, receipt_id: WriteReceiptId) -> Result<(), Arc<str>> {
        let mut inner = self.inner.lock();
        if let Some(existing) = &inner.receipt_id
            && existing != &receipt_id
        {
            return Err(Arc::from(
                "receipt sink observed a different id before acceptance returned",
            ));
        }
        if inner
            .latest
            .as_ref()
            .is_some_and(|snapshot| snapshot.receipt_id != receipt_id)
        {
            return Err(Arc::from(
                "receipt snapshot identity differs from accepted receipt",
            ));
        }
        inner.receipt_id = Some(receipt_id);
        Ok(())
    }

    pub(crate) fn attach_observation(&self, observation: Arc<dyn ReceiptObservation>) {
        self.inner.lock().observation = Some(observation);
    }

    pub(crate) fn set_not_found(&self) {
        self.inner.lock().delivery = ReceiptDeliveryState::NotFound;
    }

    pub(crate) fn set_closed(&self) {
        self.closed.store(true, Ordering::Release);
        self.inner.lock().delivery = ReceiptDeliveryState::Closed;
    }

    pub fn observe(&self) -> ReceiptObserver {
        ReceiptObserver {
            receiver: self.snapshots.subscribe(),
        }
    }

    pub fn view(&self) -> Option<ReceiptView> {
        let inner = self.inner.lock();
        Some(ReceiptView {
            receipt_id: inner.receipt_id.clone()?,
            delivery: inner.delivery,
            latest: inner.latest.clone(),
        })
    }

    /// Ends this app consumer's delivery. It does not cancel or weaken the NMP
    /// durable write obligation.
    pub fn stop_delivery(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut inner = self.inner.lock();
        if let Some(observation) = inner.observation.take() {
            observation.stop_delivery();
        }
        inner.delivery = ReceiptDeliveryState::Closed;
    }
}

impl ReceiptEventSink for AppReceipt {
    fn push_latest(&self, snapshot: ReceiptSnapshot) -> Result<(), ReceiptSinkError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(ReceiptSinkError::Closed);
        }
        if snapshot.state.byte_len() > self.maximum_frame_bytes {
            return Err(ReceiptSinkError::FrameTooLarge);
        }
        let mut inner = self.inner.lock();
        if let Some(receipt_id) = &inner.receipt_id
            && receipt_id != &snapshot.receipt_id
        {
            return Err(ReceiptSinkError::Closed);
        }
        if inner.receipt_id.is_none() {
            inner.receipt_id = Some(snapshot.receipt_id.clone());
        }
        inner.latest = Some(snapshot.clone());
        self.snapshots.send_replace(Some(snapshot));
        Ok(())
    }

    fn close(&self, _reason: Option<Arc<str>>) {
        self.set_closed();
    }
}

/// Keeps the runtime-owned receipt projection authoritative while forwarding
/// the same NMP receipt frame to the provider's bounded protocol response.
/// Provider-lane closure never weakens the app-owned durable receipt view.
#[derive(Debug)]
pub(crate) struct ReceiptFanout {
    pub(crate) app: Arc<AppReceipt>,
    pub(crate) provider: Arc<dyn ReceiptEventSink>,
}

impl ReceiptEventSink for ReceiptFanout {
    fn push_latest(&self, snapshot: ReceiptSnapshot) -> Result<(), ReceiptSinkError> {
        self.app.push_latest(snapshot.clone())?;
        let _ = self.provider.push_latest(snapshot);
        Ok(())
    }

    fn close(&self, reason: Option<Arc<str>>) {
        self.app.close(reason.clone());
        self.provider.close(reason);
    }
}

impl Drop for AppReceipt {
    fn drop(&mut self) {
        self.stop_delivery();
    }
}

#[derive(Debug)]
pub(crate) struct NoopBridgeActivity;

impl ActivitySink for NoopBridgeActivity {
    fn record(&self, _fact: ProviderActivity) {}
}
