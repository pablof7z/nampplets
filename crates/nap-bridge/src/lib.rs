//! Validated NAP envelope dispatch and provider ownership.
//!
//! Source-window identity is established by the platform trust boundary. This
//! crate accepts only the already-mapped [`SessionContext`] and never reads a
//! principal or session identifier from an untrusted payload.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};

use nmp_native_runtime_core::{
    ApprovedWrite, BoundedJson, Cancellation, Capability, ExecutionProfile, GrantDecision,
    GrantLedger, Principal, ReceiptEventSink, ResourceClass, ResourceRefusal, ResourceTracker,
    SessionId, WorkLease,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

mod outbound;

use outbound::OutboundMailbox;
pub use outbound::{
    ProviderPush, ProviderPushBatch, ProviderPushError, ProviderPushLimits, ProviderPushObserver,
    ProviderPushSender, ProviderPushTermination, SourceWindowId,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeLimits {
    pub maximum_providers: usize,
    pub maximum_actions_per_provider: usize,
    pub maximum_dependencies_per_provider: usize,
    pub maximum_envelope_bytes: usize,
    pub maximum_response_bytes: usize,
    pub maximum_sessions: usize,
    pub message_burst: u32,
    pub message_refill_per_second: u32,
    pub provider_pushes: ProviderPushLimits,
}

impl Default for BridgeLimits {
    fn default() -> Self {
        Self {
            maximum_providers: 64,
            maximum_actions_per_provider: 64,
            maximum_dependencies_per_provider: 16,
            maximum_envelope_bytes: 256 * 1024,
            maximum_response_bytes: 512 * 1024,
            maximum_sessions: 64,
            message_burst: 120,
            message_refill_per_second: 60,
            provider_pushes: ProviderPushLimits::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionContext {
    pub id: SessionId,
    pub principal: Principal,
    pub profile: ExecutionProfile,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    #[serde(rename = "type")]
    pub message_type: String,
    #[serde(default)]
    pub id: Option<String>,
    /// Provider fields are the remaining top-level NAP message fields.
    ///
    /// The pinned provider protocols do not wrap arguments in a synthetic
    /// `payload` object. A field literally named `payload` therefore remains
    /// an ordinary provider-owned field instead of gaining bridge semantics.
    #[serde(flatten)]
    pub fields: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderPlatformAvailability {
    Available,
    Unavailable { reason: Arc<str> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderDescriptor {
    pub domain: Capability,
    pub protocol_versions: BTreeSet<Arc<str>>,
    pub actions: BTreeSet<Arc<str>>,
    pub sensitive: bool,
    pub dependencies: BTreeSet<Capability>,
    pub platform_availability: ProviderPlatformAvailability,
}

pub trait Provider: Send + Sync + fmt::Debug {
    fn descriptor(&self) -> &ProviderDescriptor;
    fn call(&self, request: ProviderRequest) -> Result<ProviderCall, ProviderError>;

    /// Called once after the bridge has atomically bound this exact provider
    /// lane to a trusted mapped session. Providers may retain `outbound` for
    /// bounded unsolicited messages.
    fn session_opened(&self, _session: ProviderSession) -> Result<(), ProviderError> {
        Ok(())
    }

    /// Called once after the exact session completes its shell handshake.
    fn session_ready(&self, _session: &ProviderSessionContext) -> Result<(), ProviderError> {
        Ok(())
    }

    /// Called on stop, crash, open rollback, and runtime close. Cleanup must
    /// be idempotent and nonblocking.
    fn session_closed(&self, _session: &ProviderSessionContext, _reason: ProviderSessionEnd) {}

    /// Called after the outbound capability lane is closed and active work is
    /// cancelled, so a provider cannot race another push into revocation.
    fn session_revoked(&self, _session: &ProviderSessionContext) {}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderSessionContext {
    pub principal: Principal,
    pub session: SessionId,
    pub source_window: SourceWindowId,
    pub profile: ExecutionProfile,
}

#[derive(Clone, Debug)]
pub struct ProviderSession {
    pub context: ProviderSessionContext,
    pub outbound: ProviderPushSender,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderSessionEnd {
    Stopped,
    Crashed,
    OpenFailed,
    RuntimeClosed,
}

#[derive(Debug)]
pub struct ProviderRequest {
    pub principal: Principal,
    pub session: SessionId,
    pub action: Arc<str>,
    pub correlation_id: Option<Arc<str>>,
    pub payload: Value,
    /// The provider must move this lease into any active stream owner. A
    /// completed one-shot simply lets it drop.
    pub work: WorkLease,
}

#[derive(Debug)]
pub struct ProviderCall {
    pub response: Option<BoundedJson>,
    operation: Option<ProviderOperation>,
    write_proposal: Option<Box<ProviderWriteProposal>>,
}

impl ProviderCall {
    pub fn completed(response: Option<BoundedJson>) -> Self {
        Self {
            response,
            operation: None,
            write_proposal: None,
        }
    }

    /// Returns an active operation whose work permit remains charged until the
    /// caller explicitly completes or cancels it (or drops the response).
    pub fn streaming(response: Option<BoundedJson>, work: WorkLease) -> Self {
        Self {
            response,
            operation: Some(ProviderOperation::new(work)),
            write_proposal: None,
        }
    }

    /// Returns an exact write proposal for native review.
    ///
    /// Constructing a proposal does not accept a durable write. The caller
    /// must approve the exact [`ApprovedWrite`], convert the one-shot
    /// completion into a receipt sink, and pass both through the runtime's
    /// single `accept_write` call.
    pub fn proposed_write(
        response: Option<BoundedJson>,
        write: ApprovedWrite,
        completion: Box<dyn ProviderWriteCompletion>,
        work: WorkLease,
    ) -> Self {
        Self {
            response,
            operation: None,
            write_proposal: Some(Box::new(ProviderWriteProposal {
                write: Some(write),
                completion: Some(completion),
                work: Some(work),
            })),
        }
    }

    pub fn operation(&self) -> Option<&ProviderOperation> {
        self.operation.as_ref()
    }

    pub fn take_operation(&mut self) -> Option<ProviderOperation> {
        self.operation.take()
    }

    pub fn write_proposal(&self) -> Option<&ProviderWriteProposal> {
        self.write_proposal.as_deref()
    }

    pub fn take_write_proposal(&mut self) -> Option<ProviderWriteProposal> {
        self.write_proposal.take().map(|proposal| *proposal)
    }

    pub fn is_active(&self) -> bool {
        self.operation.is_some() || self.write_proposal.is_some()
    }
}

/// One exact provider-originated write awaiting native approval.
///
/// The proposal retains its admitted-work lease until it is approved,
/// refused, or dropped. Consuming it for approval transfers the exact write
/// and its one-shot NAP completion together, preventing either half from
/// being accidentally reused with another approval.
#[derive(Debug)]
pub struct ProviderWriteProposal {
    pub write: Option<ApprovedWrite>,
    completion: Option<Box<dyn ProviderWriteCompletion>>,
    work: Option<WorkLease>,
}

impl ProviderWriteProposal {
    pub fn into_parts(mut self) -> (ApprovedWrite, Box<dyn ProviderWriteCompletion>, WorkLease) {
        let write = self
            .write
            .take()
            .expect("a retained write proposal always owns its approved write");
        let completion = self
            .completion
            .take()
            .expect("a retained write proposal always owns its completion");
        let work = self
            .work
            .take()
            .expect("a retained write proposal always owns its work lease");
        (write, completion, work)
    }

    pub fn refuse(mut self, reason: Arc<str>) {
        if let Some(completion) = self.completion.take() {
            completion.refused(reason);
        }
        if let Some(work) = self.work.take() {
            work.cancellation().cancel();
        }
    }
}

impl Drop for ProviderWriteProposal {
    fn drop(&mut self) {
        if let Some(work) = self.work.take() {
            work.cancellation().cancel();
        }
    }
}

/// One-shot continuation for a provider write after native approval.
///
/// The completion becomes a receipt sink before the runtime calls
/// `HostDataPlane::accept_write`, allowing the app to fan out its own receipt
/// projection and the provider's protocol result through one observation.
pub trait ProviderWriteCompletion: Send + Sync + fmt::Debug {
    fn into_receipt_sink(self: Box<Self>) -> Arc<dyn ReceiptEventSink>;
    fn refused(self: Box<Self>, reason: Arc<str>);
}

/// The lifecycle owner for one active provider operation.
///
/// Providers clone the cancellation signal before returning the operation and
/// stop their native work when it is signalled. The work permit remains
/// charged while this value is retained. Dropping it is a cancellation path,
/// while [`ProviderOperation::complete`] records a normal terminal path.
#[derive(Debug)]
pub struct ProviderOperation {
    work: Option<WorkLease>,
}

impl ProviderOperation {
    fn new(work: WorkLease) -> Self {
        Self { work: Some(work) }
    }

    pub fn cancellation(&self) -> &Cancellation {
        self.work
            .as_ref()
            .expect("an owned provider operation always retains its work lease")
            .cancellation()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation().is_cancelled()
    }

    pub fn complete(mut self) {
        self.work.take();
    }

    pub fn cancel(mut self) {
        if let Some(work) = self.work.take() {
            work.cancellation().cancel();
        }
    }
}

impl Drop for ProviderOperation {
    fn drop(&mut self) {
        if let Some(work) = self.work.take() {
            work.cancellation().cancel();
        }
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ProviderError {
    #[error("invalid {domain}.{action} payload: {reason}")]
    InvalidPayload {
        domain: Arc<str>,
        action: Arc<str>,
        reason: Arc<str>,
    },
    #[error("{domain}.{action} was denied: {reason}")]
    Denied {
        domain: Arc<str>,
        action: Arc<str>,
        reason: Arc<str>,
    },
    #[error("{domain}.{action} failed: {reason}")]
    Failed {
        domain: Arc<str>,
        action: Arc<str>,
        reason: Arc<str>,
    },
}

pub trait ActivitySink: Send + Sync + fmt::Debug {
    fn record(&self, fact: ProviderActivity);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderActivity {
    pub principal: Principal,
    pub session: SessionId,
    pub domain: Capability,
    pub action: Arc<str>,
    pub outcome: ActivityOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityOutcome {
    Completed,
    Active,
    Refused,
}

#[derive(Debug)]
pub struct ProviderRegistry {
    limits: BridgeLimits,
    providers: BTreeMap<Capability, Arc<dyn Provider>>,
    resources: Arc<ResourceTracker>,
    grants: Arc<GrantLedger>,
    activity: Arc<dyn ActivitySink>,
    state: Mutex<BridgeState>,
}

#[derive(Debug, Default)]
struct BridgeState {
    sessions: BTreeMap<SessionId, SessionSlot>,
    dispatched: u64,
    ignored_unknown: u64,
    refusals: u64,
    throttles: u64,
}

#[derive(Clone, Debug)]
struct SessionSlot {
    principal: Principal,
    profile: ExecutionProfile,
    bucket: TokenBucket,
    source_window: SourceWindowId,
    domains: BTreeSet<Capability>,
    ready: bool,
    outbound: Arc<OutboundMailbox>,
}

#[derive(Clone, Debug)]
struct TokenBucket {
    tokens_milli: u64,
    updated_at_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectionPlan {
    principal: Principal,
    profile: ExecutionProfile,
    domains: BTreeSet<Capability>,
}

impl InjectionPlan {
    pub fn profile(&self) -> ExecutionProfile {
        self.profile
    }

    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    pub fn domains(&self) -> &BTreeSet<Capability> {
        &self.domains
    }

    pub fn exposes(&self, capability: &Capability) -> bool {
        self.domains.contains(capability)
    }
}

impl ProviderRegistry {
    pub fn new(
        limits: BridgeLimits,
        resources: Arc<ResourceTracker>,
        grants: Arc<GrantLedger>,
        activity: Arc<dyn ActivitySink>,
    ) -> Result<Self, BridgeError> {
        if limits.maximum_providers == 0
            || limits.maximum_actions_per_provider == 0
            || limits.maximum_dependencies_per_provider == 0
            || limits.maximum_envelope_bytes == 0
            || limits.maximum_response_bytes == 0
            || limits.maximum_sessions == 0
            || limits.message_burst == 0
            || limits.message_refill_per_second == 0
            || !limits.provider_pushes.validate()
        {
            return Err(BridgeError::InvalidLimits);
        }
        Ok(Self {
            limits,
            providers: BTreeMap::new(),
            resources,
            grants,
            activity,
            state: Mutex::new(BridgeState::default()),
        })
    }

    /// Registration is the advertisement boundary. An unavailable or
    /// non-conformant implementation must not be registered.
    pub fn register(&mut self, provider: Arc<dyn Provider>) -> Result<(), BridgeError> {
        let descriptor = provider.descriptor();
        if descriptor.actions.is_empty()
            || descriptor.actions.len() > self.limits.maximum_actions_per_provider
            || descriptor.dependencies.len() > self.limits.maximum_dependencies_per_provider
            || descriptor.dependencies.contains(&descriptor.domain)
        {
            return Err(BridgeError::InvalidProvider {
                domain: descriptor.domain.clone(),
            });
        }
        if let ProviderPlatformAvailability::Unavailable { reason } =
            &descriptor.platform_availability
        {
            return Err(BridgeError::ProviderUnavailable {
                domain: descriptor.domain.clone(),
                reason: Arc::clone(reason),
            });
        }
        if self.providers.contains_key(&descriptor.domain) {
            return Err(BridgeError::DuplicateProvider {
                domain: descriptor.domain.clone(),
            });
        }
        if self.providers.len() >= self.limits.maximum_providers {
            return Err(BridgeError::ProviderCapacity {
                capacity: self.limits.maximum_providers,
            });
        }
        self.providers.insert(descriptor.domain.clone(), provider);
        Ok(())
    }

    pub fn advertised_domains(&self) -> BTreeSet<Capability> {
        self.providers.keys().cloned().collect()
    }

    pub fn permission_descriptor(&self, domain: &Capability) -> Option<ProviderDescriptor> {
        self.providers
            .get(domain)
            .map(|provider| provider.descriptor().clone())
    }

    pub fn permission_descriptors(&self) -> Vec<ProviderDescriptor> {
        self.providers
            .values()
            .map(|provider| provider.descriptor().clone())
            .collect()
    }

    pub fn negotiate(
        &self,
        principal: &Principal,
        profile: ExecutionProfile,
        required: &BTreeSet<Capability>,
    ) -> Result<InjectionPlan, BridgeError> {
        let domains = self
            .providers
            .keys()
            .filter(|domain| profile_allows(profile, domain))
            .filter(|domain| {
                is_foundational_shell(domain)
                    || self
                        .grants
                        .decision(principal, domain)
                        .allows_without_prompt()
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        let missing = required.difference(&domains).cloned().collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(BridgeError::MissingRequiredDomains { missing });
        }
        Ok(InjectionPlan {
            principal: principal.clone(),
            profile,
            domains,
        })
    }

    pub fn open_session(
        &self,
        context: &SessionContext,
        now_millis: u64,
    ) -> Result<(), BridgeError> {
        self.insert_session(
            context,
            SourceWindowId(context.id.0),
            BTreeSet::new(),
            now_millis,
        )
        .map(|_| ())
    }

    /// Production session boundary. The source-window token and immutable
    /// injection plan are trusted runtime values, never envelope fields.
    pub fn open_session_bound(
        &self,
        context: &SessionContext,
        plan: &InjectionPlan,
        source_window: SourceWindowId,
        now_millis: u64,
    ) -> Result<ProviderPushObserver, BridgeError> {
        if context.principal != plan.principal {
            return Err(BridgeError::PlanPrincipalMismatch);
        }
        if context.profile != plan.profile {
            return Err(BridgeError::SessionIdentityMismatch {
                session: context.id,
            });
        }
        let outbound =
            self.insert_session(context, source_window, plan.domains.clone(), now_millis)?;
        let lifecycle_context = ProviderSessionContext {
            principal: context.principal.clone(),
            session: context.id,
            source_window,
            profile: context.profile,
        };
        let mut opened: Vec<Arc<dyn Provider>> = Vec::new();
        for domain in plan.domains() {
            let Some(provider) = self.providers.get(domain) else {
                continue;
            };
            let session = ProviderSession {
                context: lifecycle_context.clone(),
                outbound: outbound.sender(domain.clone()),
            };
            if let Err(source) = provider.session_opened(session) {
                outbound.close();
                self.state.lock().sessions.remove(&context.id);
                self.resources.cancel_session(context.id);
                for opened_provider in opened.into_iter().rev() {
                    opened_provider
                        .session_closed(&lifecycle_context, ProviderSessionEnd::OpenFailed);
                }
                return Err(BridgeError::Provider(source));
            }
            opened.push(Arc::clone(provider));
        }
        Ok(outbound.observe())
    }

    fn insert_session(
        &self,
        context: &SessionContext,
        source_window: SourceWindowId,
        domains: BTreeSet<Capability>,
        now_millis: u64,
    ) -> Result<Arc<OutboundMailbox>, BridgeError> {
        let mut state = self.state.lock();
        if let Some(existing) = state.sessions.get(&context.id) {
            return if existing.principal == context.principal
                && existing.profile == context.profile
                && existing.source_window == source_window
                && existing.domains == domains
            {
                Ok(Arc::clone(&existing.outbound))
            } else {
                Err(BridgeError::SessionIdentityMismatch {
                    session: context.id,
                })
            };
        }
        if state.sessions.len() >= self.limits.maximum_sessions {
            return Err(BridgeError::SessionCapacity {
                capacity: self.limits.maximum_sessions,
            });
        }
        let outbound = OutboundMailbox::new(
            context.principal.clone(),
            context.id,
            source_window,
            self.limits.provider_pushes,
        );
        state.sessions.insert(
            context.id,
            SessionSlot {
                principal: context.principal.clone(),
                profile: context.profile,
                bucket: TokenBucket {
                    tokens_milli: u64::from(self.limits.message_burst) * 1_000,
                    updated_at_millis: now_millis,
                },
                source_window,
                domains,
                ready: false,
                outbound: Arc::clone(&outbound),
            },
        );
        Ok(outbound)
    }

    pub fn close_session(&self, session: SessionId) {
        self.close_session_with_reason(session, ProviderSessionEnd::Stopped);
    }

    pub fn close_session_with_reason(&self, session: SessionId, reason: ProviderSessionEnd) {
        let Some(slot) = self.state.lock().sessions.remove(&session) else {
            return;
        };
        slot.outbound.close();
        self.resources.cancel_session(session);
        let context = ProviderSessionContext {
            principal: slot.principal,
            session,
            source_window: slot.source_window,
            profile: slot.profile,
        };
        for domain in slot.domains {
            if let Some(provider) = self.providers.get(&domain) {
                provider.session_closed(&context, reason);
            }
        }
    }

    pub fn mark_session_ready(&self, session: SessionId) -> Result<(), BridgeError> {
        let (context, domains) = {
            let mut state = self.state.lock();
            let slot = state
                .sessions
                .get_mut(&session)
                .ok_or(BridgeError::UnknownSession { session })?;
            if slot.ready {
                return Ok(());
            }
            slot.ready = true;
            (
                ProviderSessionContext {
                    principal: slot.principal.clone(),
                    session,
                    source_window: slot.source_window,
                    profile: slot.profile,
                },
                slot.domains.clone(),
            )
        };
        for domain in domains {
            if let Some(provider) = self.providers.get(&domain)
                && let Err(source) = provider.session_ready(&context)
            {
                return Err(BridgeError::Provider(source));
            }
        }
        Ok(())
    }

    pub fn observe_pushes(
        &self,
        session: SessionId,
        source_window: SourceWindowId,
    ) -> Result<ProviderPushObserver, BridgeError> {
        let state = self.state.lock();
        let slot = state
            .sessions
            .get(&session)
            .ok_or(BridgeError::UnknownSession { session })?;
        if slot.source_window != source_window {
            return Err(BridgeError::SourceWindowMismatch {
                session,
                source_window,
            });
        }
        Ok(slot.outbound.observe())
    }

    /// Revokes an exact-build grant and signals all matching active work.
    ///
    /// Existing injection plans become unusable immediately because dispatch
    /// rechecks the live grant ledger before admitting provider work.
    pub fn revoke(&self, principal: &Principal, domain: &Capability) -> usize {
        self.cancel_capability(principal, domain, true)
    }

    /// Cancels every non-durable operation and provider-push lane for one
    /// exact-build capability after an owner-level grant transaction changed
    /// the ledger. This does not overwrite the newly committed decision.
    pub fn cancel_capability_work(&self, principal: &Principal, domain: &Capability) -> usize {
        self.cancel_capability(principal, domain, false)
    }

    fn cancel_capability(
        &self,
        principal: &Principal,
        domain: &Capability,
        deny_grant: bool,
    ) -> usize {
        let state = self.state.lock();
        let sessions = state
            .sessions
            .iter()
            .filter_map(|(session, slot)| (&slot.principal == principal).then_some(*session))
            .collect::<Vec<_>>();
        let lifecycle = state
            .sessions
            .iter()
            .filter(|(_, slot)| &slot.principal == principal && slot.domains.contains(domain))
            .map(|(session, slot)| {
                slot.outbound.revoke(domain);
                ProviderSessionContext {
                    principal: slot.principal.clone(),
                    session: *session,
                    source_window: slot.source_window,
                    profile: slot.profile,
                }
            })
            .collect::<Vec<_>>();
        let cancelled = if deny_grant {
            self.grants.revoke(principal, domain, sessions)
        } else {
            sessions
                .into_iter()
                .map(|session| self.resources.cancel_session_capability(session, domain))
                .sum()
        };
        drop(state);
        if let Some(provider) = self.providers.get(domain) {
            for context in lifecycle {
                provider.session_revoked(&context);
            }
        }
        cancelled
    }

    /// Dispatches a mapped message. Unknown well-formed types are ignored.
    pub fn dispatch(
        &self,
        context: &SessionContext,
        plan: &InjectionPlan,
        bytes: &[u8],
        now_millis: u64,
    ) -> Result<DispatchOutcome, BridgeError> {
        if bytes.len() > self.limits.maximum_envelope_bytes {
            self.state.lock().refusals += 1;
            return Err(BridgeError::EnvelopeTooLarge {
                actual: bytes.len(),
                maximum: self.limits.maximum_envelope_bytes,
            });
        }
        self.take_rate_token(context, now_millis)?;
        let envelope: Envelope =
            serde_json::from_slice(bytes).map_err(|error| BridgeError::MalformedEnvelope {
                reason: error.to_string(),
            })?;
        let Some((domain_text, action_text)) = envelope.message_type.split_once('.') else {
            self.state.lock().ignored_unknown += 1;
            return Ok(DispatchOutcome::IgnoredUnknown);
        };
        let domain = Capability::new(domain_text).map_err(|_| BridgeError::MalformedEnvelope {
            reason: "invalid domain name".to_owned(),
        })?;
        let Some(provider) = self.providers.get(&domain) else {
            self.state.lock().ignored_unknown += 1;
            return Ok(DispatchOutcome::IgnoredUnknown);
        };
        if !provider.descriptor().actions.contains(action_text) {
            self.state.lock().ignored_unknown += 1;
            return Ok(DispatchOutcome::IgnoredUnknown);
        }
        if context.principal != plan.principal {
            self.record_refusal(context, &domain, Arc::from(action_text));
            return Err(BridgeError::PlanPrincipalMismatch);
        }
        if context.profile != plan.profile || !plan.exposes(&domain) {
            self.record_refusal(context, &domain, Arc::from(action_text));
            return Err(BridgeError::CapabilityDenied { domain });
        }
        let lease = match self.admit_authorized_call(context, &domain) {
            Ok(lease) => lease,
            Err(error) => {
                self.record_refusal(context, &domain, Arc::from(action_text));
                return Err(error);
            }
        };
        let request = ProviderRequest {
            principal: context.principal.clone(),
            session: context.id,
            action: Arc::from(action_text),
            correlation_id: envelope.id.map(Arc::from),
            payload: Value::Object(envelope.fields),
            work: lease,
        };
        let action = Arc::clone(&request.action);
        match provider.call(request) {
            Ok(call) => {
                if call.response.as_ref().is_some_and(|response| {
                    response.byte_len() > self.limits.maximum_response_bytes
                }) {
                    self.record_refusal(context, &domain, action);
                    return Err(BridgeError::ResponseTooLarge);
                }
                let outcome = if call.is_active() {
                    ActivityOutcome::Active
                } else {
                    ActivityOutcome::Completed
                };
                self.activity.record(ProviderActivity {
                    principal: context.principal.clone(),
                    session: context.id,
                    domain,
                    action,
                    outcome,
                });
                self.state.lock().dispatched += 1;
                Ok(DispatchOutcome::Handled(call))
            }
            Err(source) => {
                self.record_refusal(context, &domain, action);
                Err(BridgeError::Provider(source))
            }
        }
    }

    pub fn census(&self) -> BridgeCensus {
        let state = self.state.lock();
        BridgeCensus {
            sessions: state.sessions.len(),
            dispatched: state.dispatched,
            ignored_unknown: state.ignored_unknown,
            refusals: state.refusals,
            throttles: state.throttles,
        }
    }

    fn take_rate_token(
        &self,
        context: &SessionContext,
        now_millis: u64,
    ) -> Result<(), BridgeError> {
        let mut state = self.state.lock();
        let slot = state
            .sessions
            .get_mut(&context.id)
            .ok_or(BridgeError::UnknownSession {
                session: context.id,
            })?;
        if slot.principal != context.principal || slot.profile != context.profile {
            return Err(BridgeError::SessionIdentityMismatch {
                session: context.id,
            });
        }
        let bucket = &mut slot.bucket;
        let elapsed = now_millis.saturating_sub(bucket.updated_at_millis);
        let refill = elapsed.saturating_mul(u64::from(self.limits.message_refill_per_second));
        let capacity = u64::from(self.limits.message_burst) * 1_000;
        bucket.tokens_milli = bucket.tokens_milli.saturating_add(refill).min(capacity);
        bucket.updated_at_millis = now_millis;
        if bucket.tokens_milli < 1_000 {
            state.throttles += 1;
            return Err(BridgeError::MessageRateExceeded {
                session: context.id,
            });
        }
        bucket.tokens_milli -= 1_000;
        Ok(())
    }

    /// Serializes admission with bridge-owned revocation so a dispatch cannot
    /// slip new work into the interval between a live grant check and active
    /// work cancellation.
    fn admit_authorized_call(
        &self,
        context: &SessionContext,
        domain: &Capability,
    ) -> Result<WorkLease, BridgeError> {
        let _state = self.state.lock();
        if is_foundational_shell(domain) {
            return self
                .resources
                .admit(
                    context.id,
                    Some(domain.clone()),
                    ResourceClass::ProviderCall,
                )
                .map_err(BridgeError::ResourceRefused);
        }
        match self.grants.decision(&context.principal, domain) {
            decision if decision.allows_without_prompt() => self
                .resources
                .admit(
                    context.id,
                    Some(domain.clone()),
                    ResourceClass::ProviderCall,
                )
                .map_err(BridgeError::ResourceRefused),
            GrantDecision::AskEveryTime => Err(BridgeError::GrantDecisionRequired {
                domain: domain.clone(),
            }),
            GrantDecision::Denied => Err(BridgeError::CapabilityDenied {
                domain: domain.clone(),
            }),
            GrantDecision::AllowSession
            | GrantDecision::AllowExactBuild
            | GrantDecision::Managed => unreachable!("covered by allows_without_prompt"),
        }
    }

    fn record_refusal(&self, context: &SessionContext, domain: &Capability, action: Arc<str>) {
        self.state.lock().refusals += 1;
        self.activity.record(ProviderActivity {
            principal: context.principal.clone(),
            session: context.id,
            domain: domain.clone(),
            action,
            outcome: ActivityOutcome::Refused,
        });
    }
}

fn is_foundational_shell(capability: &Capability) -> bool {
    capability.as_str() == "shell"
}

fn profile_allows(profile: ExecutionProfile, capability: &Capability) -> bool {
    if profile != ExecutionProfile::Renderer {
        return true;
    }
    !matches!(capability.as_str(), "relay" | "outbox" | "query" | "count")
}

#[derive(Debug)]
pub enum DispatchOutcome {
    IgnoredUnknown,
    Handled(ProviderCall),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeCensus {
    pub sessions: usize,
    pub dispatched: u64,
    pub ignored_unknown: u64,
    pub refusals: u64,
    pub throttles: u64,
}

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("bridge limits must be finite and non-zero")]
    InvalidLimits,
    #[error("provider {domain} has an invalid bounded action inventory")]
    InvalidProvider { domain: Capability },
    #[error("provider {domain} is unavailable on this platform: {reason}")]
    ProviderUnavailable {
        domain: Capability,
        reason: Arc<str>,
    },
    #[error("provider {domain} is already registered")]
    DuplicateProvider { domain: Capability },
    #[error("provider capacity {capacity} is full")]
    ProviderCapacity { capacity: usize },
    #[error("required domains are unavailable: {missing:?}")]
    MissingRequiredDomains { missing: Vec<Capability> },
    #[error("session capacity {capacity} is full")]
    SessionCapacity { capacity: usize },
    #[error("mapped session {session:?} is not open")]
    UnknownSession { session: SessionId },
    #[error("mapped session {session:?} does not match its fixed principal and profile")]
    SessionIdentityMismatch { session: SessionId },
    #[error("source window {source_window:?} is not mapped to session {session:?}")]
    SourceWindowMismatch {
        session: SessionId,
        source_window: SourceWindowId,
    },
    #[error("envelope is {actual} bytes; the maximum is {maximum}")]
    EnvelopeTooLarge { actual: usize, maximum: usize },
    #[error("malformed envelope: {reason}")]
    MalformedEnvelope { reason: String },
    #[error("message rate exceeded for session {session:?}")]
    MessageRateExceeded { session: SessionId },
    #[error("capability {domain} was not injected into this fixed session profile")]
    CapabilityDenied { domain: Capability },
    #[error("the injection plan belongs to a different exact-build principal")]
    PlanPrincipalMismatch,
    #[error("capability {domain} requires an explicit bounded user decision")]
    GrantDecisionRequired { domain: Capability },
    #[error("provider response exceeded the bounded response limit")]
    ResponseTooLarge,
    #[error(transparent)]
    ResourceRefused(#[from] ResourceRefusal),
    #[error(transparent)]
    Provider(#[from] ProviderError),
}

#[derive(Debug, Default)]
pub struct MemoryActivitySink {
    maximum: usize,
    facts: Mutex<std::collections::VecDeque<ProviderActivity>>,
}

impl MemoryActivitySink {
    pub fn bounded(maximum: usize) -> Self {
        Self {
            maximum,
            facts: Mutex::new(std::collections::VecDeque::with_capacity(maximum)),
        }
    }

    pub fn facts(&self) -> Vec<ProviderActivity> {
        self.facts.lock().iter().cloned().collect()
    }
}

impl ActivitySink for MemoryActivitySink {
    fn record(&self, fact: ProviderActivity) {
        if self.maximum == 0 {
            return;
        }
        let mut facts = self.facts.lock();
        if facts.len() == self.maximum {
            facts.pop_front();
        }
        facts.push_back(fact);
    }
}

#[cfg(test)]
mod tests;
