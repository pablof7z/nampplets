use std::{collections::BTreeSet, fmt, sync::Arc};

use nmp_native_nap_bridge::ProviderPushError;
use nmp_native_runtime_core::{BoundedJson, Cancellation, Principal, SessionId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntentProviderLimits {
    pub maximum_sessions: usize,
    pub maximum_handlers: usize,
    pub maximum_archetypes: usize,
    pub maximum_candidates_per_archetype: usize,
    pub maximum_actions_per_handler: usize,
    pub maximum_conventions_per_handler: usize,
    pub maximum_pending_per_session: usize,
    pub maximum_pending_total: usize,
    pub maximum_payload_bytes: usize,
    pub maximum_response_bytes: usize,
    pub maximum_correlation_id_bytes: usize,
    pub maximum_text_bytes: usize,
    pub maximum_native_handle_bytes: usize,
}

impl Default for IntentProviderLimits {
    fn default() -> Self {
        Self {
            maximum_sessions: 64,
            maximum_handlers: 256,
            maximum_archetypes: 128,
            maximum_candidates_per_archetype: 32,
            maximum_actions_per_handler: 32,
            maximum_conventions_per_handler: 32,
            maximum_pending_per_session: 8,
            maximum_pending_total: 128,
            maximum_payload_bytes: 128 * 1024,
            maximum_response_bytes: 256 * 1024,
            maximum_correlation_id_bytes: 1_024,
            maximum_text_bytes: 1_024,
            maximum_native_handle_bytes: 256,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentHandlerDeclaration {
    pub archetype: Arc<str>,
    pub title: Option<Arc<str>>,
    pub actions: BTreeSet<Arc<str>>,
    pub conventions: BTreeSet<Arc<str>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RegisteredHandler {
    pub(super) principal: Principal,
    pub(super) declaration: IntentHandlerDeclaration,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntentCandidate {
    pub d_tag: Arc<str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<Arc<str>>,
    pub actions: Vec<Arc<str>>,
    pub conventions: Vec<Arc<str>>,
    pub is_default: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntentAvailability {
    pub archetype: Arc<str>,
    pub available: bool,
    pub candidates: Vec<IntentCandidate>,
    pub has_default: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IntentBehavior {
    #[serde(default)]
    pub focus: bool,
    #[serde(default)]
    pub new_window: bool,
    #[serde(default)]
    pub reuse: bool,
}

#[derive(Clone, Debug)]
pub struct IntentPolicyRequest {
    pub caller: Principal,
    pub session: SessionId,
    pub archetype: Arc<str>,
    pub action: Arc<str>,
    pub convention: Option<Arc<str>>,
    pub requested_handler: IntentHandlerRequest,
    pub behavior: IntentBehavior,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntentHandlerRequest {
    Default,
    Choose,
    Specific(Arc<str>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentPolicyDecision {
    pub allow: bool,
    pub allow_specific_handler: bool,
    pub confirmation_required: bool,
    pub reveal_candidates: bool,
}

pub trait IntentPolicy: Send + Sync + fmt::Debug {
    fn evaluate(&self, request: &IntentPolicyRequest) -> IntentPolicyDecision;
    fn allow_discovery(&self, caller: &Principal, archetype: &str) -> bool;
}

/// Conservative policy: dispatch requires native confirmation, and callers
/// cannot target a concrete dTag without a product-specific policy.
#[derive(Debug, Default)]
pub struct ConfirmEveryIntent;

impl IntentPolicy for ConfirmEveryIntent {
    fn evaluate(&self, _request: &IntentPolicyRequest) -> IntentPolicyDecision {
        IntentPolicyDecision {
            allow: true,
            allow_specific_handler: false,
            confirmation_required: true,
            reveal_candidates: true,
        }
    }

    fn allow_discovery(&self, _caller: &Principal, _archetype: &str) -> bool {
        true
    }
}

#[derive(Clone, Debug)]
pub struct IntentChoiceRequest {
    pub caller: Principal,
    pub session: SessionId,
    pub archetype: Arc<str>,
    pub action: Arc<str>,
    pub candidates: Vec<IntentCandidate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntentChoice {
    Selected(Arc<str>),
    Cancelled,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum IntentChoiceError {
    #[error("native intent chooser is saturated")]
    Saturated,
    #[error("native intent chooser is unavailable")]
    Unavailable,
}

/// A nonblocking choice seam. It returns a raw dTag; Rust validates the choice
/// against the exact candidate set before dispatch.
pub trait IntentChooser: Send + Sync + fmt::Debug {
    fn try_choose(&self, request: IntentChoiceRequest) -> Result<IntentChoice, IntentChoiceError>;
}

#[derive(Debug, Default)]
pub struct CancelIntentChoice;

impl IntentChooser for CancelIntentChoice {
    fn try_choose(&self, _request: IntentChoiceRequest) -> Result<IntentChoice, IntentChoiceError> {
        Ok(IntentChoice::Cancelled)
    }
}

#[derive(Clone, Debug)]
pub struct NativeIntentDispatch {
    pub token: IntentOperationToken,
    pub caller: Principal,
    pub session: SessionId,
    pub handler: Principal,
    pub archetype: Arc<str>,
    pub action: Arc<str>,
    pub convention: Option<Arc<str>>,
    pub payload: BoundedJson,
    pub behavior: IntentBehavior,
    pub confirmation_required: bool,
    pub cancellation: Cancellation,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum NativeIntentStartError {
    #[error("native intent dispatcher is saturated")]
    Saturated,
    #[error("native intent dispatcher is unavailable")]
    Unavailable,
    #[error("native intent session is closed")]
    Closed,
}

/// Native executes the selected target and reports raw completion. It never
/// chooses or rewrites a handler, action, convention, or payload.
pub trait NativeIntentDispatcher: Send + Sync + fmt::Debug {
    fn try_dispatch(
        &self,
        request: NativeIntentDispatch,
    ) -> Result<Arc<str>, NativeIntentStartError>;
    fn cancel(&self, native_handle: &str);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IntentOperationToken(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeIntentOutcome {
    Handled { window_id: Option<Arc<str>> },
    Cancelled,
    Failed { reason: NativeIntentFailureReason },
}

/// Why a native intent dispatch failed. The retry loop that drives dispatch
/// can distinguish these from each other -- collapsing them into one opaque
/// `Failed` hid whether the handler never launched, launched but its own JS
/// never reached `inc.subscribe`, its session vanished mid-dispatch, or the
/// push itself was refused, all of which need different debugging.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeIntentFailureReason {
    /// `launch_handler` itself refused: no verified artifact for the
    /// handler, or its own required-domain derivation failed.
    HandlerLaunchRefused,
    /// The handler session was launched but never reached a state where
    /// `inc.subscribe(convention, ...)` was observed, within the full poll
    /// budget. Distinct from "the session never came up at all".
    HandlerNeverSubscribed,
    /// `launch_handler` was accepted, but no session for the handler was
    /// ever observed `Launching`/`Running`/`Suspended` within the full poll
    /// budget -- a stuck launch, or a launched session that ended before
    /// this dispatch ever observed it running.
    HandlerNeverObservedRunning,
    /// The handler's session id was no longer a known INC session by the
    /// time the push was attempted (it ended between being observed
    /// running and the push landing).
    HandlerSessionEnded,
    /// The push itself was refused for a reason the transport reports.
    PushRefused { detail: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntentActivityOutcome {
    Started,
    Handled,
    Cancelled,
    Denied,
    Refused,
    PushRefused,
    LifecycleCancelled,
    CatalogChanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentActivity {
    pub principal: Principal,
    pub session: SessionId,
    pub action: Arc<str>,
    pub outcome: IntentActivityOutcome,
}

pub trait IntentActivitySink: Send + Sync + fmt::Debug {
    fn record(&self, fact: IntentActivity);
}

#[derive(Debug, Default)]
pub struct NoopIntentActivity;

impl IntentActivitySink for NoopIntentActivity {
    fn record(&self, _fact: IntentActivity) {}
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum IntentProviderBuildError {
    #[error("intent provider limits must be finite, non-zero, and internally consistent")]
    InvalidLimits,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum IntentCatalogError {
    #[error("handler declaration is invalid")]
    InvalidDeclaration,
    #[error("intent handler or archetype capacity is full")]
    Capacity,
    #[error("a different exact-build principal already owns this dTag")]
    DTagCollision,
    #[error("default handler is not registered for that archetype")]
    UnknownDefault,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum IntentCompletionError {
    #[error("unknown or already-completed intent operation")]
    UnknownOperation,
    #[error("intent result delivery was refused: {0}")]
    Push(ProviderPushError),
}
