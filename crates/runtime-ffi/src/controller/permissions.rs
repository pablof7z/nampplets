//! Exact-build permission review, atomic decision batches, and grants.

use std::{
    collections::BTreeSet,
    sync::{Arc, atomic::Ordering},
};

use nmp_native_runtime_app::{PermissionDecision, PlatformCommand, PlatformEvent};
use nmp_native_runtime_core::{Capability, GrantDecision, Principal, Sensitivity};

use super::RuntimeController;
use crate::{
    MAXIMUM_PERMISSION_DECISIONS, RuntimeExactBuildCoordinate, RuntimeGrantDecision,
    RuntimePermissionBatchUpdate, RuntimePermissionDecisionBatch, RuntimePermissionReviewResult,
    RuntimeRefusal, RuntimeSensitivity, VerifiedArtifact,
    projection::{grant_decision, project_permission_review},
    support::bump_signal,
};

#[uniffi::export]
impl RuntimeController {
    /// Returns one bounded Rust-owned review for an installed exact build.
    /// This never grants or launches the napplet.
    pub fn permission_review(
        &self,
        coordinate: RuntimeExactBuildCoordinate,
    ) -> RuntimePermissionReviewResult {
        if self.closed.load(Ordering::Acquire) {
            return RuntimePermissionReviewResult {
                review: None,
                refusal: Some(self.refusal("closed", "runtime is closed")),
            };
        }
        let principal = match Principal::new(
            coordinate.manifest_author,
            coordinate.d_tag,
            coordinate.aggregate_hash,
        ) {
            Ok(principal) => principal,
            Err(error) => {
                let refusal =
                    self.workspace_refusal("invalid-exact-build-coordinate", error.to_string());
                return RuntimePermissionReviewResult {
                    review: None,
                    refusal: Some(refusal),
                };
            }
        };
        match self.app.permission_review(&principal) {
            Ok(review) => RuntimePermissionReviewResult {
                review: Some(project_permission_review(review)),
                refusal: None,
            },
            Err(error) => {
                let refusal = self.workspace_refusal("permission-review", error.to_string());
                RuntimePermissionReviewResult {
                    review: None,
                    refusal: Some(refusal),
                }
            }
        }
    }

