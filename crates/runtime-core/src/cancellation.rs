use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use parking_lot::{Condvar, Mutex};
use thiserror::Error;

const MAXIMUM_CANCELLATION_WAKEUPS: usize = 8;
type CancellationWake = dyn Fn() + Send + Sync + 'static;

/// Event-driven cancellation signal shared by one bounded native operation.
#[derive(Clone, Debug)]
pub struct Cancellation {
    inner: Arc<Inner>,
}

pub(crate) struct Inner {
    cancelled: AtomicBool,
    gate: Mutex<()>,
    changed: Condvar,
    next_wakeup_id: AtomicU64,
    wakeups: Mutex<BTreeMap<u64, Arc<CancellationWake>>>,
}

impl fmt::Debug for Inner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Inner")
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("registered_wakeups", &self.wakeups.lock().len())
            .finish_non_exhaustive()
    }
}

impl Inner {
    pub(crate) fn cancel(&self) {
        let was_cancelled = self.cancelled.swap(true, Ordering::AcqRel);
        if !was_cancelled {
            self.changed.notify_all();
            let wakeups = std::mem::take(&mut *self.wakeups.lock());
            for wake in wakeups.into_values() {
                wake();
            }
        }
    }
}

impl Cancellation {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                cancelled: AtomicBool::new(false),
                gate: Mutex::new(()),
                changed: Condvar::new(),
                next_wakeup_id: AtomicU64::new(0),
                wakeups: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    pub fn cancel(&self) -> bool {
        let was_cancelled = self.inner.cancelled.load(Ordering::Acquire);
        self.inner.cancel();
        !was_cancelled
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    pub fn wait(&self) {
        if self.is_cancelled() {
            return;
        }
        let mut guard = self.inner.gate.lock();
        while !self.is_cancelled() {
            self.inner.changed.wait(&mut guard);
        }
    }

    pub fn wait_until(&self, deadline: Instant) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            return Ok(());
        }
        let mut guard = self.inner.gate.lock();
        loop {
            if self.is_cancelled() {
                return Ok(());
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Cancelled);
            }
            self.inner
                .changed
                .wait_for(&mut guard, deadline.saturating_duration_since(now));
        }
    }

    pub fn wait_for(&self, duration: Duration) -> Result<(), Cancelled> {
        self.wait_until(Instant::now() + duration)
    }

    /// Register one finite, event-driven wakeup for cancellation.
    ///
    /// Dropping the returned registration removes the wakeup. Registering
    /// after cancellation invokes the wakeup immediately and returns an inert
    /// registration. Capacity is explicit so callers never create an
    /// unbounded waiter resource.
    pub fn register_wakeup<F>(
        &self,
        wakeup: F,
    ) -> Result<CancellationWakeRegistration, CancellationWakeError>
    where
        F: Fn() + Send + Sync + 'static,
    {
        let wakeup: Arc<CancellationWake> = Arc::new(wakeup);
        let mut wakeups = self.inner.wakeups.lock();
        if self.is_cancelled() {
            drop(wakeups);
            wakeup();
            return Ok(CancellationWakeRegistration {
                inner: Weak::new(),
                id: None,
            });
        }
        if wakeups.len() >= MAXIMUM_CANCELLATION_WAKEUPS {
            return Err(CancellationWakeError::Capacity {
                capacity: MAXIMUM_CANCELLATION_WAKEUPS,
            });
        }
        let id = self.inner.next_wakeup_id.fetch_add(1, Ordering::Relaxed);
        wakeups.insert(id, wakeup);
        Ok(CancellationWakeRegistration {
            inner: Arc::downgrade(&self.inner),
            id: Some(id),
        })
    }

    pub(crate) fn registry_weak(&self) -> std::sync::Weak<Inner> {
        Arc::downgrade(&self.inner)
    }
}

impl Default for Cancellation {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled;

#[derive(Debug)]
#[must_use = "dropping the registration removes its cancellation wakeup"]
pub struct CancellationWakeRegistration {
    inner: Weak<Inner>,
    id: Option<u64>,
}

impl Drop for CancellationWakeRegistration {
    fn drop(&mut self) {
        let Some(id) = self.id.take() else {
            return;
        };
        if let Some(inner) = self.inner.upgrade() {
            inner.wakeups.lock().remove(&id);
        }
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum CancellationWakeError {
    #[error("cancellation wakeup capacity {capacity} is full")]
    Capacity { capacity: usize },
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    #[test]
    fn wakeups_are_bounded_removed_on_drop_and_called_once() {
        let cancellation = Cancellation::new();
        let called = Arc::new(AtomicUsize::new(0));
        let dropped_counter = Arc::clone(&called);
        let dropped = cancellation
            .register_wakeup(move || {
                dropped_counter.fetch_add(1, Ordering::AcqRel);
            })
            .unwrap();
        drop(dropped);

        let mut registrations = Vec::new();
        for _ in 0..MAXIMUM_CANCELLATION_WAKEUPS {
            let counter = Arc::clone(&called);
            registrations.push(
                cancellation
                    .register_wakeup(move || {
                        counter.fetch_add(1, Ordering::AcqRel);
                    })
                    .unwrap(),
            );
        }
        assert!(matches!(
            cancellation.register_wakeup(|| {}),
            Err(CancellationWakeError::Capacity {
                capacity: MAXIMUM_CANCELLATION_WAKEUPS,
            })
        ));

        assert!(cancellation.cancel());
        assert!(!cancellation.cancel());
        assert_eq!(called.load(Ordering::Acquire), MAXIMUM_CANCELLATION_WAKEUPS);
        drop(registrations);
    }

    #[test]
    fn registration_after_cancellation_wakes_immediately() {
        let cancellation = Cancellation::new();
        cancellation.cancel();
        let called = Arc::new(AtomicBool::new(false));
        let wake_called = Arc::clone(&called);

        let registration = cancellation
            .register_wakeup(move || wake_called.store(true, Ordering::Release))
            .unwrap();

        assert!(called.load(Ordering::Acquire));
        drop(registration);
    }
}
