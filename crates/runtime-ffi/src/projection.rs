//! Rust-owned projections between kernel types and the FFI boundary records.

use std::sync::Arc;

use nmp_native_artifact::ManifestCoordinate;
use nmp_native_nmp_adapter::{
    AccountLifecycleError, LocalAccountHandle, LocalAccountKind, LocalAccountSnapshot,
};
use nmp_native_providers::{ThemeProviderLimits, ThemeSnapshot};
use nmp_native_runtime_app::{
    PermissionDecisionController, PermissionPlatformAvailability, PermissionReviewView,
    PlatformEvent,
};
use nmp_native_runtime_core::{
    CapabilityRequirement, ExecutionProfile, GrantDecision, Sensitivity,
};

use crate::{
    ArtifactCoordinate, NativeAppearanceSnapshot, RuntimeAccountFailure, RuntimeAccountHandle,
    RuntimeAccountKind, RuntimeAccountSnapshot, RuntimeCatalogFailure, RuntimeEvent,
    RuntimeExactBuildCoordinate, RuntimeExecutionProfile, RuntimeGrantDecision,
    RuntimePermissionCapabilitySnapshot, RuntimePermissionDecisionController,
    RuntimePermissionDecisionOption, RuntimePermissionExistingDecision,
    RuntimePermissionPlatformAvailability, RuntimePermissionRequirement,
    RuntimePermissionReviewSnapshot, RuntimePermissionSensitivity,
};

pub(crate) fn theme_from_appearance(
    appearance: NativeAppearanceSnapshot,
) -> Result<ThemeSnapshot, String> {
    let background = match (
        appearance.dark,
        appearance.increased_contrast,
        appearance.reduced_transparency,
    ) {
        (true, true, _) | (true, _, true) => "#000000",
        (true, false, false) => "#1c1c1e",
        (false, true, _) | (false, _, true) => "#ffffff",
        (false, false, false) => "#f5f5f7",
    };
    let text = if appearance.dark {
        "#ffffff"
    } else {
        "#000000"
    };
    let primary = format!(
        "#{:02x}{:02x}{:02x}",
        appearance.accent_red, appearance.accent_green, appearance.accent_blue
    );
    let mode = if appearance.dark { "Dark" } else { "Light" };
    let contrast = if appearance.increased_contrast {
        " High Contrast"
    } else {
        ""
    };
    let transparency = if appearance.reduced_transparency {
        " Reduced Transparency"
    } else {
        ""
    };
    ThemeSnapshot::from_value(
        &serde_json::json!({
            "colors": {
                "background": background,
                "text": text,
                "primary": primary,
            },
            "title": format!("macOS {mode}{contrast}{transparency}"),
        }),
        ThemeProviderLimits::default(),
    )
    .map_err(|error| error.to_string())
}

pub(crate) fn map_coordinate(coordinate: ArtifactCoordinate) -> Result<ManifestCoordinate, String> {
    match coordinate {
        ArtifactCoordinate::Snapshot { event_id, author } => {
            ManifestCoordinate::snapshot(&event_id, &author)
        }
        ArtifactCoordinate::Root { author } => ManifestCoordinate::root(&author),
        ArtifactCoordinate::Named { author, d_tag } => ManifestCoordinate::named(&author, &d_tag),
    }
    .map_err(|error| error.to_string())
}

pub(crate) fn parse_catalog_coordinate(value: &str) -> Result<ManifestCoordinate, String> {
    if value.is_empty()
        || value.len() > 2_048
        || value.chars().any(char::is_control)
        || value.trim() != value
    {
        return Err(
            "coordinate must be 1..=2048 UTF-8 bytes without controls or surrounding whitespace"
                .to_owned(),
        );
    }
    let mut fields = value.splitn(3, ':');
    let kind = fields.next().unwrap_or_default();
    let first = fields
        .next()
        .ok_or_else(|| "coordinate is missing its author or event identifier".to_owned())?;
    let second = fields.next();
    let coordinate = match (kind, second) {
        ("5129", Some(author)) => ManifestCoordinate::snapshot(first, author),
        ("15129", None) => ManifestCoordinate::root(first),
        ("35129", Some(d_tag)) => ManifestCoordinate::named(first, d_tag),
        ("5129", None) => {
            return Err("snapshot coordinate must be 5129:event-id:author".to_owned());
        }
        ("35129", None) => {
            return Err("named coordinate must be 35129:author:d-tag".to_owned());
        }
        _ => {
            return Err(
                "supported coordinates are 5129:event-id:author, 15129:author, and 35129:author:d-tag"
                    .to_owned(),
            );
        }
    };
    coordinate.map_err(|error| error.to_string())
}

