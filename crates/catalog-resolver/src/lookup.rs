use std::{fmt, sync::Arc};

use nmp_native_artifact::ManifestCoordinate;
use parking_lot::{Condvar, Mutex};
use thiserror::Error;

use crate::{CancellationToken, ResolveError};

#[derive(Clone, Debug)]
pub struct ManifestLookupRequest {
    pub(crate) coordinate: ManifestCoordinate,
    pub(crate) maximum_event_bytes: usize,
    pub(crate) maximum_facts: usize,
}

impl ManifestLookupRequest {
    pub fn coordinate(&self) -> &ManifestCoordinate {
        &self.coordinate
    }

    pub fn maximum_event_bytes(&self) -> usize {
        self.maximum_event_bytes
    }

    pub fn maximum_facts(&self) -> usize {
        self.maximum_facts
    }
}

/// A source-scoped fact. `Observed` never means globally complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoordinateLookupFact {
    pub(crate) source: Arc<str>,
    pub(crate) state: CoordinateLookupState,
}

impl CoordinateLookupFact {
    pub fn observed(source: impl Into<Arc<str>>, rows: usize) -> Self {
        Self {
            source: source.into(),
            state: CoordinateLookupState::Observed { rows },
        }
    }

    pub fn shortfall(source: impl Into<Arc<str>>, reason: impl Into<Arc<str>>) -> Self {
        Self {
            source: source.into(),
            state: CoordinateLookupState::Shortfall {
                reason: reason.into(),
            },
        }
    }

    pub fn selected(source: impl Into<Arc<str>>, event_id: impl Into<Arc<str>>) -> Self {
        Self {
            source: source.into(),
            state: CoordinateLookupState::Selected {
                event_id: event_id.into(),
            },
        }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn state(&self) -> &CoordinateLookupState {
        &self.state
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoordinateLookupState {
    Observed { rows: usize },
    Shortfall { reason: Arc<str> },
    Selected { event_id: Arc<str> },
}

#[derive(Clone, Debug)]
pub struct ManifestLookupResponse {
    pub(crate) selected_event_json: Option<Arc<[u8]>>,
    pub(crate) facts: Arc<[CoordinateLookupFact]>,
}

impl ManifestLookupResponse {
    pub fn found(
        selected_event_json: impl Into<Arc<[u8]>>,
        facts: impl Into<Arc<[CoordinateLookupFact]>>,
    ) -> Self {
        Self {
            selected_event_json: Some(selected_event_json.into()),
            facts: facts.into(),
        }
    }

    pub fn not_found(facts: impl Into<Arc<[CoordinateLookupFact]>>) -> Self {
        Self {
            selected_event_json: None,
            facts: facts.into(),
        }
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("manifest lookup port failed: {reason}")]
pub struct LookupPortError {
    pub(crate) reason: Arc<str>,
}

impl LookupPortError {
    pub fn new(reason: impl Into<Arc<str>>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

/// One live NMP lookup registration. Cancellation must be idempotent and
/// nonblocking; it detaches the owned observation without joining app code.
pub trait ManifestLookupOperation: Send + Sync + fmt::Debug {
    fn cancel(&self);
}

#[derive(Clone, Debug)]
pub struct ManifestLookupCompletion {
    state: Arc<LookupCompletionState>,
}

#[derive(Debug)]
struct LookupCompletionState {
    result: Mutex<LookupCompletionResult>,
    ready: Arc<Condvar>,
}

#[derive(Debug)]
enum LookupCompletionResult {
    Pending,
    Ready(Result<ManifestLookupResponse, LookupPortError>),
    Closed,
}

impl ManifestLookupCompletion {
    pub(crate) fn pending() -> Self {
        Self {
            state: Arc::new(LookupCompletionState {
                result: Mutex::new(LookupCompletionResult::Pending),
                ready: Arc::new(Condvar::new()),
            }),
        }
    }

    /// Completes the lookup exactly once. `false` means the resolver already
    /// completed or cancelled the operation and the result was not retained.
    pub fn resolve(&self, result: Result<ManifestLookupResponse, LookupPortError>) -> bool {
        let accepted = {
            let mut state = self.state.result.lock();
            if !matches!(*state, LookupCompletionResult::Pending) {
                false
            } else {
                *state = LookupCompletionResult::Ready(result);
                true
            }
        };
        if accepted {
            self.state.ready.notify_all();
        }
        accepted
    }

    pub(crate) fn wait(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<ManifestLookupResponse, ResolveError> {
        let _registration = cancellation.register(Arc::clone(&self.state.ready))?;
        let mut state = self.state.result.lock();
        loop {
            if cancellation.is_cancelled() {
                *state = LookupCompletionResult::Closed;
                return Err(ResolveError::Cancelled);
            }
            match std::mem::replace(&mut *state, LookupCompletionResult::Closed) {
                LookupCompletionResult::Pending => {
                    *state = LookupCompletionResult::Pending;
                    self.state.ready.wait(&mut state);
                }
                LookupCompletionResult::Ready(result) => {
                    return result.map_err(|error| ResolveError::Lookup {
                        reason: error.reason,
                    });
                }
                LookupCompletionResult::Closed => return Err(ResolveError::LookupClosed),
            }
        }
    }
}

/// NMP/public-facade boundary. Implementations start one observation and
/// return its cancellation ownership without blocking. They return NMP's
/// selected row and scoped evidence through `completion`; relay choice and
/// replacement policy never cross this boundary.
pub trait ManifestLookupPort: Send + Sync + fmt::Debug {
    fn start_lookup(
        &self,
        request: ManifestLookupRequest,
        completion: ManifestLookupCompletion,
    ) -> Result<Arc<dyn ManifestLookupOperation>, LookupPortError>;
}
