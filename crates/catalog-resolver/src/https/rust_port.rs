use std::{
    collections::BTreeSet,
    fmt,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    thread,
    time::Duration,
};

use parking_lot::Mutex;
use url::Host;

use super::{
    AcquisitionRefusal, HttpsAcquisitionCompletion, HttpsAcquisitionOperation,
    HttpsAcquisitionPort, HttpsFetchRequest, HttpsFetchResponse, HttpsPortError,
    validate_candidate, validate_resolved_addresses,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RustHttpsAcquisitionConfig {
    pub maximum_in_flight: usize,
    pub maximum_resolved_addresses: usize,
    pub maximum_url_bytes: usize,
    pub worker_threads: usize,
    pub deadline: Duration,
}

impl Default for RustHttpsAcquisitionConfig {
    fn default() -> Self {
        Self {
            maximum_in_flight: 4,
            maximum_resolved_addresses: 16,
            maximum_url_bytes: 2_048,
            worker_threads: 2,
            deadline: Duration::from_secs(15),
        }
    }
}

impl RustHttpsAcquisitionConfig {
    fn validate(self) -> Result<Self, HttpsPortError> {
        if self.maximum_in_flight == 0
            || self.maximum_resolved_addresses == 0
            || self.maximum_url_bytes == 0
            || self.worker_threads == 0
            || self.deadline.is_zero()
        {
            return Err(HttpsPortError::new(
                "Rust HTTPS executor limits must be finite and non-zero",
            ));
        }
        Ok(self)
    }
}

/// Production Rust HTTPS transport. A fixed Tokio runtime owns at most
/// `maximum_in_flight` zero-queue tasks. DNS is resolved once through the
/// system resolver, rejected by this crate's public-address policy before any
/// connect, then pinned into a redirect-disabled reqwest client under the
/// original URL hostname so TLS certificate validation and SNI are preserved.
pub struct RustHttpsAcquisitionPort {
    config: RustHttpsAcquisitionConfig,
    runtime: Option<tokio::runtime::Runtime>,
    admission: Arc<HttpsAdmission>,
}

impl fmt::Debug for RustHttpsAcquisitionPort {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustHttpsAcquisitionPort")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl RustHttpsAcquisitionPort {
    pub fn new(config: RustHttpsAcquisitionConfig) -> Result<Self, HttpsPortError> {
        let config = config.validate()?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(config.worker_threads)
            .max_blocking_threads(config.maximum_in_flight)
            .enable_io()
            .enable_time()
            .thread_name("nampplets-artifact-https")
            .build()
            .map_err(|error| HttpsPortError::new(error.to_string()))?;
        Ok(Self {
            config,
            runtime: Some(runtime),
            admission: Arc::new(HttpsAdmission::new(config.maximum_in_flight)),
        })
    }
}

impl Drop for RustHttpsAcquisitionPort {
    /// Tokio refuses to synchronously shut down a runtime from a thread that
    /// is itself executing inside another runtime's async context (observed
    /// when the last handle to this port is released from the FFI
    /// observation thread). Shutting the runtime down on a fresh, bare
    /// thread avoids that panic regardless of which thread drops this port.
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            let _ = thread::Builder::new()
                .name("nampplets-artifact-https-shutdown".to_owned())
                .spawn(move || drop(runtime));
        }
    }
}

#[derive(Debug)]
struct RustHttpsOperation {
    abort: tokio::task::AbortHandle,
}

impl HttpsAcquisitionOperation for RustHttpsOperation {
    fn cancel(&self) {
        self.abort.abort();
    }
}

impl Drop for RustHttpsOperation {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

impl HttpsAcquisitionPort for RustHttpsAcquisitionPort {
    fn start_fetch(
        &self,
        request: HttpsFetchRequest,
        completion: HttpsAcquisitionCompletion,
    ) -> Result<Arc<dyn HttpsAcquisitionOperation>, HttpsPortError> {
        let permit = self.admission.reserve()?;
        let config = self.config;
        let runtime = self
            .runtime
            .as_ref()
            .expect("runtime is only taken by Drop, after which the port is unreachable");
        let task = runtime.spawn(async move {
            let result = rust_https_fetch(request, config).await;
            completion.resolve(result);
            drop(permit);
        });
        Ok(Arc::new(RustHttpsOperation {
            abort: task.abort_handle(),
        }))
    }
}

#[derive(Debug)]
struct HttpsAdmission {
    maximum: usize,
    active: Mutex<usize>,
}

impl HttpsAdmission {
    fn new(maximum: usize) -> Self {
        Self {
            maximum,
            active: Mutex::new(0),
        }
    }