pub(crate) fn runtime_catalog_failure(
    code: impl Into<String>,
    detail: impl Into<String>,
) -> RuntimeCatalogFailure {
    RuntimeCatalogFailure {
        code: code.into(),
        detail: detail.into(),
        provenance: Vec::new(),
    }
}

pub(crate) fn map_profile(profile: RuntimeExecutionProfile) -> ExecutionProfile {
    match profile {
        RuntimeExecutionProfile::Legacy => ExecutionProfile::Legacy,
        RuntimeExecutionProfile::Renderer => ExecutionProfile::Renderer,
        RuntimeExecutionProfile::Hybrid => ExecutionProfile::Hybrid,
    }
}

pub(crate) fn grant_decision(decision: RuntimeGrantDecision) -> GrantDecision {
    match decision {
        RuntimeGrantDecision::Denied => GrantDecision::Denied,
        RuntimeGrantDecision::AskEveryTime => GrantDecision::AskEveryTime,
        RuntimeGrantDecision::AllowSession => GrantDecision::AllowSession,
        RuntimeGrantDecision::AllowExactBuild => GrantDecision::AllowExactBuild,
    }
}

fn project_grant_decision(decision: GrantDecision) -> RuntimePermissionExistingDecision {
    match decision {
        GrantDecision::Denied => RuntimePermissionExistingDecision::Denied,
        GrantDecision::AskEveryTime => RuntimePermissionExistingDecision::AskEveryTime,
        GrantDecision::AllowSession => RuntimePermissionExistingDecision::AllowSession,
        GrantDecision::AllowExactBuild => RuntimePermissionExistingDecision::AllowExactBuild,
        GrantDecision::Managed => RuntimePermissionExistingDecision::Managed,
    }
}

fn project_requested_grant_decision(decision: GrantDecision) -> Option<RuntimeGrantDecision> {
    match decision {
        GrantDecision::Denied => Some(RuntimeGrantDecision::Denied),
        GrantDecision::AskEveryTime => Some(RuntimeGrantDecision::AskEveryTime),
        GrantDecision::AllowSession => Some(RuntimeGrantDecision::AllowSession),
        GrantDecision::AllowExactBuild => Some(RuntimeGrantDecision::AllowExactBuild),
        GrantDecision::Managed => None,
    }
}

