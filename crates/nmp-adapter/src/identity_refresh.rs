//! Bounded network refresh policy for public-identity reads.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use nmp::{AccessContext, Demand, Filter, LiveQuery, SourceAuthority, SourceStatus, Subscription};
use nmp_native_runtime_core::{Cancellation, PublicIdentityError};

const NETWORK_REFRESH_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) fn public_identity_live_query(filter: Filter) -> Result<LiveQuery, PublicIdentityError> {
    // Identity reads ask the operator-configured public lanes for generic
    // public facts. A bare `from_filter` would classify this author-bearing
    // selection as AuthorOutboxes, silently excluding app relays that cache
    // canonical profile/contact events.
    Demand::new(filter, SourceAuthority::Public, AccessContext::Public)
        .map(LiveQuery)
        .map_err(|error| PublicIdentityError::Failed {
            reason: Arc::from(error.to_string()),
        })
}

pub(crate) fn receive_identity_frame(
    subscription: Subscription,
    cancellation: &Cancellation,
    closed: &AtomicBool,
    network_refresh: bool,
) -> Result<nmp::Frame, PublicIdentityError> {
    let mut frame = subscription
        .recv()
        .map_err(|_| observation_closed_error(closed, "before its first frame"))?;
    if !network_refresh || identity_frame_is_ready(&frame) {
        return Ok(frame);
    }

    let deadline = Instant::now() + NETWORK_REFRESH_TIMEOUT;
    let cancel_handle = subscription.cancel_handle();
    let _cancellation_wakeup = cancellation
        .register_wakeup(move || cancel_handle.cancel())
        .map_err(|error| match error {
            nmp_native_runtime_core::CancellationWakeError::Capacity { capacity } => {
                PublicIdentityError::CancellationCapacity { capacity }
            }
        })?;

    loop {
        if cancellation.is_cancelled() {
            return Err(PublicIdentityError::Cancelled);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(frame);
        }
        match subscription.recv_timeout(remaining) {
            Ok(next) => {
                frame = next;
                if identity_frame_is_ready(&frame) {
                    return Ok(frame);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return Ok(frame),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(if cancellation.is_cancelled() {
                    PublicIdentityError::Cancelled
                } else {
                    observation_closed_error(closed, "during network refresh")
                });
            }
        }
    }
}

fn identity_frame_is_ready(frame: &nmp::Frame) -> bool {
    let Some(window) = &frame.window else {
        return true;
    };
    if !window.rows.is_empty() {
        return true;
    }
    if frame.evidence.sources.is_empty() {
        return !frame.evidence.shortfall.is_empty();
    }
    frame.evidence.sources.iter().all(|source| {
        source.reconciled_through.is_some()
            || matches!(
                source.status,
                SourceStatus::AuthDenied | SourceStatus::Error
            )
    })
}

fn observation_closed_error(closed: &AtomicBool, phase: &str) -> PublicIdentityError {
    if closed.load(Ordering::Acquire) {
        PublicIdentityError::Closed
    } else {
        PublicIdentityError::Failed {
            reason: Arc::from(format!("NMP identity observation closed {phase}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        sync::Arc,
        thread,
        time::{Duration, Instant},
    };

    use nmp::{
        AcquisitionEvidence, Binding, EngineConfig, Frame, ShortfallFact, WindowContents,
        WindowLoad,
    };
    use nmp_native_runtime_core::{
        PublicIdentityDataPlane, PublicIdentityQuery, PublicIdentityReadLimits,
    };

    use super::*;
    use crate::NmpDataPlane;

    #[test]
    fn identity_reads_use_operator_public_lanes_even_with_an_author_filter() {
        let query = public_identity_live_query(Filter {
            kinds: Some(BTreeSet::from([0])),
            authors: Some(Binding::Literal(BTreeSet::from([
                "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5".to_owned(),
            ]))),
            ..Filter::default()
        })
        .unwrap();

        assert_eq!(query.0.source, SourceAuthority::Public);
        assert_eq!(query.0.access, AccessContext::Public);
    }

    #[test]
    fn terminal_empty_evidence_finishes_without_waiting_for_the_deadline() {
        let frame = Frame {
            deltas: Vec::new(),
            window: Some(WindowContents {
                rows: Vec::new(),
                load: WindowLoad::Idle,
            }),
            evidence: AcquisitionEvidence {
                sources: Vec::new(),
                shortfall: vec![ShortfallFact::NoResolvedDemand],
            },
        };

        assert!(identity_frame_is_ready(&frame));
    }

    #[test]
    fn configured_identity_read_keeps_the_empty_observation_until_cancelled() {
        let plane = NmpDataPlane::open(
            EngineConfig {
                app_relays: vec!["ws://127.0.0.1:9".to_owned()],
                allowed_local_relay_hosts: vec!["127.0.0.1".to_owned()],
                ..EngineConfig::default()
            },
            2,
        )
        .unwrap();
        let frozen = plane
            .set_active_public_identity(Some(nmp_native_runtime_core::AccountRef(Arc::from(
                "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5",
            ))))
            .unwrap();
        let cancellation = Cancellation::new();
        let cancel = cancellation.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            cancel.cancel();
        });
        let started = Instant::now();
        assert!(matches!(
            plane.read_public_identity(
                &frozen,
                PublicIdentityQuery::Profile,
                &cancellation,
                PublicIdentityReadLimits {
                    maximum_items: 8,
                    maximum_sources: 8,
                    maximum_frame_bytes: 16 * 1024,
                },
            ),
            Err(PublicIdentityError::Cancelled)
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
        plane.close();
    }
}
