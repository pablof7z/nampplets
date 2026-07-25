//! Atomic all-or-nothing permission batch application.

use std::{collections::BTreeMap, sync::Arc};

use nmp_native_nap_bridge::ProviderPlatformAvailability;
use nmp_native_runtime_core::{GrantBatchError, GrantDecision, Principal, Sensitivity};

use super::grant_outcome;
use crate::{
    app::{AppState, RuntimeApp},
    commands::PlatformEvent,
    views::{AppErrorCode, PermissionDecision},
};

impl RuntimeApp {
    pub(crate) fn apply_permission_batch(
        &self,
        state: &mut AppState,
        principal: Principal,
        decisions: Vec<PermissionDecision>,
        now: u64,
    ) {
        let Some(build) = state.installed.get(&principal) else {
            self.refuse(
                state,
                AppErrorCode::NotInstalled,
                Some(principal),
                None,
                "permission target is not an installed exact build",
                now,
            );
            return;
        };
        let requested = build
            .capability_requests
            .iter()
            .map(|request| (request.capability.clone(), request.requirement))
            .collect::<BTreeMap<_, _>>();
        if decisions.is_empty() || decisions.len() != requested.len() {
            self.refuse(
                state,
                AppErrorCode::Grant,
                Some(principal),
                None,
                "permission batch must contain exactly one decision for every requested capability",
                now,
            );
            return;
        }
        let mut selected = BTreeMap::new();
        for decision in &decisions {
            if decision.capability.as_str() == "shell" {
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
            if !requested.contains_key(&decision.capability) {
                self.refuse(
                    state,
                    AppErrorCode::Grant,
                    Some(principal),
                    None,
                    format!(
                        "permission batch contains unrequested capability {}",
                        decision.capability
                    ),
                    now,
                );
                return;
            }
            if selected
                .insert(decision.capability.clone(), decision.decision)
                .is_some()
            {
                self.refuse(
                    state,
                    AppErrorCode::Grant,
                    Some(principal),
                    None,
                    format!(
                        "permission batch repeats capability {}",
                        decision.capability
                    ),
                    now,
                );
                return;
            }
            if decision.decision == GrantDecision::Managed {
                self.refuse(
                    state,
                    AppErrorCode::Grant,
                    Some(principal),
                    None,
                    "managed decisions may be set only by host policy",
                    now,
                );
                return;
            }
        }
        if selected.keys().ne(requested.keys()) {
            self.refuse(
                state,
                AppErrorCode::Grant,
                Some(principal),
                None,
                "permission batch capability set does not match the installed exact build",
                now,
            );
            return;
        }

        let mut metadata = BTreeMap::new();
        for capability in requested.keys() {
            let descriptor = self.bridge.permission_descriptor(capability);
            if descriptor.is_none()
                && selected
                    .get(capability)
                    .is_some_and(|decision| *decision != GrantDecision::Denied)
            {
                self.refuse(
                    state,
                    AppErrorCode::Grant,
                    Some(principal),
                    None,
                    format!(
                        "capability {capability} has no registered provider metadata; only denial is valid"
                    ),
                    now,
                );
                return;
            }
            if descriptor.as_ref().is_some_and(|descriptor| {
                matches!(
                    descriptor.platform_availability,
                    ProviderPlatformAvailability::Unavailable { .. }
                )
            }) && selected
                .get(capability)
                .is_some_and(|decision| *decision != GrantDecision::Denied)
            {
                self.refuse(
                    state,
                    AppErrorCode::Grant,
                    Some(principal),
                    None,
                    format!(
                        "capability {capability} is unavailable on this platform; only denial is valid"
                    ),
                    now,
                );
                return;
            }
            metadata.insert(capability.clone(), descriptor);
        }

        for (capability, decision) in &selected {
            if !decision.allows_without_prompt() {
                continue;
            }
            let Some(descriptor) = metadata.get(capability).and_then(Option::as_ref) else {
                continue;
            };
            for dependency in &descriptor.dependencies {
                let dependency_decision = match selected.get(dependency).copied() {
                    Some(decision) => decision,
                    None => match self.current_grant_decision(&principal, dependency) {
                        Ok(decision) => decision,
                        Err(error) => {
                            self.refuse_store(state, Some(principal), None, error, now);
                            return;
                        }
                    },
                };
                if !dependency_decision.allows_without_prompt() {
                    self.refuse(
                        state,
                        AppErrorCode::Grant,
                        Some(principal),
                        None,
                        format!("capability {capability} requires allowed dependency {dependency}"),
                        now,
                    );
                    return;
                }
            }
        }

        let mut previous = BTreeMap::new();
        let mut ledger_changes = Vec::with_capacity(decisions.len());
        let mut persistent = Vec::with_capacity(decisions.len());
        for decision in &decisions {
            let current = match self.current_grant_decision(&principal, &decision.capability) {
                Ok(current) => current,
                Err(error) => {
                    self.refuse_store(state, Some(principal), None, error, now);
                    return;
                }
            };
            if current == GrantDecision::Managed {
                self.refuse(
                    state,
                    AppErrorCode::Grant,
                    Some(principal),
                    None,
                    format!(
                        "capability {} is managed by host policy",
                        decision.capability
                    ),
                    now,
                );
                return;
            }
            previous.insert(decision.capability.clone(), current);
            let sensitivity = metadata
                .get(&decision.capability)
                .and_then(Option::as_ref)
                .map_or(Sensitivity::Sensitive, |descriptor| {
                    if descriptor.sensitive {
                        Sensitivity::Sensitive
                    } else {
                        Sensitivity::Ordinary
                    }
                });
            ledger_changes.push((decision.capability.clone(), sensitivity, decision.decision));
            persistent.push((decision.capability.clone(), decision.decision));
        }
        match self
            .grants
            .commit_batch(principal.clone(), &ledger_changes, || {
                self.store.set_grants_atomic(&principal, &persistent)
            }) {
            Ok(()) => {}
            Err(GrantBatchError::Grant(error)) => {
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
            Err(GrantBatchError::Commit(error)) => {
                self.refuse_store(state, Some(principal), None, error, now);
                return;
            }
        }

        for decision in &decisions {
            let prior = previous
                .get(&decision.capability)
                .copied()
                .unwrap_or(GrantDecision::Denied);
            if prior.allows_without_prompt() && !decision.decision.allows_without_prompt() {
                self.bridge
                    .cancel_capability_work(&principal, &decision.capability);
                let operations = state
                    .operations
                    .iter()
                    .filter_map(|(id, operation)| {
                        (operation.principal == principal
                            && operation.domain == decision.capability)
                            .then_some(*id)
                    })
                    .collect::<Vec<_>>();
                for id in operations {
                    if let Some(operation) = state.operations.remove(&id) {
                        operation.cancel(Arc::from("permission revoked"));
                    }
                }
            }
            self.record_activity(
                state,
                &principal,
                "grant",
                decision.capability.as_str(),
                grant_outcome(decision.decision),
                now,
            );
        }
        self.push_event(
            state,
            PlatformEvent::PermissionBatchApplied {
                principal,
                decisions,
            },
        );
    }
}