    fn reserve(self: &Arc<Self>) -> Result<HttpsPermit, HttpsPortError> {
        let mut active = self.active.lock();
        if *active >= self.maximum {
            return Err(HttpsPortError::Saturated {
                maximum: self.maximum,
            });
        }
        *active += 1;
        Ok(HttpsPermit {
            admission: Arc::clone(self),
        })
    }
}

#[derive(Debug)]
struct HttpsPermit {
    admission: Arc<HttpsAdmission>,
}

impl Drop for HttpsPermit {
    fn drop(&mut self) {
        let mut active = self.admission.active.lock();
        *active = active.saturating_sub(1);
    }
}

async fn rust_https_fetch(
    request: HttpsFetchRequest,
    config: RustHttpsAcquisitionConfig,
) -> Result<HttpsFetchResponse, HttpsPortError> {
    tokio::time::timeout(config.deadline, rust_https_fetch_inner(request, config))
        .await
        .map_err(|_| HttpsPortError::new("HTTPS acquisition deadline elapsed"))?
}

async fn rust_https_fetch_inner(
    request: HttpsFetchRequest,
    config: RustHttpsAcquisitionConfig,
) -> Result<HttpsFetchResponse, HttpsPortError> {
    let url = validate_candidate(request.url(), config.maximum_url_bytes)
        .map_err(|reason| HttpsPortError::Refused { reason })?;
    let host = url.host_str().ok_or(HttpsPortError::Refused {
        reason: AcquisitionRefusal::InvalidCandidate,
    })?;
    let port = url.port_or_known_default().ok_or(HttpsPortError::Refused {
        reason: AcquisitionRefusal::InvalidCandidate,
    })?;
    let socket_addresses = match url.host() {
        Some(Host::Ipv4(address)) => vec![SocketAddr::new(IpAddr::V4(address), port)],
        Some(Host::Ipv6(address)) => vec![SocketAddr::new(IpAddr::V6(address), port)],
        Some(Host::Domain(domain)) => {
            let resolved = tokio::net::lookup_host((domain, port))
                .await
                .map_err(|error| HttpsPortError::new(error.to_string()))?;
            let mut unique = BTreeSet::new();
            for address in resolved {
                unique.insert(address);
                if unique.len() > config.maximum_resolved_addresses {
                    return Err(HttpsPortError::Refused {
                        reason: AcquisitionRefusal::AddressLimit {
                            actual: unique.len(),
                            maximum: config.maximum_resolved_addresses,
                        },
                    });
                }
            }
            unique.into_iter().collect()
        }
        None => {
            return Err(HttpsPortError::Refused {
                reason: AcquisitionRefusal::InvalidCandidate,
            });
        }
    };
    let resolved_addresses: Vec<IpAddr> = socket_addresses
        .iter()
        .map(|address| address.ip())
        .collect();
    validate_resolved_addresses(&resolved_addresses, config.maximum_resolved_addresses)
        .map_err(|reason| HttpsPortError::Refused { reason })?;

    let mut client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(config.deadline)
        .timeout(config.deadline);
    if matches!(url.host(), Some(Host::Domain(_))) {
        client = client.resolve_to_addrs(host, &socket_addresses);
    }
    let client = client
        .build()
        .map_err(|error| HttpsPortError::new(error.to_string()))?;
    let mut response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|error| HttpsPortError::new(error.to_string()))?;
    let effective_url: Arc<str> = Arc::from(response.url().as_str());
    let status = response.status().as_u16();
    let redirect_location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(Arc::<str>::from);
    let maximum_with_probe =
        request
            .maximum_bytes
            .checked_add(1)
            .ok_or(HttpsPortError::Refused {
                reason: AcquisitionRefusal::Oversize {
                    actual: usize::MAX,
                    maximum: request.maximum_bytes,
                },
            })?;
    let mut body = Vec::with_capacity(maximum_with_probe.min(64 * 1_024));
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| HttpsPortError::new(error.to_string()))?
    {
        let remaining = maximum_with_probe.saturating_sub(body.len());
        if remaining == 0 {
            break;
        }
        let take = remaining.min(chunk.len());
        body.extend_from_slice(&chunk[..take]);
        if body.len() == maximum_with_probe {
            break;
        }
    }
    Ok(HttpsFetchResponse::new(
        effective_url,
        status,
        redirect_location,
        resolved_addresses,
        body,
    ))
}
