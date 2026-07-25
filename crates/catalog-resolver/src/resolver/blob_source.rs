use std::{io::Cursor, sync::Arc};

use nmp_native_artifact::{
    BlobFetchRequest, BlobFetchResponse, BlobSourceError, ManifestBlobSource,
};
use parking_lot::Mutex;

use crate::{
    AcquisitionFact, AcquisitionOutcome, AcquisitionRefusal, CancellationToken,
    HttpsAcquisitionCompletion, HttpsAcquisitionPort, HttpsFetchRequest, HttpsPortError,
    ResolverLimits,
    https::{HttpsWaitError, validate_candidate, validate_resolved_addresses},
    redirect::{ResponseAction, classify_response},
};

use super::bounded_reason;

#[derive(Debug)]
pub(crate) struct SafeManifestBlobSource {
    transport: Arc<dyn HttpsAcquisitionPort>,
    cancellation: CancellationToken,
    limits: ResolverLimits,
    state: Mutex<AcquisitionState>,
}

#[derive(Debug, Default)]
struct AcquisitionState {
    facts: Vec<AcquisitionFact>,
    terminal_refusal: Option<AcquisitionRefusal>,
}

impl SafeManifestBlobSource {
    pub(crate) fn new(
        transport: Arc<dyn HttpsAcquisitionPort>,
        cancellation: CancellationToken,
        limits: ResolverLimits,
    ) -> Self {
        Self {
            transport,
            cancellation,
            limits,
            state: Mutex::new(AcquisitionState::default()),
        }
    }

    pub(crate) fn facts(&self) -> Arc<[AcquisitionFact]> {
        self.state.lock().facts.clone().into()
    }

    pub(crate) fn terminal_refusal(&self) -> Option<AcquisitionRefusal> {
        self.state.lock().terminal_refusal.clone()
    }

    fn refuse(
        &self,
        logical_path: &str,
        source_url: &str,
        reason: AcquisitionRefusal,
    ) -> BlobSourceError {
        let fact = AcquisitionFact {
            logical_path: Arc::from(logical_path),
            source_url: Arc::from(source_url),
            outcome: AcquisitionOutcome::Refused {
                reason: reason.clone(),
            },
        };
        let mut state = self.state.lock();
        if state.facts.len() < self.limits.maximum_acquisition_facts {
            state.facts.push(fact);
            state.terminal_refusal = Some(reason.clone());
        } else {
            state.terminal_refusal = Some(AcquisitionRefusal::EvidenceCapacity {
                maximum: self.limits.maximum_acquisition_facts,
            });
        }
        BlobSourceError {
            reason: state
                .terminal_refusal
                .as_ref()
                .expect("terminal refusal was just assigned")
                .to_string(),
        }
    }

    fn record(
        &self,
        logical_path: &str,
        source_url: &str,
        outcome: AcquisitionOutcome,
    ) -> Result<(), BlobSourceError> {
        let mut state = self.state.lock();
        if state.facts.len() >= self.limits.maximum_acquisition_facts {
            let reason = AcquisitionRefusal::EvidenceCapacity {
                maximum: self.limits.maximum_acquisition_facts,
            };
            state.terminal_refusal = Some(reason.clone());
            return Err(BlobSourceError {
                reason: reason.to_string(),
            });
        }
        state.facts.push(AcquisitionFact {
            logical_path: Arc::from(logical_path),
            source_url: Arc::from(source_url),
            outcome,
        });
        Ok(())
    }
}

