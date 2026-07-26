use nmp_native_runtime_core::{Capability, GrantDecision, Principal};
use rusqlite::{OptionalExtension, params};

use crate::{
    RuntimeStore, StoreError,
    validate::{grant_decision_text, parse_grant_decision, principal_params},
};

impl RuntimeStore {
    pub fn set_grant(
        &self,
        principal: &Principal,
        capability: &Capability,
        decision: GrantDecision,
    ) -> Result<(), StoreError> {
        let connection = self.connection.lock();
        let exists: bool = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM grants WHERE
                author = ?1 AND d_tag = ?2 AND aggregate_hash = ?3 AND capability = ?4
            )",
            params![
                principal.manifest_author(),
                principal.d_tag(),
                principal.aggregate_hash(),
                capability.as_str()
            ],
            |row| row.get(0),
        )?;
        if !exists {
            let count: usize = connection.query_row(
                "SELECT COUNT(*) FROM grants WHERE
                 author = ?1 AND d_tag = ?2 AND aggregate_hash = ?3",
                principal_params(principal),
                |row| row.get(0),
            )?;
            if count >= self.limits.maximum_grants_per_principal {
                return Err(StoreError::GrantCapacity {
                    capacity: self.limits.maximum_grants_per_principal,
                });
            }
        }
        connection.execute(
            "INSERT INTO grants
                (author, d_tag, aggregate_hash, capability, decision)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(author, d_tag, aggregate_hash, capability) DO UPDATE SET
                decision = excluded.decision",
            params![
                principal.manifest_author(),
                principal.d_tag(),
                principal.aggregate_hash(),
                capability.as_str(),
                grant_decision_text(decision),
            ],
        )?;
        Ok(())
    }

    /// Atomically replaces the named persistent grant rows for one exact
    /// principal. Session-only grants deliberately delete a durable row so a
    /// prior exact-build allowance cannot reappear after restart.
    pub fn set_grants_atomic(
        &self,
        principal: &Principal,
        decisions: &[(Capability, GrantDecision)],
    ) -> Result<(), StoreError> {
        if decisions.is_empty() {
            return Err(StoreError::EmptyGrantBatch);
        }
        let unique = decisions
            .iter()
            .map(|(capability, _)| capability)
            .collect::<std::collections::BTreeSet<_>>();
        if unique.len() != decisions.len() {
            return Err(StoreError::DuplicateGrantBatchCapability);
        }
        if decisions.len() > self.limits.maximum_grants_per_principal {
            return Err(StoreError::GrantCapacity {
                capacity: self.limits.maximum_grants_per_principal,
            });
        }

        let mut connection = self.connection.lock();
        let transaction = connection.transaction()?;
        let installed: bool = transaction.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM installations
                WHERE author = ?1 AND d_tag = ?2 AND aggregate_hash = ?3
            )",
            principal_params(principal),
            |row| row.get(0),
        )?;
        if !installed {
            return Err(StoreError::InstallationNotFound);
        }
        let mut statement = transaction.prepare(
            "SELECT capability FROM grants WHERE
             author = ?1 AND d_tag = ?2 AND aggregate_hash = ?3",
        )?;
        let rows =
            statement.query_map(principal_params(principal), |row| row.get::<_, String>(0))?;
        let mut persistent = rows.collect::<Result<std::collections::BTreeSet<_>, _>>()?;
        drop(statement);
        for (capability, decision) in decisions {
            if *decision == GrantDecision::AllowSession {
                persistent.remove(capability.as_str());
            } else {
                persistent.insert(capability.as_str().to_owned());
            }
        }
        if persistent.len() > self.limits.maximum_grants_per_principal {
            return Err(StoreError::GrantCapacity {
                capacity: self.limits.maximum_grants_per_principal,
            });
        }

        for (capability, decision) in decisions {
            if *decision == GrantDecision::AllowSession {
                transaction.execute(
                    "DELETE FROM grants WHERE
                     author = ?1 AND d_tag = ?2 AND aggregate_hash = ?3 AND capability = ?4",
                    params![
                        principal.manifest_author(),
                        principal.d_tag(),
                        principal.aggregate_hash(),
                        capability.as_str(),
                    ],
                )?;
            } else {
                transaction.execute(
                    "INSERT INTO grants
                        (author, d_tag, aggregate_hash, capability, decision)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(author, d_tag, aggregate_hash, capability) DO UPDATE SET
                        decision = excluded.decision",
                    params![
                        principal.manifest_author(),
                        principal.d_tag(),
                        principal.aggregate_hash(),
                        capability.as_str(),
                        grant_decision_text(*decision),
                    ],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn grant(
        &self,
        principal: &Principal,
        capability: &Capability,
    ) -> Result<GrantDecision, StoreError> {
        Ok(self
            .grant_entry(principal, capability)?
            .unwrap_or(GrantDecision::Denied))
    }

    /// Returns `None` only when this exact build has never stored a decision
    /// for the capability. Callers that apply a profile default must preserve
    /// this distinction from an explicit durable denial.
    pub fn grant_entry(
        &self,
        principal: &Principal,
        capability: &Capability,
    ) -> Result<Option<GrantDecision>, StoreError> {
        let connection = self.connection.lock();
        let decision = connection
            .query_row(
                "SELECT decision FROM grants WHERE
                 author = ?1 AND d_tag = ?2 AND aggregate_hash = ?3 AND capability = ?4",
                params![
                    principal.manifest_author(),
                    principal.d_tag(),
                    principal.aggregate_hash(),
                    capability.as_str()
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        decision
            .map(|value| parse_grant_decision(&value))
            .transpose()
    }
}
