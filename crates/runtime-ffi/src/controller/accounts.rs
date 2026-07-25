//! Local account registration, activation, and removal.

use nmp_native_nmp_adapter::LocalAccountHandle;

use super::RuntimeController;
use crate::{
    RuntimeAccountFailure, RuntimeAccountHandle, RuntimeAccountUpdate,
    projection::{
        local_account_handle, project_account_error, project_account_handle,
        project_account_snapshot,
    },
    support::bump_signal,
};

#[uniffi::export]
impl RuntimeController {
    /// Registers a local signing account through NMP without retaining or
    /// reflecting the supplied secret. Registration does not silently switch
    /// the active account; native UI must explicitly activate the returned
    /// exact installation handle.
    pub fn register_local_account(&self, secret_key: String) -> RuntimeAccountUpdate {
        if secret_key.is_empty()
            || secret_key.len() > 1_024
            || secret_key.chars().any(char::is_control)
        {
            return RuntimeAccountUpdate {
                accepted: false,
                handle: None,
                snapshot: None,
                failure: Some(RuntimeAccountFailure::InvalidSecretKey),
            };
        }
        match self.data_plane.register_local_account(&secret_key) {
            Ok(handle) => self.account_update(Some(handle)),
            Err(error) => RuntimeAccountUpdate {
                accepted: false,
                handle: None,
                snapshot: None,
                failure: Some(project_account_error(error)),
            },
        }
    }

    /// Registers a keyless read-only identity from canonical hexadecimal or
    /// `npub` input. Registration remains separate from activation.
    ///
    /// NIP-05-shaped input receives a typed refusal because the pinned NMP
    /// public facade has no governed resolver; this boundary never performs
    /// application-owned HTTP, DNS, or NIP-05 verification.
    pub fn register_read_only_account(&self, public_identity: String) -> RuntimeAccountUpdate {
        if public_identity.is_empty()
            || public_identity.len() > 1_024
            || public_identity.chars().any(char::is_control)
        {
            return RuntimeAccountUpdate {
                accepted: false,
                handle: None,
                snapshot: None,
                failure: Some(RuntimeAccountFailure::InvalidPublicKey),
            };
        }
        match self.data_plane.register_read_only_account(&public_identity) {
            Ok(handle) => self.account_update(Some(handle)),
            Err(error) => RuntimeAccountUpdate {
                accepted: false,
                handle: None,
                snapshot: None,
                failure: Some(project_account_error(error)),
            },
        }
    }

    /// Selects one exact, currently-owned local account installation.
    pub fn activate_local_account(&self, handle: RuntimeAccountHandle) -> RuntimeAccountUpdate {
        let handle = local_account_handle(handle);
        match self.data_plane.activate_local_account(&handle) {
            Ok(_) => self.account_update(Some(handle)),
            Err(error) => RuntimeAccountUpdate {
                accepted: false,
                handle: None,
                snapshot: None,
                failure: Some(project_account_error(error)),
            },
        }
    }

    /// Signs out without removing any registered signer. Already accepted
    /// writes remain frozen to the account selected at acceptance.
    pub fn logout_local_account(&self) -> RuntimeAccountUpdate {
        match self.data_plane.logout_local_account() {
            Ok(_) => self.account_update(None),
            Err(error) => RuntimeAccountUpdate {
                accepted: false,
                handle: None,
                snapshot: None,
                failure: Some(project_account_error(error)),
            },
        }
    }

    /// Removes only the exact local account installation named by the opaque
    /// public handle. Forged, replaced, or stale handles are refused.
    pub fn remove_local_account(&self, handle: RuntimeAccountHandle) -> RuntimeAccountUpdate {
        let handle = local_account_handle(handle);
        match self.data_plane.remove_local_account(&handle) {
            Ok(_) => self.account_update(None),
            Err(error) => RuntimeAccountUpdate {
                accepted: false,
                handle: None,
                snapshot: None,
                failure: Some(project_account_error(error)),
            },
        }
    }

    pub fn account_snapshot(&self) -> RuntimeAccountUpdate {
        match self.data_plane.local_account_snapshot() {
            Ok(snapshot) => RuntimeAccountUpdate {
                accepted: true,
                handle: None,
                snapshot: Some(project_account_snapshot(snapshot)),
                failure: None,
            },
            Err(error) => RuntimeAccountUpdate {
                accepted: false,
                handle: None,
                snapshot: None,
                failure: Some(project_account_error(error)),
            },
        }
    }
}

impl RuntimeController {
    pub(super) fn account_update(
        &self,
        handle: Option<LocalAccountHandle>,
    ) -> RuntimeAccountUpdate {
        let projected_handle = handle.map(project_account_handle);
        bump_signal(&self.signal);
        match self.data_plane.local_account_snapshot() {
            Ok(snapshot) => RuntimeAccountUpdate {
                accepted: true,
                handle: projected_handle,
                snapshot: Some(project_account_snapshot(snapshot)),
                failure: None,
            },
            Err(error) => RuntimeAccountUpdate {
                accepted: true,
                handle: projected_handle,
                snapshot: None,
                failure: Some(project_account_error(error)),
            },
        }
    }
}
