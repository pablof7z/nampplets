//! Bounded admission and cancellation for catalog worker operations.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use nmp_native_catalog_resolver::CancellationToken;
use nmp_native_nmp_adapter::catalog::CatalogBrowseCancel;
use parking_lot::Mutex;

use super::types::RuntimeCatalogError;

#[derive(Debug)]
pub(super) enum ActiveCancellation {
    Resolve(CancellationToken),
}

impl ActiveCancellation {
    pub(super) fn cancel(&self) {
        match self {
            Self::Resolve(cancellation) => cancellation.cancel(),
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct BrowseOperationControl {
    cancelled: AtomicBool,
    handle: Mutex<Option<CatalogBrowseCancel>>,
}

impl BrowseOperationControl {
    pub(super) fn attach(&self, handle: CatalogBrowseCancel) {
        *self.handle.lock() = Some(handle);
        if self.cancelled.load(Ordering::Acquire) {
            self.cancel();
        }
    }

    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(handle) = self.handle.lock().as_ref() {
            handle.cancel();
        }
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}
#[derive(Debug)]
pub(super) struct OneShotAdmission {
    maximum: usize,
    pub(super) active: AtomicUsize,
}

impl OneShotAdmission {
    pub(super) fn new(maximum: usize) -> Self {
        Self {
            maximum,
            active: AtomicUsize::new(0),
        }
    }

    pub(super) fn reserve(self: &Arc<Self>) -> Result<OneShotPermit, RuntimeCatalogError> {
        let mut active = self.active.load(Ordering::Acquire);
        loop {
            if active >= self.maximum {
                return Err(RuntimeCatalogError::Busy {
                    maximum: self.maximum as u64,
                });
            }
            match self.active.compare_exchange_weak(
                active,
                active + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(OneShotPermit {
                        admission: Arc::clone(self),
                    });
                }
                Err(observed) => active = observed,
            }
        }
    }
}

#[derive(Debug)]
pub(super) struct OneShotPermit {
    admission: Arc<OneShotAdmission>,
}

impl Drop for OneShotPermit {
    fn drop(&mut self) {
        self.admission.active.fetch_sub(1, Ordering::AcqRel);
    }
}
