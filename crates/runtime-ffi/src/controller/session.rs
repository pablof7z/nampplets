//! Session lifecycle, mapped envelopes, and verified reads.

use std::{collections::BTreeSet, sync::Arc};

use nmp_native_runtime_app::{PlatformCommand, ProviderOperationId};
use nmp_native_runtime_core::{CapabilityRequirement, SessionId};

use super::{RuntimeController, support::installation_capability_requests};
use crate::{
    RuntimeExecutionProfile, VerifiedArtifact, VerifiedRead,
    projection::map_profile,
    support::{bump_signal, media_type_for},
};

#[uniffi::export]
impl RuntimeController {
    pub fn launch(&self, artifact: Arc<VerifiedArtifact>, profile: RuntimeExecutionProfile) {
        let capability_requests = match installation_capability_requests(&artifact) {
            Ok(requests) => requests,
            Err(error) => {
                self.record_refusal("invalid-capability-request", error);
                return;
            }
        };
        if capability_requests.len() > self.maximum_command_items {
            self.record_refusal(
                "required-domain-capacity",
                format!(
                    "verified capability profile has {} domains; the maximum is {}",
                    capability_requests.len(),
                    self.maximum_command_items
                ),
            );
            return;
        }
        let Some(principal) = artifact.principal.clone() else {
            self.record_refusal(
                "unsupported-manifest-identity",
                "launch target has no exact-build principal",
            );
            return;
        };
        let mut domains = BTreeSet::new();
        for request in capability_requests {
            if request.requirement == CapabilityRequirement::Required {
                domains.insert(request.capability);
            }
        }
        self.app.dispatch(PlatformCommand::Launch {
            principal,
            profile: map_profile(profile),
            required_domains: domains,
        });
        bump_signal(&self.signal);
    }

    pub fn stop(&self, session_id: u64) {
        self.app.dispatch(PlatformCommand::Stop {
            session: SessionId(session_id),
        });
        bump_signal(&self.signal);
    }

    /// Suspends one current session listed by its installed-build projection.
    /// Lifecycle policy and stale-session refusal remain inside RuntimeApp.
    pub fn suspend(&self, session_id: u64) {
        self.app.dispatch(PlatformCommand::Suspend {
            session: SessionId(session_id),
        });
        bump_signal(&self.signal);
    }

    /// Resumes one current suspended session. RuntimeApp validates the typed
    /// lifecycle transition and projects any refusal through state/events.
    pub fn resume(&self, session_id: u64) {
        self.app.dispatch(PlatformCommand::Resume {
            session: SessionId(session_id),
        });
        bump_signal(&self.signal);
    }

    pub fn crash(&self, session_id: u64, reason: String) {
        if reason.len() > 1_024 || reason.chars().any(char::is_control) {
            self.record_refusal(
                "invalid-crash-reason",
                "crash reason must be control-free and at most 1024 bytes",
            );
            return;
        }
        self.app.dispatch(PlatformCommand::Crash {
            session: SessionId(session_id),
            reason: Arc::from(reason),
        });
        bump_signal(&self.signal);
    }

    /// Resolves one Rust-retained provider write proposal. Native supplies
    /// only the bounded operation id and decision; the exact principal,
    /// account, correlation, and draft remain inside RuntimeApp.
    pub fn decide_provider_write(&self, operation_id: u64, approve: bool) {
        if operation_id == 0 {
            self.record_refusal(
                "invalid-provider-operation",
                "provider operation identifiers are positive",
            );
            return;
        }
        self.app.dispatch(PlatformCommand::DecideProviderWrite {
            operation: ProviderOperationId(operation_id),
            approve,
        });
        bump_signal(&self.signal);
    }

    pub fn mapped_envelope(&self, session_id: u64, bytes: Vec<u8>) {
        if bytes.len() > self.maximum_envelope_bytes {
            self.record_refusal(
                "envelope-too-large",
                format!(
                    "mapped envelope has {} bytes; the maximum is {}",
                    bytes.len(),
                    self.maximum_envelope_bytes
                ),
            );
            return;
        }
        self.app.dispatch(PlatformCommand::MappedEnvelope {
            session: SessionId(session_id),
            bytes: Arc::from(bytes),
        });
        bump_signal(&self.signal);
    }

    pub fn read_verified(
        &self,
        session_id: u64,
        logical_path: String,
        maximum_bytes: u64,
    ) -> VerifiedRead {
        if logical_path.len() > self.maximum_command_string_bytes {
            return VerifiedRead::Refused {
                refusal: self.refusal(
                    "logical-path-too-large",
                    format!(
                        "logical path exceeds {} bytes",
                        self.maximum_command_string_bytes
                    ),
                ),
            };
        }
        let maximum_bytes = match usize::try_from(maximum_bytes) {
            Ok(value) if value > 0 && value <= self.maximum_verified_read_bytes => value,
            _ => {
                return VerifiedRead::Refused {
                    refusal: self.refusal(
                        "invalid-read-limit",
                        format!(
                            "read limit must be 1..={}",
                            self.maximum_verified_read_bytes
                        ),
                    ),
                };
            }
        };
        let principal = self
            .app
            .snapshot()
            .sessions
            .iter()
            .find(|session| session.id == SessionId(session_id))
            .map(|session| session.principal.clone());
        let Some(principal) = principal else {
            return VerifiedRead::Refused {
                refusal: self.refusal("unknown-session", "no active mapped session"),
            };
        };
        let Some(artifact) = self.artifacts.lock().get(&principal).cloned() else {
            return VerifiedRead::Refused {
                refusal: self.refusal("unknown-artifact", "session artifact is not retained"),
            };
        };
        let Some(expected) = artifact
            .index()
            .entries()
            .find(|entry| entry.path() == logical_path)
            .map(|entry| entry.sha256().as_str().to_owned())
        else {
            return VerifiedRead::Refused {
                refusal: self.refusal(
                    "verified-read",
                    "logical path is not present in the sealed artifact index",
                ),
            };
        };
        match artifact.read_verified(&logical_path, maximum_bytes) {
            Ok(bytes) => VerifiedRead::Bytes {
                media_type: media_type_for(&logical_path).to_owned(),
                sha256: expected,
                bytes,
            },
            Err(error) => VerifiedRead::Refused {
                refusal: self.refusal("verified-read", error.to_string()),
            },
        }
    }
}