impl ManifestBlobSource for SafeManifestBlobSource {
    /// Every candidate, and every redirect hop it leads to, is refetched
    /// through the same HTTPS-only / credential-free / public-address /
    /// effective-URL policy. A redirect never substitutes a location that
    /// bypasses that policy; it only ever advances to another location this
    /// policy has independently approved. Hops are capped by
    /// `maximum_redirect_hops` so a redirect chain cannot loop or stall
    /// acquisition indefinitely. Content is still sealed only after its
    /// bytes hash-match the manifest-pinned digest, independent of origin.
    fn fetch(&self, request: &BlobFetchRequest) -> Result<BlobFetchResponse, BlobSourceError> {
        'candidates: for candidate in request.candidate_urls() {
            if self.cancellation.is_cancelled() {
                return Err(self.refuse(
                    request.logical_path(),
                    candidate,
                    AcquisitionRefusal::Cancelled,
                ));
            }
            let mut current_url = match validate_candidate(candidate, self.limits.maximum_url_bytes)
            {
                Ok(url) => url,
                Err(reason) => {
                    return Err(self.refuse(request.logical_path(), candidate, reason));
                }
            };
            let mut hops = 0usize;
            loop {
                let current = current_url.as_str().to_owned();
                let raw_request = HttpsFetchRequest {
                    url: Arc::from(current.as_str()),
                    maximum_bytes: request.maximum_bytes(),
                };
                let completion = HttpsAcquisitionCompletion::pending();
                let operation = match self.transport.start_fetch(raw_request, completion.clone()) {
                    Ok(operation) => operation,
                    Err(HttpsPortError::Refused { reason }) => {
                        return Err(self.refuse(request.logical_path(), &current, reason));
                    }
                    Err(HttpsPortError::Saturated { maximum }) => {
                        return Err(self.refuse(
                            request.logical_path(),
                            &current,
                            AcquisitionRefusal::ExecutorSaturated { maximum },
                        ));
                    }
                    Err(HttpsPortError::Transport { reason }) => {
                        let reason = bounded_reason(reason, self.limits.maximum_reason_bytes);
                        self.record(
                            request.logical_path(),
                            &current,
                            AcquisitionOutcome::TransportFailed { reason },
                        )?;
                        continue 'candidates;
                    }
                };
                let response_result = completion.wait(&self.cancellation);
                operation.cancel();
                let response = match response_result {
                    Ok(response) => response,
                    Err(HttpsWaitError::Cancelled) => {
                        return Err(self.refuse(
                            request.logical_path(),
                            &current,
                            AcquisitionRefusal::Cancelled,
                        ));
                    }
                    Err(HttpsWaitError::Port(HttpsPortError::Refused { reason })) => {
                        return Err(self.refuse(request.logical_path(), &current, reason));
                    }
                    Err(HttpsWaitError::Port(HttpsPortError::Saturated { maximum })) => {
                        return Err(self.refuse(
                            request.logical_path(),
                            &current,
                            AcquisitionRefusal::ExecutorSaturated { maximum },
                        ));
                    }
                    Err(HttpsWaitError::CancellationSaturated { maximum }) => {
                        return Err(self.refuse(
                            request.logical_path(),
                            &current,
                            AcquisitionRefusal::CancellationCapacity { maximum },
                        ));
                    }
                    Err(HttpsWaitError::Port(HttpsPortError::Transport { reason })) => {
                        let reason = bounded_reason(reason, self.limits.maximum_reason_bytes);
                        self.record(
                            request.logical_path(),
                            &current,
                            AcquisitionOutcome::TransportFailed { reason },
                        )?;
                        continue 'candidates;
                    }
                    Err(HttpsWaitError::Closed) => {
                        self.record(
                            request.logical_path(),
                            &current,
                            AcquisitionOutcome::TransportFailed {
                                reason: Arc::from("HTTPS operation closed without a result"),
                            },
                        )?;
                        continue 'candidates;
                    }
                };
                if self.cancellation.is_cancelled() {
                    return Err(self.refuse(
                        request.logical_path(),
                        &current,
                        AcquisitionRefusal::Cancelled,
                    ));
                }
                if let Err(reason) = validate_resolved_addresses(
                    &response.resolved_addresses,
                    self.limits.maximum_resolved_addresses,
                ) {
                    return Err(self.refuse(request.logical_path(), &current, reason));
                }
                match classify_response(
                    &current_url,
                    &response.effective_url,
                    response.status,
                    response.redirect_location.as_deref(),
                    self.limits.maximum_url_bytes,
                ) {
                    Ok(ResponseAction::Follow(next_url)) => {
                        if hops >= self.limits.maximum_redirect_hops {
                            return Err(self.refuse(
                                request.logical_path(),
                                &current,
                                AcquisitionRefusal::TooManyRedirects {
                                    maximum: self.limits.maximum_redirect_hops,
                                },
                            ));
                        }
                        current_url = next_url;
                        hops += 1;
                        continue;
                    }
                    Ok(ResponseAction::HandleStatus) => {}
                    Err(reason) => {
                        return Err(self.refuse(request.logical_path(), &current, reason));
                    }
                }
                if response.body.len() > request.maximum_bytes() {
                    return Err(self.refuse(
                        request.logical_path(),
                        &current,
                        AcquisitionRefusal::Oversize {
                            actual: response.body.len(),
                            maximum: request.maximum_bytes(),
                        },
                    ));
                }
                if response.status != 200 {
                    self.record(
                        request.logical_path(),
                        &current,
                        AcquisitionOutcome::HttpStatus {
                            status: response.status,
                        },
                    )?;
                    continue 'candidates;
                }
                self.record(
                    request.logical_path(),
                    &current,
                    AcquisitionOutcome::Succeeded {
                        bytes: response.body.len(),
                    },
                )?;
                return Ok(BlobFetchResponse::ok(
                    current,
                    Box::new(Cursor::new(response.body)),
                ));
            }
        }
        Err(self.refuse(
            request.logical_path(),
            "",
            AcquisitionRefusal::AllSourcesFailed,
        ))
    }
}
