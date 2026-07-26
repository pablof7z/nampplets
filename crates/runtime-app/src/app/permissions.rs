//! Grant decisions, permission review projection, and persistent grant
//! restoration.

mod batch;
mod policy;

use std::sync::Arc;

use nmp_native_runtime_core::{Capability, GrantDecision, Principal, Sensitivity};
use nmp_native_runtime_store::StoreError;

use super::{AppState, RuntimeApp};
use crate::{
    commands::PlatformEvent,
    views::{AppErrorCode, PermissionCapabilityView, PermissionReviewError, PermissionReviewView},
};
use policy::{permission_decision_policy, permission_provider_projection};

impl RuntimeApp {
    /// Builds one bounded exact-build permission review from Rust-owned
    /// installation requests, provider metadata, live session grants, and
    /// durable grant rows. Missing provider metadata stays explicitly unknown.
    pub fn permission_review(
        &self,
        principal: &Principal,
    ) -> Result<PermissionReviewView, PermissionReviewError> {
        let build = self
            .state
            .lock()
            .installed
            .get(principal)
            .cloned()
            .ok_or(PermissionReviewError::NotInstalled)?;
        let mut capabilities = Vec::with_capacity(build.capability_requests.len());
        for request in &build.capability_requests {
            let persistent = self
                .store
                .grant_entry(principal, &request.capability)
                .map_err(|error| PermissionReviewError::Store {
                    detail: Arc::from(error.to_string()),
                })?;
            let current_entry = self
                .grants
                .decision_entry(principal, &request.capability)
                .or(persistent);
            let current_decision = current_entry.unwrap_or(GrantDecision::Denied);
            let descriptor = self.bridge.permission_descriptor(&request.capability);
            let (sensitivity, dependencies, platform_availability) =
                permission_provider_projection(descriptor);
            let policy = permission_decision_policy(
                current_decision,
                &platform_availability,
                self.permission_default,
                current_entry.is_none(),
            );
            capabilities.push(PermissionCapabilityView {
                capability: request.capability.clone(),
                requirement: request.requirement,
                sensitivity,
                dependencies,
                platform_availability,
                current_decision,
                is_granted: current_decision.allows_without_prompt(),
                requested_decision: policy.requested,
                recommended_decision: policy.recommended,
                decision_options: policy.options,
            });
        }
        Ok(PermissionReviewView {
            principal: principal.clone(),
            title: build.title,
            capabilities,
        })
    }

    pub(super) fn set_grant(
        &self,
        state: &mut AppState,
        principal: Principal,
        capability: Capability,
        sensitivity: Sensitivity,
        decision: GrantDecision,
        now: u64,
    ) {
        if !state.installed.contains_key(&principal) {
            self.refuse(
                state,
                AppErrorCode::NotInstalled,
                Some(principal),
                None,
                "grant target is not an installed exact build",
                now,
            );
            return;
        }
        if capability.as_str() == "shell" {
            self.refuse(
                state,
                AppErrorCode::Grant,
                Some(principal),
                None,
                "foundational shell is mandatory and is not grant-controlled",
                now,
            );
            return;
        }
        let previous = self.grants.decision(&principal, &capability);
        if let Err(error) =
            self.grants
                .set(principal.clone(), capability.clone(), sensitivity, decision)
        {
            self.refuse(
                state,
                AppErrorCode::Grant,
                Some(principal),
                None,
                error.to_string(),
                now,
            );
            return;
        }
        let persistent = decision != GrantDecision::AllowSession;
        if persistent && let Err(error) = self.store.set_grant(&principal, &capability, decision) {
            let _ = self
                .grants
                .set(principal.clone(), capability.clone(), sensitivity, previous);
            self.refuse_store(state, Some(principal), None, error, now);
            return;
        }
        self.record_activity(
            state,
            &principal,
            "grant",
            capability.as_str(),
            grant_outcome(decision),
            now,
        );
        self.push_event(
            state,
            PlatformEvent::GrantChanged {
                principal,
                capability,
                decision,
            },
        );
    }

    pub(super) fn current_grant_decision(
        &self,
        principal: &Principal,
        capability: &Capability,
    ) -> Result<GrantDecision, StoreError> {
        match self.grants.decision_entry(principal, capability) {
            Some(decision) => Ok(decision),
            None => self.store.grant(principal, capability),
        }
    }

    pub(super) fn revoke(
        &self,
        state: &mut AppState,
        principal: Principal,
        capability: Capability,
        now: u64,
    ) {
        if capability.as_str() == "shell" {
            self.refuse(
                state,
                AppErrorCode::Grant,
                Some(principal),
                None,
                "foundational shell is mandatory and is not grant-controlled",
                now,
            );
            return;
        }
        if let Err(error) = self
            .store
            .set_grant(&principal, &capability, GrantDecision::Denied)
        {
            self.refuse_store(state, Some(principal), None, error, now);
            return;
        }
        self.bridge.revoke(&principal, &capability);
        let operations = state
            .operations
            .iter()
            .filter_map(|(id, operation)| {
                (operation.principal == principal && operation.domain == capability)
                    .then_some((*id, operation.session))
            })
            .collect::<Vec<_>>();
        for (id, _) in operations {
            if let Some(operation) = state.operations.remove(&id) {
                operation.cancel(Arc::from("session ended"));
            }
        }
        self.record_activity(
            state,
            &principal,
            "grant",
            capability.as_str(),
            "revoked",
            now,
        );
        self.push_event(
            state,
            PlatformEvent::GrantChanged {
                principal,
                capability,
                decision: GrantDecision::Denied,
            },
        );
    }

    pub(super) fn restore_persistent_grants(
        &self,
        principal: &Principal,
    ) -> Result<(), StoreError> {
        for capability in self.bridge.advertised_domains() {
            let decision = self.store.grant(principal, &capability)?;
            if decision != GrantDecision::Denied {
                self.grants
                    .set(
                        principal.clone(),
                        capability,
                        Sensitivity::Sensitive,
                        decision,
                    )
                    .map_err(|error| StoreError::Corrupt(error.to_string()))?;
            }
        }
        Ok(())
    }
}

pub(super) fn grant_outcome(decision: GrantDecision) -> &'static str {
    match decision {
        GrantDecision::Denied => "denied",
        GrantDecision::AskEveryTime => "ask-every-time",
        GrantDecision::AllowSession => "allowed-session",
        GrantDecision::AllowExactBuild => "allowed-exact-build",
        GrantDecision::Managed => "managed",
    }
}

#[cfg(test)]
mod tests;