    /// Applies one complete exact-build decision set atomically in Rust.
    /// Success never launches the napplet; launch remains a separate command.
    pub fn apply_permission_decisions(
        &self,
        batch: RuntimePermissionDecisionBatch,
    ) -> RuntimePermissionBatchUpdate {
        if self.closed.load(Ordering::Acquire) {
            return RuntimePermissionBatchUpdate {
                applied: false,
                review: None,
                refusal: Some(self.refusal("closed", "runtime is closed")),
            };
        }
        if batch.decisions.is_empty() || batch.decisions.len() > MAXIMUM_PERMISSION_DECISIONS {
            let refusal = self.workspace_refusal(
                "invalid-permission-batch",
                format!(
                    "permission batch has {} decisions; the allowed range is 1..={MAXIMUM_PERMISSION_DECISIONS}",
                    batch.decisions.len()
                ),
            );
            return RuntimePermissionBatchUpdate {
                applied: false,
                review: None,
                refusal: Some(refusal),
            };
        }
        let principal = match Principal::new(
            batch.coordinate.manifest_author,
            batch.coordinate.d_tag,
            batch.coordinate.aggregate_hash,
        ) {
            Ok(principal) => principal,
            Err(error) => {
                let refusal =
                    self.workspace_refusal("invalid-exact-build-coordinate", error.to_string());
                return RuntimePermissionBatchUpdate {
                    applied: false,
                    review: None,
                    refusal: Some(refusal),
                };
            }
        };
        let mut domains = BTreeSet::new();
        let mut decisions = Vec::with_capacity(batch.decisions.len());
        for selection in batch.decisions {
            let capability = match Capability::new(selection.domain) {
                Ok(capability) => capability,
                Err(error) => {
                    let refusal =
                        self.workspace_refusal("invalid-permission-domain", error.to_string());
                    return RuntimePermissionBatchUpdate {
                        applied: false,
                        review: None,
                        refusal: Some(refusal),
                    };
                }
            };
            if !domains.insert(capability.clone()) {
                let refusal = self.workspace_refusal(
                    "duplicate-permission-domain",
                    format!("permission batch repeats capability {capability}"),
                );
                return RuntimePermissionBatchUpdate {
                    applied: false,
                    review: None,
                    refusal: Some(refusal),
                };
            }
            decisions.push(PermissionDecision {
                capability,
                decision: grant_decision(selection.decision),
            });
        }

        let cursor = self.app.events_after(0).newest_available;
        self.app.dispatch(PlatformCommand::ApplyPermissionBatch {
            principal: principal.clone(),
            decisions,
        });
        bump_signal(&self.signal);
        let events = self.app.events_after(cursor);
        let applied = events.events.iter().any(|event| {
            matches!(
                &event.event,
                PlatformEvent::PermissionBatchApplied {
                    principal: applied,
                    ..
                } if applied == &principal
            )
        });
        if !applied {
            let refusal = events
                .events
                .iter()
                .rev()
                .find_map(|event| match &event.event {
                    PlatformEvent::Refused(fact) if fact.principal.as_ref() == Some(&principal) => {
                        Some(RuntimeRefusal {
                            code: format!("{:?}", fact.code).to_ascii_lowercase(),
                            detail: fact.detail.to_string(),
                            occurred_at_millis: fact.occurred_at_millis,
                        })
                    }
                    _ => None,
                })
                .unwrap_or_else(|| {
                    self.refusal(
                        "permission-batch-refused",
                        "the runtime refused the permission batch without a matching outcome",
                    )
                });
            return RuntimePermissionBatchUpdate {
                applied: false,
                review: None,
                refusal: Some(refusal),
            };
        }
        match self.app.permission_review(&principal) {
            Ok(review) => RuntimePermissionBatchUpdate {
                applied: true,
                review: Some(project_permission_review(review)),
                refusal: None,
            },
            Err(error) => RuntimePermissionBatchUpdate {
                applied: true,
                review: None,
                refusal: Some(
                    self.workspace_refusal("permission-review-after-apply", error.to_string()),
                ),
            },
        }
    }

    pub fn set_grant(
        &self,
        artifact: Arc<VerifiedArtifact>,
        capability: String,
        sensitivity: RuntimeSensitivity,
        decision: RuntimeGrantDecision,
    ) {
        let Some(principal) = artifact.principal.clone() else {
            self.record_refusal(
                "unsupported-manifest-identity",
                "grant target has no exact-build principal",
            );
            return;
        };
        let capability = match Capability::new(capability) {
            Ok(capability) => capability,
            Err(error) => {
                self.record_refusal("invalid-capability", error.to_string());
                return;
            }
        };
        self.app.dispatch(PlatformCommand::SetGrant {
            principal,
            capability,
            sensitivity: match sensitivity {
                RuntimeSensitivity::Ordinary => Sensitivity::Ordinary,
                RuntimeSensitivity::Sensitive => Sensitivity::Sensitive,
            },
            decision: match decision {
                RuntimeGrantDecision::Denied => GrantDecision::Denied,
                RuntimeGrantDecision::AskEveryTime => GrantDecision::AskEveryTime,
                RuntimeGrantDecision::AllowSession => GrantDecision::AllowSession,
                RuntimeGrantDecision::AllowExactBuild => GrantDecision::AllowExactBuild,
            },
        });
        bump_signal(&self.signal);
    }

    pub fn revoke(&self, artifact: Arc<VerifiedArtifact>, capability: String) {
        let Some(principal) = artifact.principal.clone() else {
            self.record_refusal(
                "unsupported-manifest-identity",
                "revocation target has no exact-build principal",
            );
            return;
        };
        let capability = match Capability::new(capability) {
            Ok(capability) => capability,
            Err(error) => {
                self.record_refusal("invalid-capability", error.to_string());
                return;
            }
        };
        self.app.dispatch(PlatformCommand::Revoke {
            principal,
            capability,
        });
        bump_signal(&self.signal);
    }
}
