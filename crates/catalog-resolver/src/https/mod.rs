//! Raw HTTPS acquisition ports, facts, and the production Rust transport.
//!
//! [`address`] holds the security-critical public-address validation
//! (`validate_candidate`, `validate_resolved_addresses`, `is_public_ip*`)
//! moved byte-for-byte from the original single-file implementation.
//! [`rust_port`] holds the production `RustHttpsAcquisitionPort`.

mod address;
mod rust_port;

use std::{fmt, net::IpAddr, sync::Arc};

use parking_lot::{Condvar, Mutex};
use thiserror::Error;

pub(crate) use address::{validate_candidate, validate_resolved_addresses};
pub use rust_port::{RustHttpsAcquisitionConfig, RustHttpsAcquisitionPort};

use crate::{CancellationToken, ResolveError, cancellation::CancellationWake};

#[derive(Clone, Debug)]
pub struct HttpsFetchRequest {
    pub(crate) url: Arc<str>,
    pub(crate) maximum_bytes: usize,
}

impl HttpsFetchRequest {
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn maximum_bytes(&self) -> usize {
        self.maximum_bytes
    }
}

#[derive(Clone, Debug)]
pub struct HttpsFetchResponse {
    pub(crate) effective_url: Arc<str>,
    pub(crate) status: u16,
    pub(crate) redirect_location: Option<Arc<str>>,
    pub(crate) resolved_addresses: Arc<[IpAddr]>,
    pub(crate) body: Arc<[u8]>,
}

