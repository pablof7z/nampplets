use std::{collections::BTreeSet, fmt, sync::Arc};

use nmp_native_runtime_core::{Capability, ExecutionProfile, Principal, SessionId, WorkLease};
use serde_json::Value;
use thiserror::Error;

use crate::{ProviderCall, ProviderPushSender, SourceWindowId};

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
