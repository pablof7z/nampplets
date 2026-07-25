mod dispatch;
mod revoke;
mod session;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use nmp_native_runtime_core::{
    Capability, ExecutionProfile, GrantLedger, Principal, ResourceTracker, SessionId,
};
use parking_lot::Mutex;

use crate::outbound::OutboundMailbox;
use crate::{
    ActivitySink, BridgeCensus, BridgeError, BridgeLimits, Provider, ProviderDescriptor,
    ProviderPlatformAvailability, SourceWindowId,
};

#[derive(Debug)]
pub struct ProviderRegistry {
    limits: BridgeLimits,
    providers: BTreeMap<Capability, Arc<dyn Provider>>,
    pub(crate) resources: Arc<ResourceTracker>,
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