impl HttpsFetchResponse {
    pub fn new(
        effective_url: impl Into<Arc<str>>,
        status: u16,
        redirect_location: Option<Arc<str>>,
        resolved_addresses: impl Into<Arc<[IpAddr]>>,
        body: impl Into<Arc<[u8]>>,
    ) -> Self {
        Self {
            effective_url: effective_url.into(),
            status,
            redirect_location,
            resolved_addresses: resolved_addresses.into(),
            body: body.into(),
        }
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum HttpsPortError {
    #[error("HTTPS acquisition port failed: {reason}")]
    Transport { reason: Arc<str> },
    #[error("HTTPS acquisition was refused before connect: {reason}")]
    Refused { reason: AcquisitionRefusal },
    #[error("HTTPS executor is saturated at {maximum} operations")]
    Saturated { maximum: usize },
}

impl HttpsPortError {
    pub fn new(reason: impl Into<Arc<str>>) -> Self {
        Self::Transport {
            reason: reason.into(),
        }
    }
}

pub trait HttpsAcquisitionOperation: Send + Sync + fmt::Debug {
    fn cancel(&self);
}

#[derive(Clone, Debug)]
pub struct HttpsAcquisitionCompletion {
    state: Arc<HttpsCompletionState>,
}

#[derive(Debug)]
struct HttpsCompletionState {
    result: Mutex<HttpsCompletionResult>,
    ready: Arc<Condvar>,
}

#[derive(Debug)]
enum HttpsCompletionResult {
    Pending,
    Ready(Result<HttpsFetchResponse, HttpsPortError>),
    Closed,
}

impl CancellationWake for HttpsCompletionState {
    fn wake(&self) {
        // Synchronize notification with the predicate check and atomic park.
        let _result = self.result.lock();
        self.ready.notify_all();
    }
}

#[derive(Debug)]
pub(crate) enum HttpsWaitError {
    Cancelled,
    Port(HttpsPortError),
    Closed,
    CancellationSaturated { maximum: usize },
}

impl HttpsAcquisitionCompletion {
    pub(crate) fn pending() -> Self {
        Self {
            state: Arc::new(HttpsCompletionState {
                result: Mutex::new(HttpsCompletionResult::Pending),
                ready: Arc::new(Condvar::new()),
            }),
        }
    }

    pub fn resolve(&self, result: Result<HttpsFetchResponse, HttpsPortError>) -> bool {
        let accepted = {
            let mut state = self.state.result.lock();
            if !matches!(*state, HttpsCompletionResult::Pending) {
                false
            } else {
                *state = HttpsCompletionResult::Ready(result);
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
    ) -> Result<HttpsFetchResponse, HttpsWaitError> {
        let wakeup: Arc<dyn CancellationWake> = self.state.clone();
        let _registration = cancellation.register(wakeup).map_err(|error| match error {
            ResolveError::CancellationSaturated { maximum } => {
                HttpsWaitError::CancellationSaturated { maximum }
            }
            _ => HttpsWaitError::Closed,
        })?;
        let mut state = self.state.result.lock();
        loop {
            if cancellation.is_cancelled() {
                *state = HttpsCompletionResult::Closed;
                return Err(HttpsWaitError::Cancelled);
            }
            match std::mem::replace(&mut *state, HttpsCompletionResult::Closed) {
                HttpsCompletionResult::Pending => {
                    *state = HttpsCompletionResult::Pending;
                    self.state.ready.wait(&mut state);
                }
                HttpsCompletionResult::Ready(result) => {
                    return result.map_err(HttpsWaitError::Port);
                }
                HttpsCompletionResult::Closed => return Err(HttpsWaitError::Closed),
            }
        }
    }
}

/// Raw HTTPS executor. Implementations start without blocking, disable
/// redirects, pin every connection to the exact reported DNS results while
/// preserving hostname TLS/SNI, and cap streaming reads to
/// `maximum_bytes + 1`.
pub trait HttpsAcquisitionPort: Send + Sync + fmt::Debug {
    fn start_fetch(
        &self,
        request: HttpsFetchRequest,
        completion: HttpsAcquisitionCompletion,
    ) -> Result<Arc<dyn HttpsAcquisitionOperation>, HttpsPortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcquisitionFact {
    pub(crate) logical_path: Arc<str>,
    pub(crate) source_url: Arc<str>,
    pub(crate) outcome: AcquisitionOutcome,
}

impl AcquisitionFact {
    pub fn logical_path(&self) -> &str {
        &self.logical_path
    }

    pub fn source_url(&self) -> &str {
        &self.source_url
    }

    pub fn outcome(&self) -> &AcquisitionOutcome {
        &self.outcome
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcquisitionOutcome {
    TransportFailed { reason: Arc<str> },
    HttpStatus { status: u16 },
    Refused { reason: AcquisitionRefusal },
    Succeeded { bytes: usize },
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AcquisitionRefusal {
    #[error("operation was cancelled")]
    Cancelled,
    #[error("HTTPS executor is saturated at {maximum} operations")]
    ExecutorSaturated { maximum: usize },
    #[error("cancellation wake capacity is saturated at {maximum} listeners")]
    CancellationCapacity { maximum: usize },
    #[error("candidate URL is invalid")]
    InvalidCandidate,
    #[error("candidate URL is not credential-free HTTPS")]
    NonHttps,
    #[error("candidate URL or DNS result is not a public address: {address}")]
    NonPublicAddress { address: IpAddr },
    #[error("HTTPS response has no resolved-address evidence")]
    MissingAddressEvidence,
    #[error("HTTPS response has {actual} resolved addresses; the maximum is {maximum}")]
    AddressLimit { actual: usize, maximum: usize },
    #[error("supported redirect response {reason}")]
    Redirect { reason: &'static str },
    #[error("redirect exceeded the maximum of {maximum} hops")]
    TooManyRedirects { maximum: usize },
    #[error("effective response URL differs from the exact requested candidate")]
    SourceConfusion,
    #[error("response is {actual} bytes; the maximum is {maximum}")]
    Oversize { actual: usize, maximum: usize },
    #[error("acquisition evidence reached its maximum of {maximum} facts")]
    EvidenceCapacity { maximum: usize },
    #[error("every finite approved source failed")]
    AllSourcesFailed,
}
