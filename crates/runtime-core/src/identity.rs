use std::{fmt, sync::Arc};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{AccountRef, BoundedJson, Cancellation};

/// Frozen public account identity. The generation is an adapter-owned change
/// sequence, not a second account database.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicIdentity {
    pub generation: u64,
    pub account: Option<AccountRef>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublicIdentityQuery {
    Relays,
    Profile,
    Follows,
    List { list_type: Arc<str> },
    Zaps,
    Mutes,
    Blocked,
    Badges,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicIdentityReadLimits {
    pub maximum_items: usize,
    pub maximum_sources: usize,
    pub maximum_frame_bytes: usize,
}

/// Bounded adapter projection for one query frozen to one exact public
/// identity. The value schema belongs to the provider; NMP evidence remains
/// separately visible and is never collapsed to a global completion flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicIdentityRead {
    pub frozen_identity: PublicIdentity,
    pub value: BoundedJson,
    pub scoped_evidence: BoundedJson,
}

pub trait PublicIdentityChangeSink: Send + Sync + fmt::Debug {
    fn changed(&self, identity: PublicIdentity);
    fn close(&self);
}

pub trait PublicIdentityObservation: Send + Sync + fmt::Debug {
    fn close(&self);
}

pub struct PublicIdentitySubscription {
    pub current: PublicIdentity,
    pub observation: Arc<dyn PublicIdentityObservation>,
}

impl fmt::Debug for PublicIdentitySubscription {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PublicIdentitySubscription")
            .field("current", &self.current)
            .finish_non_exhaustive()
    }
}

/// Public-identity port implemented by the sole NMP facade owner.
///
/// This is read-only from napplets. Native account UX may change the NMP
/// engine's active account through the owning adapter, which emits this port's
/// observation. No signer or secret-bearing object crosses this interface.
pub trait PublicIdentityDataPlane: Send + Sync + fmt::Debug {
    fn freeze_public_identity(&self) -> Result<PublicIdentity, PublicIdentityError>;

    fn read_public_identity(
        &self,
        frozen: &PublicIdentity,
        query: PublicIdentityQuery,
        cancellation: &Cancellation,
        limits: PublicIdentityReadLimits,
    ) -> Result<PublicIdentityRead, PublicIdentityError>;

    fn observe_public_identity(
        &self,
        sink: Arc<dyn PublicIdentityChangeSink>,
    ) -> Result<PublicIdentitySubscription, PublicIdentityError>;
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PublicIdentityError {
    #[error("identity service is closed")]
    Closed,
    #[error("identity observer capacity {capacity} is full")]
    ObserverCapacity { capacity: usize },
    #[error("identity query is not supported by the pinned NMP public facade: {query}")]
    QueryUnavailable { query: Arc<str> },
    #[error("identity query was cancelled")]
    Cancelled,
    #[error("identity cancellation wake capacity {capacity} is full")]
    CancellationCapacity { capacity: usize },
    #[error("identity projection exceeded its negotiated bound")]
    LimitExceeded,
    #[error("identity source returned invalid public data")]
    InvalidSourceData,
    #[error("identity service failed: {reason}")]
    Failed { reason: Arc<str> },
}
