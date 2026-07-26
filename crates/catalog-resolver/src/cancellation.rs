use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use parking_lot::Mutex;

use crate::ResolveError;

const MAXIMUM_CANCELLATION_WAKEUPS: usize = 8;

/// Cloneable event-driven cancellation shared across one bounded operation.
///
/// Cancellation wakes every registered resolver wait immediately. A token has
/// a finite listener ceiling and never creates a polling thread.
#[derive(Clone, Debug)]
pub struct CancellationToken {
    state: Arc<CancellationState>,
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self {
            state: Arc::new(CancellationState::default()),
        }
    }
}

#[derive(Debug, Default)]
struct CancellationState {
    cancelled: AtomicBool,
    next_registration: AtomicU64,
    wakeups: Mutex<BTreeMap<u64, Arc<dyn CancellationWake>>>,
}

pub(crate) trait CancellationWake: fmt::Debug + Send + Sync {
    fn wake(&self);
}

#[derive(Debug)]
pub(crate) struct CancellationRegistration {
    state: Arc<CancellationState>,
    id: Option<u64>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        if self.state.cancelled.swap(true, Ordering::AcqRel) {
            return;
        }
        let wakeups = {
            let mut registered = self.state.wakeups.lock();
            std::mem::take(&mut *registered)
        };
        for wakeup in wakeups.into_values() {
            wakeup.wake();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    pub(crate) fn register(
        &self,
        wakeup: Arc<dyn CancellationWake>,
    ) -> Result<CancellationRegistration, ResolveError> {
        let mut wakeups = self.state.wakeups.lock();
        if self.is_cancelled() {
            drop(wakeups);
            wakeup.wake();
            return Ok(CancellationRegistration {
                state: Arc::clone(&self.state),
                id: None,
            });
        }
        if wakeups.len() >= MAXIMUM_CANCELLATION_WAKEUPS {
            return Err(ResolveError::CancellationSaturated {
                maximum: MAXIMUM_CANCELLATION_WAKEUPS,
            });
        }
        let id = self.state.next_registration.fetch_add(1, Ordering::Relaxed);
        wakeups.insert(id, wakeup);
        Ok(CancellationRegistration {
            state: Arc::clone(&self.state),
            id: Some(id),
        })
    }
}

impl Drop for CancellationRegistration {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            self.state.wakeups.lock().remove(&id);
        }
    }
}