pub(crate) fn project_permission_review(
    review: PermissionReviewView,
) -> RuntimePermissionReviewSnapshot {
    // A Required capability with no registered provider on this runtime
    // build can never receive a decision (permission_decision_policy forces
    // it to Denied with every option invalid), so it must not permanently
    // block launch the way a genuinely available-but-denied capability does.
    // Launch drops such domains instead of injecting them; see
    // `RuntimeApp::launch`.
    let launch_permitted = review.capabilities.iter().all(|capability| {
        capability.requirement != CapabilityRequirement::Required
            || !matches!(
                capability.platform_availability,
                PermissionPlatformAvailability::Available
            )
            || capability.current_decision.allows_without_prompt()
    });
    RuntimePermissionReviewSnapshot {
        coordinate: RuntimeExactBuildCoordinate {
            manifest_author: review.principal.manifest_author().to_owned(),
            d_tag: review.principal.d_tag().to_owned(),
            aggregate_hash: review.principal.aggregate_hash().to_owned(),
        },
        revision: review.revision.to_string(),
        title: review.title.to_string(),
        capabilities: review
            .capabilities
            .into_iter()
            .map(|capability| RuntimePermissionCapabilitySnapshot {
                domain: capability.capability.as_str().to_owned(),
                requirement: match capability.requirement {
                    CapabilityRequirement::Required => RuntimePermissionRequirement::Required,
                    CapabilityRequirement::Optional => RuntimePermissionRequirement::Optional,
                },
                sensitivity: match capability.sensitivity {
                    Some(Sensitivity::Ordinary) => RuntimePermissionSensitivity::Ordinary,
                    Some(Sensitivity::Sensitive) => RuntimePermissionSensitivity::Sensitive,
                    None => RuntimePermissionSensitivity::Unknown,
                },
                dependencies: capability
                    .dependencies
                    .into_iter()
                    .map(|dependency| dependency.as_str().to_owned())
                    .collect(),
                platform_availability: match capability.platform_availability {
                    PermissionPlatformAvailability::Available => {
                        RuntimePermissionPlatformAvailability::Available
                    }
                    PermissionPlatformAvailability::Unknown { reason } => {
                        RuntimePermissionPlatformAvailability::Unknown {
                            reason: reason.to_string(),
                        }
                    }
                    PermissionPlatformAvailability::Unavailable { reason } => {
                        RuntimePermissionPlatformAvailability::Unavailable {
                            reason: reason.to_string(),
                        }
                    }
                },
                controller: match capability.controller {
                    PermissionDecisionController::User => RuntimePermissionDecisionController::User,
                    PermissionDecisionController::HostPolicy { reason } => {
                        RuntimePermissionDecisionController::HostPolicy {
                            reason: reason.to_string(),
                        }
                    }
                },
                existing_decision: project_grant_decision(capability.current_decision),
                is_granted: capability.is_granted,
                requested_decision: capability
                    .requested_decision
                    .and_then(project_requested_grant_decision),
                recommended_decision: capability
                    .recommended_decision
                    .and_then(project_requested_grant_decision),
                decision_options: capability
                    .decision_options
                    .into_iter()
                    .filter_map(|option| {
                        project_requested_grant_decision(option.decision).map(|decision| {
                            RuntimePermissionDecisionOption {
                                decision,
                                valid: option.valid,
                                invalid_reason: option
                                    .invalid_reason
                                    .map(|reason| reason.to_string()),
                            }
                        })
                    })
                    .collect(),
            })
            .collect(),
        read_only: review.read_only,
        launch_permitted,
    }
}

pub(crate) fn project_profile(profile: ExecutionProfile) -> RuntimeExecutionProfile {
    match profile {
        ExecutionProfile::Legacy => RuntimeExecutionProfile::Legacy,
        ExecutionProfile::Renderer => RuntimeExecutionProfile::Renderer,
        ExecutionProfile::Hybrid => RuntimeExecutionProfile::Hybrid,
    }
}

