//! Exact-build permission review records exchanged with native presentation.

use crate::{RuntimeExactBuildCoordinate, RuntimeRefusal};

#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum RuntimeExecutionProfile {
    Legacy,
    Renderer,
    Hybrid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimeGrantDecision {
    Denied,
    AskEveryTime,
    AllowSession,
    AllowExactBuild,
}

#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum RuntimeSensitivity {
    Ordinary,
    Sensitive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimePermissionRequirement {
    Required,
    Optional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimePermissionSensitivity {
    Ordinary,
    Sensitive,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimePermissionPlatformAvailability {
    Available,
    Unknown { reason: String },
    Unavailable { reason: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RuntimePermissionExistingDecision {
    Denied,
    AskEveryTime,
    AllowSession,
    AllowExactBuild,
    Managed,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimePermissionDecisionOption {
    pub decision: RuntimeGrantDecision,
    pub valid: bool,
    pub invalid_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimePermissionCapabilitySnapshot {
    pub domain: String,
    pub requirement: RuntimePermissionRequirement,
    pub sensitivity: RuntimePermissionSensitivity,
    pub dependencies: Vec<String>,
    pub platform_availability: RuntimePermissionPlatformAvailability,
    pub existing_decision: RuntimePermissionExistingDecision,
    pub requested_decision: Option<RuntimeGrantDecision>,
    pub decision_options: Vec<RuntimePermissionDecisionOption>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimePermissionReviewSnapshot {
    pub coordinate: RuntimeExactBuildCoordinate,
    pub title: String,
    pub capabilities: Vec<RuntimePermissionCapabilitySnapshot>,
    pub launch_permitted: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimePermissionReviewResult {
    pub review: Option<RuntimePermissionReviewSnapshot>,
    pub refusal: Option<RuntimeRefusal>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimePermissionDecisionSelection {
    pub domain: String,
    pub decision: RuntimeGrantDecision,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimePermissionDecisionBatch {
    pub coordinate: RuntimeExactBuildCoordinate,
    pub decisions: Vec<RuntimePermissionDecisionSelection>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimePermissionBatchUpdate {
    pub applied: bool,
    pub review: Option<RuntimePermissionReviewSnapshot>,
    pub refusal: Option<RuntimeRefusal>,
}
