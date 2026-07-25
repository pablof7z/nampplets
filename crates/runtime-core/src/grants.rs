use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{Principal, ResourceTracker, SessionId};

const MAX_CAPABILITY_BYTES: usize = 64;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Capability(String);

impl Capability {
    pub fn new(value: impl Into<String>) -> Result<Self, GrantError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_CAPABILITY_BYTES
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'-' | b'_')
            })
        {
            return Err(GrantError::InvalidCapability);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Capability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("Capability").field(&self.0).finish()
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantDecision {
    Denied,
    AskEveryTime,
    AllowSession,
    AllowExactBuild,
    Managed,
}

impl GrantDecision {
    /// The user-selectable affirmative decisions, broadest first.
    ///
    /// `AllowExactBuild` survives the session for one exact aggregate hash;
    /// `AllowSession` dies with the session, so it is strictly narrower.
    /// `Managed` is host policy and is never a decision a user may pick.
    /// Callers that need "the broadest affirmative decision" read this order
    /// instead of inventing one at a presentation boundary.
    pub const AFFIRMATIVE_BY_BREADTH: [Self; 2] = [Self::AllowExactBuild, Self::AllowSession];

    pub fn allows_without_prompt(self) -> bool {
        matches!(
            self,
            Self::AllowSession | Self::AllowExactBuild | Self::Managed
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    Ordinary,
    Sensitive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityRequirement {
    Required,
    Optional,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRequest {
    pub capability: Capability,
    pub requirement: CapabilityRequirement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrantLimits {
    pub principals: usize,
    pub capabilities_per_principal: usize,
}

impl Default for GrantLimits {
    fn default() -> Self {
        Self {
            principals: 256,
            capabilities_per_principal: 64,
        }
    }
}

#[derive(Debug)]
pub struct GrantLedger {
    limits: GrantLimits,
    resources: Arc<ResourceTracker>,
    entries: RwLock<BTreeMap<Principal, BTreeMap<Capability, GrantRecord>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GrantRecord {
    decision: GrantDecision,
    sensitivity: Sensitivity,
}

#[derive(Debug, PartialEq, Eq)]
pub enum GrantBatchError<E> {
    Grant(GrantError),
    Commit(E),
}

impl GrantLedger {
    pub fn new(limits: GrantLimits, resources: Arc<ResourceTracker>) -> Result<Self, GrantError> {
        if limits.principals == 0 || limits.capabilities_per_principal == 0 {
            return Err(GrantError::InvalidLimits);
        }
        Ok(Self {
            limits,
            resources,
            entries: RwLock::new(BTreeMap::new()),
        })
    }

    pub fn set(
        &self,
        principal: Principal,
        capability: Capability,
        sensitivity: Sensitivity,
        decision: GrantDecision,
    ) -> Result<(), GrantError> {
        let mut entries = self.entries.write();
        if !entries.contains_key(&principal) && entries.len() >= self.limits.principals {
            return Err(GrantError::PrincipalCapacity {
                capacity: self.limits.principals,
            });
        }
        let grants = entries.entry(principal).or_default();
        if !grants.contains_key(&capability)
            && grants.len() >= self.limits.capabilities_per_principal
        {
            return Err(GrantError::CapabilityCapacity {
                capacity: self.limits.capabilities_per_principal,
            });
        }
        grants.insert(
            capability,
            GrantRecord {
                decision,
                sensitivity,
            },
        );
        Ok(())
    }

    pub fn decision(&self, principal: &Principal, capability: &Capability) -> GrantDecision {
        self.entries
            .read()
            .get(principal)
            .and_then(|grants| grants.get(capability))
            .map_or(GrantDecision::Denied, |grant| grant.decision)
    }

    pub fn decision_entry(
        &self,
        principal: &Principal,
        capability: &Capability,
    ) -> Option<GrantDecision> {
        self.entries
            .read()
            .get(principal)
            .and_then(|grants| grants.get(capability))
            .map(|grant| grant.decision)
    }

    /// Commits one finite exact-principal batch while the ledger write lock is
    /// held. The persistence callback runs only after all ledger validation
    /// succeeds, and memory is changed only after that callback succeeds.
    ///
    /// Callers must perform irreversible revocation/cancellation after this
    /// method returns `Ok`, never from the callback.
    pub fn commit_batch<E>(
        &self,
        principal: Principal,
        changes: &[(Capability, Sensitivity, GrantDecision)],
        persist: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), GrantBatchError<E>> {
        if changes.is_empty() {
            return Err(GrantBatchError::Grant(GrantError::EmptyBatch));
        }
        let mut unique = BTreeSet::new();
        for (capability, _, _) in changes {
            if !unique.insert(capability.clone()) {
                return Err(GrantBatchError::Grant(
                    GrantError::DuplicateBatchCapability {
                        capability: capability.clone(),
                    },
                ));
            }
        }

        let mut entries = self.entries.write();
        if !entries.contains_key(&principal) && entries.len() >= self.limits.principals {
            return Err(GrantBatchError::Grant(GrantError::PrincipalCapacity {
                capacity: self.limits.principals,
            }));
        }
        let current_count = entries.get(&principal).map_or(0, BTreeMap::len);
        let additional = unique
            .iter()
            .filter(|capability| {
                !entries
                    .get(&principal)
                    .is_some_and(|grants| grants.contains_key(*capability))
            })
            .count();
        if current_count.saturating_add(additional) > self.limits.capabilities_per_principal {
            return Err(GrantBatchError::Grant(GrantError::CapabilityCapacity {
                capacity: self.limits.capabilities_per_principal,
            }));
        }

        persist().map_err(GrantBatchError::Commit)?;
        let grants = entries.entry(principal).or_default();
        for (capability, sensitivity, decision) in changes {
            grants.insert(
                capability.clone(),
                GrantRecord {
                    decision: *decision,
                    sensitivity: *sensitivity,
                },
            );
        }
        Ok(())
    }

    pub fn revoke(
        &self,
        principal: &Principal,
        capability: &Capability,
        sessions: impl IntoIterator<Item = SessionId>,
    ) -> usize {
        if let Some(record) = self
            .entries
            .write()
            .get_mut(principal)
            .and_then(|grants| grants.get_mut(capability))
        {
            record.decision = GrantDecision::Denied;
        }

        sessions
            .into_iter()
            .map(|session| {
                self.resources
                    .cancel_session_capability(session, capability)
            })
            .sum()
    }

    /// Grants are never copied to a new aggregate hash implicitly.
    pub fn decisions_for(&self, principal: &Principal) -> Vec<(Capability, GrantDecision)> {
        self.entries
            .read()
            .get(principal)
            .into_iter()
            .flat_map(|grants| {
                grants
                    .iter()
                    .map(|(capability, record)| (capability.clone(), record.decision))
            })
            .collect()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GrantError {
    #[error("capability names must be finite lowercase domain identifiers")]
    InvalidCapability,
    #[error("grant limits must be finite and non-zero")]
    InvalidLimits,
    #[error("grant decision batch must not be empty")]
    EmptyBatch,
    #[error("grant decision batch repeats capability {capability}")]
    DuplicateBatchCapability { capability: Capability },
    #[error("grant principal capacity {capacity} is full")]
    PrincipalCapacity { capacity: usize },
    #[error("per-principal capability capacity {capacity} is full")]
    CapabilityCapacity { capacity: usize },
}

#[cfg(test)]
mod tests {
    use crate::{ResourceClass, ResourceLimits};

    use super::*;

    fn principal(hash: char) -> Principal {
        Principal::new("a".repeat(64), "app", hash.to_string().repeat(64)).unwrap()
    }

    #[test]
    fn affirmative_breadth_order_is_declared_by_this_crate() {
        assert_eq!(
            GrantDecision::AFFIRMATIVE_BY_BREADTH,
            [GrantDecision::AllowExactBuild, GrantDecision::AllowSession]
        );
        assert!(
            GrantDecision::AFFIRMATIVE_BY_BREADTH
                .iter()
                .all(|decision| decision.allows_without_prompt())
        );
        assert!(!GrantDecision::AFFIRMATIVE_BY_BREADTH.contains(&GrantDecision::Managed));
    }

    #[test]
    fn update_does_not_inherit_sensitive_grant() {
        let resources = Arc::new(ResourceTracker::new(ResourceLimits::default()).unwrap());
        let ledger = GrantLedger::new(GrantLimits::default(), resources).unwrap();
        let upload = Capability::new("upload").unwrap();
        ledger
            .set(
                principal('b'),
                upload.clone(),
                Sensitivity::Sensitive,
                GrantDecision::AllowExactBuild,
            )
            .unwrap();

        assert_eq!(
            ledger.decision(&principal('c'), &upload),
            GrantDecision::Denied
        );
    }

    #[test]
    fn revoke_cancels_matching_work_only() {
        let resources = Arc::new(ResourceTracker::new(ResourceLimits::default()).unwrap());
        let ledger = GrantLedger::new(GrantLimits::default(), Arc::clone(&resources)).unwrap();
        let resource = Capability::new("resource").unwrap();
        let other = Capability::new("theme").unwrap();
        let first = resources
            .admit(
                SessionId(1),
                Some(resource.clone()),
                ResourceClass::ResourceStream,
            )
            .unwrap();
        let second = resources
            .admit(SessionId(2), Some(other), ResourceClass::ProviderCall)
            .unwrap();

        assert_eq!(ledger.revoke(&principal('b'), &resource, [SessionId(1)]), 1);
        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());
    }

    #[test]
    fn batch_commit_is_all_or_nothing_across_persistence_and_memory() {
        let resources = Arc::new(ResourceTracker::new(ResourceLimits::default()).unwrap());
        let ledger = GrantLedger::new(GrantLimits::default(), resources).unwrap();
        let identity = Capability::new("identity").unwrap();
        let outbox = Capability::new("outbox").unwrap();
        let exact = principal('b');
        let changes = [
            (
                identity.clone(),
                Sensitivity::Sensitive,
                GrantDecision::AllowExactBuild,
            ),
            (
                outbox.clone(),
                Sensitivity::Sensitive,
                GrantDecision::AllowExactBuild,
            ),
        ];

        let result = ledger.commit_batch(exact.clone(), &changes, || Err::<(), _>("disk-full"));
        assert_eq!(result, Err(GrantBatchError::Commit("disk-full")));
        assert_eq!(ledger.decision(&exact, &identity), GrantDecision::Denied);
        assert_eq!(ledger.decision(&exact, &outbox), GrantDecision::Denied);

        ledger
            .commit_batch(exact.clone(), &changes, || Ok::<(), &str>(()))
            .unwrap();
        assert_eq!(
            ledger.decision(&exact, &identity),
            GrantDecision::AllowExactBuild
        );
        assert_eq!(
            ledger.decision(&exact, &outbox),
            GrantDecision::AllowExactBuild
        );
    }

    #[test]
    fn batch_rejects_duplicates_before_persistence() {
        let resources = Arc::new(ResourceTracker::new(ResourceLimits::default()).unwrap());
        let ledger = GrantLedger::new(GrantLimits::default(), resources).unwrap();
        let identity = Capability::new("identity").unwrap();
        let changes = [
            (
                identity.clone(),
                Sensitivity::Ordinary,
                GrantDecision::Denied,
            ),
            (
                identity.clone(),
                Sensitivity::Sensitive,
                GrantDecision::AllowExactBuild,
            ),
        ];
        let persisted = std::cell::Cell::new(false);

        let result = ledger.commit_batch(principal('b'), &changes, || {
            persisted.set(true);
            Ok::<(), ()>(())
        });

        assert!(matches!(
            result,
            Err(GrantBatchError::Grant(
                GrantError::DuplicateBatchCapability { .. }
            ))
        ));
        assert!(!persisted.get());
    }
}