pub(crate) fn project_event(sequence: u64, event: &PlatformEvent) -> RuntimeEvent {
    let kind = match event {
        PlatformEvent::Installed { .. } => "installed",
        PlatformEvent::LibraryFilterChanged { .. } => "library-filter-changed",
        PlatformEvent::Uninstalled { .. } => "uninstalled",
        PlatformEvent::GrantChanged { .. } => "grant-changed",
        PlatformEvent::PermissionChangesApplied { .. } => "permission-changes-applied",
        PlatformEvent::SessionChanged(_) => "session-changed",
        PlatformEvent::EnvelopeHandled {
            session, response, ..
        } => {
            return RuntimeEvent {
                sequence,
                kind: "envelope-handled".to_owned(),
                detail: format!("{event:?}"),
                session_id: Some(session.0),
                response_json: response.as_ref().map(|value| value.as_str().to_owned()),
            };
        }
        PlatformEvent::EnvelopeIgnored { .. } => "envelope-ignored",
        PlatformEvent::NappletDiagnostic {
            session,
            level,
            message,
        } => {
            // Projected structurally rather than as a debug string: the point
            // of classifying in Rust is that native renders a typed fact
            // instead of re-parsing one.
            return RuntimeEvent {
                sequence,
                kind: "napplet-diagnostic".to_owned(),
                detail: level.as_str().to_owned(),
                session_id: Some(session.0),
                response_json: Some(message.clone()),
            };
        }
        PlatformEvent::ProviderOperationFinished { .. } => "provider-operation-finished",
        PlatformEvent::ProviderPush {
            session, envelope, ..
        } => {
            return RuntimeEvent {
                sequence,
                kind: "provider-push".to_owned(),
                detail: format!("{event:?}"),
                session_id: Some(session.0),
                response_json: Some(envelope.as_str().to_owned()),
            };
        }
        PlatformEvent::ProviderPushLaneClosed { session, .. } => {
            return RuntimeEvent {
                sequence,
                kind: "provider-push-lane-closed".to_owned(),
                detail: format!("{event:?}"),
                session_id: Some(session.0),
                response_json: None,
            };
        }
        PlatformEvent::BindingOpened { .. } => "binding-opened",
        PlatformEvent::BindingClosed { .. } => "binding-closed",
        PlatformEvent::WriteAccepted { .. } => "write-accepted",
        PlatformEvent::WorkspaceSaved { .. } => "workspace-saved",
        PlatformEvent::WorkspaceRestored { .. } => "workspace-restored",
        PlatformEvent::WorkspaceAssignmentChanged { .. } => "workspace-assignment-changed",
        PlatformEvent::ReceiptReattached { .. } => "receipt-reattached",
        PlatformEvent::ReceiptNotFound { .. } => "receipt-not-found",
        PlatformEvent::Refused(_) => "refused",
        PlatformEvent::Closed => "closed",
    };
    RuntimeEvent {
        sequence,
        kind: kind.to_owned(),
        detail: format!("{event:?}"),
        session_id: None,
        response_json: None,
    }
}

pub(crate) fn local_account_handle(handle: RuntimeAccountHandle) -> LocalAccountHandle {
    LocalAccountHandle {
        installation_id: handle.installation_id,
        account: nmp_native_runtime_core::AccountRef(Arc::from(handle.public_key)),
        kind: match handle.kind {
            RuntimeAccountKind::LocalSigner => LocalAccountKind::LocalSigner,
            RuntimeAccountKind::ReadOnly => LocalAccountKind::ReadOnly,
        },
    }
}

pub(crate) fn project_account_handle(handle: LocalAccountHandle) -> RuntimeAccountHandle {
    RuntimeAccountHandle {
        installation_id: handle.installation_id,
        public_key: handle.account.0.to_string(),
        kind: match handle.kind {
            LocalAccountKind::LocalSigner => RuntimeAccountKind::LocalSigner,
            LocalAccountKind::ReadOnly => RuntimeAccountKind::ReadOnly,
        },
    }
}

pub(crate) fn project_account_snapshot(snapshot: LocalAccountSnapshot) -> RuntimeAccountSnapshot {
    RuntimeAccountSnapshot {
        generation: snapshot.identity.generation,
        active_public_key: snapshot
            .identity
            .account
            .map(|account| account.0.to_string()),
        local_accounts: snapshot
            .installations
            .into_iter()
            .map(project_account_handle)
            .collect(),
    }
}

pub(crate) fn project_account_error(error: AccountLifecycleError) -> RuntimeAccountFailure {
    match error {
        AccountLifecycleError::Closed => RuntimeAccountFailure::Closed,
        AccountLifecycleError::InvalidSecretKey => RuntimeAccountFailure::InvalidSecretKey,
        AccountLifecycleError::InvalidPublicKey => RuntimeAccountFailure::InvalidPublicKey,
        AccountLifecycleError::Nip05ResolutionUnavailable => {
            RuntimeAccountFailure::Nip05ResolutionUnavailable
        }
        AccountLifecycleError::Capacity { limit } => RuntimeAccountFailure::Capacity {
            limit: limit as u64,
        },
        AccountLifecycleError::InstanceExhausted => RuntimeAccountFailure::InstanceExhausted,
        AccountLifecycleError::StaleInstallation => RuntimeAccountFailure::StaleInstallation,
        AccountLifecycleError::Failed { reason } => RuntimeAccountFailure::Failed {
            reason: reason.to_string(),
        },
    }
}
