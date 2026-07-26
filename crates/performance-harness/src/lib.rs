//! Bounded, tooling-only producers for performance evidence v1.

mod clock;
mod environment;
mod result;
mod statistics;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use clock::{MonotonicClock, SystemMonotonicClock};
pub use environment::{Environment, MeasurementAvailability};
pub use result::{
    BuildIdentity, EvidenceIdentity, Failure, FixtureIdentity, Protocol, Refusal, ResultArtifact,
    RunState, Sample,
};
pub use statistics::{Distribution, OutcomeCounts, ProducerSummary, Variance};

pub const RESULT_SCHEMA_ID: &str = "urn:nampplets:performance:result:v1";
pub const COMPARISON_SCHEMA_ID: &str = "urn:nampplets:performance:comparison:v1";
pub const MAX_SAMPLES: usize = 10_000;
pub const MAX_WARMUPS: usize = 1_000;
pub const MAX_SAMPLE_DEADLINE_NS: u64 = 300_000_000_000;
pub const MAX_RUN_DEADLINE_NS: u64 = 7_200_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttemptPhase {
    Warmup,
    Measured { sequence: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttemptOutcome {
    Success,
    Refused(Refusal),
    Failed(Failure),
}

#[derive(Debug)]
pub struct Harness<C> {
    clock: C,
}

impl<C: MonotonicClock> Harness<C> {
    pub fn new(clock: C) -> Self {
        Self { clock }
    }

    pub fn run(
        &self,
        run_id: impl Into<String>,
        identity: EvidenceIdentity,
        build: BuildIdentity,
        mut attempt: impl FnMut(AttemptPhase) -> AttemptOutcome,
    ) -> Result<ResultArtifact, HarnessError> {
        validate_protocol(&identity.protocol)?;
        let run_started = self.clock.now_ns();
        for _ in 0..identity.protocol.warmup_count {
            let started = self.clock.now_ns();
            let _ = attempt(AttemptPhase::Warmup);
            let finished = self.clock.now_ns();
            checked_elapsed(started, finished)?;
            enforce_run_deadline(run_started, finished, identity.protocol.run_deadline_ns)?;
        }

        let mut samples = Vec::with_capacity(identity.protocol.sample_count);
        for sequence in 0..identity.protocol.sample_count {
            let started = self.clock.now_ns();
            let outcome = attempt(AttemptPhase::Measured { sequence });
            let finished = self.clock.now_ns();
            let duration_ns = checked_elapsed(started, finished)?;
            enforce_run_deadline(run_started, finished, identity.protocol.run_deadline_ns)?;
            let sample = if duration_ns >= identity.protocol.per_sample_deadline_ns {
                Sample::DeadlineExceeded {
                    sequence,
                    duration_ns,
                    cpu_time_ns: None,
                    peak_rss_bytes: None,
                }
            } else {
                Sample::from_attempt(sequence, duration_ns, outcome)
            };
            samples.push(sample);
        }
        ResultArtifact::new(run_id, identity, build, samples)
    }
}

fn validate_protocol(protocol: &Protocol) -> Result<(), HarnessError> {
    if protocol.sample_count == 0 || protocol.sample_count > MAX_SAMPLES {
        return Err(HarnessError::SampleLimit);
    }
    if protocol.warmup_count > MAX_WARMUPS {
        return Err(HarnessError::WarmupLimit);
    }
    if protocol.per_sample_deadline_ns == 0
        || protocol.per_sample_deadline_ns > MAX_SAMPLE_DEADLINE_NS
    {
        return Err(HarnessError::SampleDeadlineLimit);
    }
    if protocol.run_deadline_ns == 0 || protocol.run_deadline_ns > MAX_RUN_DEADLINE_NS {
        return Err(HarnessError::RunDeadlineLimit);
    }
    Ok(())
}

fn checked_elapsed(started: u64, finished: u64) -> Result<u64, HarnessError> {
    finished
        .checked_sub(started)
        .ok_or(HarnessError::ClockRegressed)
}

fn enforce_run_deadline(started: u64, finished: u64, deadline_ns: u64) -> Result<(), HarnessError> {
    if checked_elapsed(started, finished)? >= deadline_ns {
        Err(HarnessError::RunDeadlineExceeded)
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum HarnessError {
    #[error("measured sample count is outside the v1 safety bound")]
    SampleLimit,
    #[error("warmup count is outside the v1 safety bound")]
    WarmupLimit,
    #[error("per-sample deadline is outside the v1 safety bound")]
    SampleDeadlineLimit,
    #[error("run deadline is outside the v1 safety bound")]
    RunDeadlineLimit,
    #[error("monotonic clock regressed")]
    ClockRegressed,
    #[error("run deadline elapsed; no partial artifact is emitted")]
    RunDeadlineExceeded,
    #[error("result artifact is inconsistent: {0}")]
    InvalidResult(&'static str),
    #[error("canonical evidence serialization failed: {0}")]
    Serialization(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ComparisonSummary {
    disposition: String,
    mismatch_codes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ConfidenceReason {
    code: String,
    detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Confidence {
    disposition: String,
    reason: ConfidenceReason,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparisonArtifact {
    schema_id: String,
    comparison_id: String,
    baseline: result::ResultReference,
    candidate: result::ResultReference,
    producer_summary: ComparisonSummary,
    confidence: Confidence,
    checksum_sha256: String,
}

impl ComparisonArtifact {
    pub fn observed(
        comparison_id: impl Into<String>,
        baseline: &ResultArtifact,
        candidate: &ResultArtifact,
    ) -> Result<Self, HarnessError> {
        let mismatches = mismatch_codes(&baseline.identity, &candidate.identity);
        let incomparable = !mismatches.is_empty();
        let mut artifact = Self {
            schema_id: COMPARISON_SCHEMA_ID.to_owned(),
            comparison_id: comparison_id.into(),
            baseline: baseline.reference()?,
            candidate: candidate.reference()?,
            producer_summary: ComparisonSummary {
                disposition: if incomparable {
                    "incomparable"
                } else {
                    "observed_only"
                }
                .to_owned(),
                mismatch_codes: mismatches,
            },
            confidence: Confidence {
                disposition: "not_evaluated".to_owned(),
                reason: ConfidenceReason {
                    code: if incomparable {
                        "incomparable_inputs"
                    } else {
                        "no_ratified_method"
                    }
                    .to_owned(),
                    detail: "No confidence method is ratified for this evidence.".to_owned(),
                },
            },
            checksum_sha256: String::new(),
        };
        artifact.checksum_sha256 = result::artifact_checksum(&artifact)?;
        Ok(artifact)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, HarnessError> {
        result::canonical_value(self)
    }
}

fn mismatch_codes(left: &EvidenceIdentity, right: &EvidenceIdentity) -> Vec<String> {
    let pairs = [
        (
            left.benchmark_id != right.benchmark_id,
            "benchmark_mismatch",
        ),
        (left.state != right.state, "state_mismatch"),
        (
            left.reset_scopes != right.reset_scopes,
            "reset_scope_mismatch",
        ),
        (left.fixture != right.fixture, "fixture_mismatch"),
        (left.protocol != right.protocol, "protocol_mismatch"),
        (left.build_mode != right.build_mode, "build_mode_mismatch"),
        (left.toolchain != right.toolchain, "toolchain_mismatch"),
        (
            left.environment.environment_class != right.environment.environment_class,
            "environment_class_mismatch",
        ),
        (left.environment.os != right.environment.os, "os_mismatch"),
        (
            left.environment.hardware != right.environment.hardware,
            "hardware_mismatch",
        ),
        (
            left.environment.power_state != right.environment.power_state,
            "power_state_mismatch",
        ),
        (
            left.environment.thermal_state != right.environment.thermal_state,
            "thermal_state_mismatch",
        ),
        (
            left.environment.measurement_availability != right.environment.measurement_availability,
            "measurement_availability_mismatch",
        ),
    ];
    pairs
        .into_iter()
        .filter(|&(different, _)| different)
        .map(|(_, code)| code.to_owned())
        .collect()
}
