use std::{fmt, sync::atomic::Ordering};

use crate::NmpDataPlane;

impl fmt::Debug for NmpDataPlane {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NmpDataPlane")
            .field(
                "active_workers",
                &self.workers.active.load(Ordering::Acquire),
            )
            .field("maximum_workers", &self.workers.maximum)
            .field("identity_observers", &self.identity.lock().observers.len())
            .field("closed", &self.closed.load(Ordering::Acquire))
            .finish()
    }
}
