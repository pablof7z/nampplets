use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use nmp_native_runtime_core::{ExecutionProfile, Principal, SessionId};

use crate::ProviderPushLimits;

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
