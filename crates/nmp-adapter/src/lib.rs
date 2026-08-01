//! NMP public-facade adapter for bounded runtime bindings and durable writes.
//!
//! This crate is the only runtime crate that depends on NMP. It deliberately
//! imports only the supported `nmp` facade; mechanism crates are not
//! dependencies and no canonical Nostr row or write state is persisted here.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    num::NonZeroUsize,
    str::FromStr,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};

use nmp::{
    Binding, Demand, Durability, Engine, EngineConfig, EngineError, FifoReceiver, Filter,
    LiveQuery, ObservationCancel, ReceiptId, ReceiptReattachment as NmpReceiptReattachment,
    ReceiptReplayCursor, ShortfallFact, SourceStatus, Window, WindowLoad, WriteIntent,
    WritePayload, WriteRouting, WriteStatus,
};
use nmp_native_runtime_core::{
    AcceptedWrite, ApprovedWrite, BindingEventSink, BindingRequest, BindingSinkError, BoundedJson,
    HostBindingHandle, HostBindingSnapshot, HostDataError, HostDataPlane, PublicIdentity,
    PublicIdentityChangeSink, PublicIdentityDataPlane, PublicIdentityError,
    PublicIdentityObservation, PublicIdentityQuery, PublicIdentityRead, PublicIdentityReadLimits,
    PublicIdentitySubscription, ReceiptEventSink, ReceiptObservation, ReceiptReattachment,
    ReceiptSinkError, ReceiptSnapshot, WriteReceiptId,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod catalog;
mod debug;
pub mod diagnostics;
mod identity;
mod identity_refresh;
use identity::{
    identity_read_without_account, map_identity_engine_error, project_identity_frame,
    public_identity_query_name, supported_identity_kind, validate_identity_read_limits,
};
#[cfg(test)]
use identity::{project_follows, project_profile, project_relay_list};
mod lists;
mod nap;

pub use nap::{NapNostrProvider, NapNostrProviderLimits, NapNostrProviderSet};

const EVENT_COLLECTION_FAMILY: &str = "event.collection";
const EVENT_COLLECTION_SCHEMA: &str = "nostr.events.collection/1";
const DEFAULT_INITIAL_ROWS: usize = 20;
const MIN_FRAME_BYTES: usize = 1_024;
const MAX_PROFILE_ACCOUNTS: usize = 32;

/// One local trust profile backed by exactly one NMP engine.
pub struct NmpDataPlane {
    engine: Arc<Engine>,
    manifest_catalog: catalog::NmpManifestCatalog,
    relay_diagnostics: diagnostics::NmpRelayDiagnostics,
    workers: Arc<WorkerAdmission>,
    accounts: Mutex<AccountState>,
    identity: Arc<Mutex<IdentityState>>,
    identity_network_refresh: bool,
    closed: AtomicBool,
}

const MAX_IDENTITY_OBSERVERS: usize = 64;

/// The capability attached to one profile-owned account installation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalAccountKind {
    LocalSigner,
    ReadOnly,
}

/// Public, non-secret ownership proof for one exact profile account
/// installation. The adapter never serializes or retains a secret key. Local
/// signer installations privately retain NMP's opaque registration; read-only
/// installations retain only the canonical public key.
///
/// A handle becomes stale when it is removed, or when registering the same
/// public key replaces its NMP signing capability. A stale handle cannot
/// remove the replacement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalAccountHandle {
    pub installation_id: u64,
    pub account: nmp_native_runtime_core::AccountRef,
    pub kind: LocalAccountKind,
}

/// Bounded account-lifecycle projection for native account UI. It contains
/// public keys and opaque installation ids only, never signer objects or
/// private key material.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalAccountSnapshot {
    pub identity: PublicIdentity,
    pub installations: Vec<LocalAccountHandle>,
}

/// Typed account-lifecycle failures. Secret material is deliberately absent
/// from every variant and from all adapter-owned state.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AccountLifecycleError {
    #[error("account service is closed")]
    Closed,
    #[error("local account secret key is invalid")]
    InvalidSecretKey,
    #[error("read-only account public key is invalid")]
    InvalidPublicKey,
    #[error("the pinned NMP facade cannot resolve NIP-05 identifiers")]
    Nip05ResolutionUnavailable,
    #[error("profile account registry is full at {limit} entries")]
    Capacity { limit: usize },
    #[error("NMP account capability instance namespace is exhausted")]
    InstanceExhausted,
    #[error("local account installation is stale or not owned by this profile")]
    StaleInstallation,
    #[error("account lifecycle failed: {reason}")]
    Failed { reason: Arc<str> },
}

#[derive(Debug, Default)]
struct AccountState {
    next_installation_id: u64,
    installations: BTreeMap<u64, InstalledAccount>,
}

#[derive(Debug)]
struct InstalledAccount {
    handle: LocalAccountHandle,
    registration: Option<nmp::AccountRegistration>,
}

#[derive(Debug)]
struct IdentityState {
    generation: u64,
    current: Option<nmp_native_runtime_core::AccountRef>,
    next_observer_id: u64,
    observers: BTreeMap<u64, Arc<dyn PublicIdentityChangeSink>>,
}

impl NmpDataPlane {
    pub fn open(
        config: EngineConfig,
        maximum_bridge_workers: usize,
    ) -> Result<Self, HostDataError> {
        if maximum_bridge_workers == 0 {
            return Err(HostDataError::BindingRefused {
                reason: Arc::from("maximum bridge workers must be non-zero"),
            });
        }
        let identity_network_refresh = !config.indexer_relays.is_empty()
            || !config.app_relays.is_empty()
            || !config.fallback_relays.is_empty();
        let engine = Engine::new(config).map_err(map_open_engine_error)?;
        let mut data_plane = Self::from_engine(Arc::new(engine), maximum_bridge_workers);
        data_plane.identity_network_refresh = identity_network_refresh;
        Ok(data_plane)
    }

    pub fn from_engine(engine: Arc<Engine>, maximum_bridge_workers: usize) -> Self {
        assert!(
            maximum_bridge_workers > 0,
            "maximum bridge workers must be non-zero"
        );
        let current = engine
            .active_account()
            .ok()
            .flatten()
            .map(|pubkey| nmp_native_runtime_core::AccountRef(Arc::from(pubkey.to_string())));
        let manifest_catalog = catalog::NmpManifestCatalog::new(
            Arc::clone(&engine),
            catalog::ManifestCatalogLimits::default(),
        )
        .expect("the built-in manifest catalog limits are valid");
        let relay_diagnostics = diagnostics::NmpRelayDiagnostics::new(
            Arc::clone(&engine),
            diagnostics::RelayDiagnosticsLimits::default(),
        )
        .expect("the built-in relay diagnostics limits are valid");
        Self {
            engine,
            manifest_catalog,
            relay_diagnostics,
            workers: Arc::new(WorkerAdmission {
                active: AtomicUsize::new(0),
                maximum: maximum_bridge_workers,
            }),
            accounts: Mutex::new(AccountState::default()),
            identity: Arc::new(Mutex::new(IdentityState {
                generation: 0,
                current,
                next_observer_id: 0,
                observers: BTreeMap::new(),
            })),
            identity_network_refresh: true,
            closed: AtomicBool::new(false),
        }
    }

    pub fn active_bridge_workers(&self) -> usize {
        self.workers.active.load(Ordering::Acquire)
    }

    /// Returns the one profile-owned catalog facade. Clones share the same
    /// bounded browse and exact-lookup admission domains.
    pub fn manifest_catalog(&self) -> catalog::NmpManifestCatalog {
        self.manifest_catalog.clone()
    }

    /// Returns the one profile-owned diagnostics facade. NMP already tracks
    /// every relay and wire-subscription fact it delivers; clones share the
    /// same bounded observation admission domain.
    pub fn relay_diagnostics(&self) -> diagnostics::NmpRelayDiagnostics {
        self.relay_diagnostics.clone()
    }

    /// Register one local signer through the supported NMP facade. The secret
    /// is consumed only by this call; the adapter retains its opaque NMP
    /// registration and public key, never the caller's secret bytes.
    ///
    /// Registration deliberately does not select the new account. Native UI
    /// must call [`Self::activate_local_account`] with the returned exact
    /// handle, avoiding accidental identity switches during account import.
    pub fn register_local_account(
        &self,
        secret_key: &str,
    ) -> Result<LocalAccountHandle, AccountLifecycleError> {
        self.ensure_account_service_open()?;
        // Serialize all profile-owned account operations. NMP intentionally
        // replaces a same-key registration, so the adapter must not let two
        // callers race into a stale ownership record.
        let mut accounts = self.accounts.lock();
        let installation_id = accounts
            .next_installation_id
            .checked_add(1)
            .ok_or(AccountLifecycleError::InstanceExhausted)?;
        let registration = self
            .engine
            .add_account(secret_key)
            .map_err(map_account_engine_error)?;
        let account =
            nmp_native_runtime_core::AccountRef(Arc::from(registration.public_key().to_string()));
        let replaces_existing = accounts
            .installations
            .values()
            .any(|installed| installed.handle.account == account);
        if !replaces_existing && accounts.installations.len() >= MAX_PROFILE_ACCOUNTS {
            let removed = self
                .engine
                .remove_account(&registration)
                .map_err(map_account_engine_error)?;
            if !removed {
                return Err(AccountLifecycleError::Failed {
                    reason: Arc::from(
                        "NMP refused cleanup after the profile account limit was reached",
                    ),
                });
            }
            return Err(AccountLifecycleError::Capacity {
                limit: MAX_PROFILE_ACCOUNTS,
            });
        }
        // The public facade invalidates an older same-key registration. Drop
        // the stale adapter record without trying to remove it: only the
        // exact new registration owns the replacement capability now.
        accounts
            .installations
            .retain(|_, installed| installed.handle.account != account);
        accounts.next_installation_id = installation_id;
        let handle = LocalAccountHandle {
            installation_id,
            account,
            kind: LocalAccountKind::LocalSigner,
        };
        accounts.installations.insert(
            handle.installation_id,
            InstalledAccount {
                handle: handle.clone(),
                registration: Some(registration),
            },
        );
        Ok(handle)
    }

    /// Register a keyless identity for read-only browsing. NMP's public-key
    /// parser is the sole protocol parser on this path; no native or
    /// application-owned NIP-19 implementation is used.
    ///
    /// The pinned NMP facade has no governed NIP-05 resolver. Inputs that
    /// identify themselves as NIP-05 are refused truthfully instead of
    /// triggering app-owned HTTP, DNS, or identity-resolution logic.
    pub fn register_read_only_account(
        &self,
        public_identity: &str,
    ) -> Result<LocalAccountHandle, AccountLifecycleError> {
        self.ensure_account_service_open()?;
        if public_identity.contains('@') {
            return Err(AccountLifecycleError::Nip05ResolutionUnavailable);
        }
        let canonical_hex = public_identity.len() == 64
            && public_identity
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        let canonical_npub = public_identity.starts_with("npub1")
            && public_identity
                .bytes()
                .all(|byte| !byte.is_ascii_uppercase());
        if !canonical_hex && !canonical_npub {
            return Err(AccountLifecycleError::InvalidPublicKey);
        }
        let public_key = nmp::PublicKey::from_str(public_identity)
            .map_err(|_| AccountLifecycleError::InvalidPublicKey)?;
        let account = nmp_native_runtime_core::AccountRef(Arc::from(public_key.to_string()));

        let mut accounts = self.accounts.lock();
        let existing_id = accounts
            .installations
            .iter()
            .find_map(|(id, installed)| (installed.handle.account == account).then_some(*id));
        if existing_id.is_none() && accounts.installations.len() >= MAX_PROFILE_ACCOUNTS {
            return Err(AccountLifecycleError::Capacity {
                limit: MAX_PROFILE_ACCOUNTS,
            });
        }
        if let Some(existing_id) = existing_id {
            let existing = accounts
                .installations
                .get(&existing_id)
                .expect("the id was read from this map");
            if let Some(registration) = &existing.registration {
                let removed = self
                    .engine
                    .remove_account(registration)
                    .map_err(map_account_engine_error)?;
                if !removed {
                    return Err(AccountLifecycleError::StaleInstallation);
                }
            }
            accounts.installations.remove(&existing_id);
        }

        let installation_id = accounts
            .next_installation_id
            .checked_add(1)
            .ok_or(AccountLifecycleError::InstanceExhausted)?;
        accounts.next_installation_id = installation_id;
        let handle = LocalAccountHandle {
            installation_id,
            account,
            kind: LocalAccountKind::ReadOnly,
        };
        accounts.installations.insert(
            installation_id,
            InstalledAccount {
                handle: handle.clone(),
                registration: None,
            },
        );
        Ok(handle)
    }

    /// Select one currently-owned local account as NMP's active identity and
    /// publish an identity change only after the facade confirms it.
    pub fn activate_local_account(
        &self,
        handle: &LocalAccountHandle,
    ) -> Result<PublicIdentity, AccountLifecycleError> {
        self.ensure_account_service_open()?;
        let accounts = self.accounts.lock();
        let account = installed_account(&accounts, handle)?.handle.account.clone();
        let public_key = parse_account_public_key(&account)?;
        self.engine
            .set_active_account(Some(public_key))
            .map_err(map_account_engine_error)?;
        drop(accounts);
        Ok(self.update_identity(Some(account)))
    }

    /// Select NMP's read-only/signed-out identity. This does not remove any
    /// registered local signer and therefore does not retarget accepted
    /// writes, whose author was frozen at their approval/acceptance boundary.
    pub fn logout_local_account(&self) -> Result<PublicIdentity, AccountLifecycleError> {
        self.ensure_account_service_open()?;
        let _accounts = self.accounts.lock();
        self.engine
            .set_active_account(None)
            .map_err(map_account_engine_error)?;
        drop(_accounts);
        Ok(self.update_identity(None))
    }

    /// Remove exactly one adapter-owned local account installation. Removal
    /// first proves ownership through NMP's opaque registration; a stale
    /// handle cannot detach a replacement for the same public key. If this
    /// account was active, logout follows successful removal and pushes the
    /// signed-out identity exactly once. Accepted writes keep their frozen
    /// author inside NMP regardless of this later lifecycle change.
    pub fn remove_local_account(
        &self,
        handle: &LocalAccountHandle,
    ) -> Result<PublicIdentity, AccountLifecycleError> {
        self.ensure_account_service_open()?;
        let mut accounts = self.accounts.lock();
        let installed = installed_account(&accounts, handle)?;
        let account = installed.handle.account.clone();
        if let Some(registration) = &installed.registration {
            let removed = self
                .engine
                .remove_account(registration)
                .map_err(map_account_engine_error)?;
            if !removed {
                accounts.installations.remove(&handle.installation_id);
                return Err(AccountLifecycleError::StaleInstallation);
            }
        }
        accounts.installations.remove(&handle.installation_id);
        let active = self
            .engine
            .active_account()
            .map_err(map_account_engine_error)?
            .is_some_and(|current| current.to_string() == account.0.as_ref());
        if active {
            self.engine
                .set_active_account(None)
                .map_err(map_account_engine_error)?;
        }
        drop(accounts);
        if active {
            Ok(self.update_identity(None))
        } else {
            self.refresh_identity().map_err(map_public_identity_error)
        }
    }

    /// Return the finite set of locally registered account handles and the
    /// current public identity. NMP's capability registry bounds this vector;
    /// it is not a durable account database.
    pub fn local_account_snapshot(&self) -> Result<LocalAccountSnapshot, AccountLifecycleError> {
        self.ensure_account_service_open()?;
        let installations = self
            .accounts
            .lock()
            .installations
            .values()
            .map(|installed| installed.handle.clone())
            .collect();
        let identity = self.refresh_identity().map_err(map_public_identity_error)?;
        Ok(LocalAccountSnapshot {
            identity,
            installations,
        })
    }

    /// Close the profile. Engine shutdown is idempotent and wakes all query
    /// and receipt drains without a polling loop.
    pub fn close(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            let observers = std::mem::take(&mut self.identity.lock().observers);
            for (_, observer) in observers {
                observer.close();
            }
            self.engine.shutdown();
        }
    }

    /// Native account-selection boundary. The adapter remains the exclusive
    /// mutator of its privately owned NMP engine and emits one change after
    /// the public facade confirms the new active account.
    pub fn set_active_public_identity(
        &self,
        account: Option<nmp_native_runtime_core::AccountRef>,
    ) -> Result<PublicIdentity, PublicIdentityError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(PublicIdentityError::Closed);
        }
        let parsed = account
            .as_ref()
            .map(|account| nmp::PublicKey::from_str(&account.0))
            .transpose()
            .map_err(|_| PublicIdentityError::InvalidSourceData)?;
        let canonical = parsed
            .as_ref()
            .map(|pubkey| nmp_native_runtime_core::AccountRef(Arc::from(pubkey.to_string())));
        let _accounts = self.accounts.lock();
        self.engine
            .set_active_account(parsed)
            .map_err(map_identity_engine_error)?;
        drop(_accounts);
        Ok(self.update_identity(canonical))
    }

    fn update_identity(
        &self,
        current: Option<nmp_native_runtime_core::AccountRef>,
    ) -> PublicIdentity {
        let (identity, observers) = {
            let mut state = self.identity.lock();
            let changed = state.current != current;
            if changed {
                state.generation = state.generation.saturating_add(1);
                state.current = current;
            }
            (
                PublicIdentity {
                    generation: state.generation,
                    account: state.current.clone(),
                },
                if changed {
                    state.observers.values().cloned().collect::<Vec<_>>()
                } else {
                    Vec::new()
                },
            )
        };
        for observer in observers {
            observer.changed(identity.clone());
        }
        identity
    }

    fn refresh_identity(&self) -> Result<PublicIdentity, PublicIdentityError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(PublicIdentityError::Closed);
        }
        let _accounts = self.accounts.lock();
        let current = self
            .engine
            .active_account()
            .map_err(map_identity_engine_error)?
            .map(|pubkey| nmp_native_runtime_core::AccountRef(Arc::from(pubkey.to_string())));
        drop(_accounts);
        Ok(self.update_identity(current))
    }

    fn ensure_open(&self) -> Result<(), HostDataError> {
        if self.closed.load(Ordering::Acquire) {
            Err(HostDataError::ServiceClosed)
        } else {
            Ok(())
        }
    }

    fn ensure_account_service_open(&self) -> Result<(), AccountLifecycleError> {
        if self.closed.load(Ordering::Acquire) {
            Err(AccountLifecycleError::Closed)
        } else {
            Ok(())
        }
    }

    fn spawn_receipt_drain(
        &self,
        component: &'static str,
        statuses: FifoReceiver<WriteStatus>,
        next_cursor: Option<ReceiptReplayCursor>,
        raw_receipt_id: ReceiptId,
        receipt_id: WriteReceiptId,
        sink: Arc<dyn ReceiptEventSink>,
        permit: WorkerPermit,
    ) -> Result<(JoinHandle<()>, Arc<ReceiptDeliveryControl>), HostDataError> {
        let engine = Arc::clone(&self.engine);
        let control = Arc::new(ReceiptDeliveryControl::default());
        let worker_control = Arc::clone(&control);
        let worker = thread::Builder::new()
            .name(component.to_owned())
            .spawn(move || {
                let _permit = permit;
                drain_receipt_pages(
                    engine,
                    raw_receipt_id,
                    statuses,
                    next_cursor,
                    receipt_id,
                    sink,
                    Some(worker_control),
                );
            })
            .map_err(|error| HostDataError::ThreadUnavailable {
                component: Arc::from(component),
                reason: Arc::from(error.to_string()),
            })?;
        Ok((worker, control))
    }

    /// Accept one already-governed NMP intent while preserving the adapter's
    /// pre-acceptance drain guarantee. This remains crate-private so no NMP
    /// type crosses the runtime package boundary.
    fn accept_intent(
        &self,
        frozen_account: nmp_native_runtime_core::AccountRef,
        intent: WriteIntent,
        receipt_sink: Arc<dyn ReceiptEventSink>,
    ) -> Result<AcceptedWrite, HostDataError> {
        self.ensure_open()?;
        let permit = self.workers.reserve("nmp-receipt")?;
        type InitialReceipt = Option<(FifoReceiver<WriteStatus>, WriteReceiptId, ReceiptId)>;
        let (ready_tx, ready_rx): (
            mpsc::SyncSender<InitialReceipt>,
            mpsc::Receiver<InitialReceipt>,
        ) = mpsc::sync_channel(1);
        let receipt_sink_for_worker = Arc::clone(&receipt_sink);
        let engine = Arc::clone(&self.engine);
        let worker = thread::Builder::new()
            .name("nmp-receipt".to_owned())
            .spawn(move || {
                let _permit = permit;
                let Ok(Some((statuses, receipt_id, raw_receipt_id))) = ready_rx.recv() else {
                    return;
                };
                drain_receipt_pages(
                    engine,
                    raw_receipt_id,
                    statuses,
                    None,
                    receipt_id,
                    receipt_sink_for_worker,
                    None,
                );
            })
            .map_err(|error| HostDataError::ThreadUnavailable {
                component: Arc::from("nmp-receipt"),
                reason: Arc::from(error.to_string()),
            })?;

        let stream = match self.engine.publish_tracked(intent) {
            Ok(stream) => stream,
            Err(error) => {
                let _ = ready_tx.send(None);
                let _ = worker.join();
                return Err(map_write_engine_error(error));
            }
        };
        let receipt_id = WriteReceiptId(Arc::from(stream.id.0.to_string()));
        if ready_tx
            .send(Some((stream.statuses, receipt_id.clone(), stream.id)))
            .is_err()
        {
            return Err(HostDataError::ReceiptUnreadable {
                reason: Arc::from("receipt drain terminated after NMP accepted the write"),
            });
        }
        // The profile owns the drain until the receipt terminates or the
        // engine closes. Detaching component UI never cancels the obligation.
        drop(worker);
        Ok(AcceptedWrite {
            receipt_id,
            frozen_account,
        })
    }
}

impl HostDataPlane for NmpDataPlane {
    fn open_binding(
        &self,
        request: BindingRequest,
        sink: Arc<dyn BindingEventSink>,
    ) -> Result<Arc<dyn HostBindingHandle>, HostDataError> {
        self.ensure_open()?;
        let permit = self.workers.reserve("nmp-binding")?;
        let query = collection_query(&request)?;
        let maximum_rows = NonZeroUsize::new(request.maximum_rows as usize).ok_or_else(|| {
            HostDataError::BindingRefused {
                reason: Arc::from("maximum_rows must be non-zero"),
            }
        })?;
        if request.maximum_frame_bytes < MIN_FRAME_BYTES {
            return Err(HostDataError::BindingRefused {
                reason: Arc::from("maximum_frame_bytes is below the 1024-byte minimum"),
            });
        }
        let initial = NonZeroUsize::new(DEFAULT_INITIAL_ROWS.min(maximum_rows.get()))
            .expect("the minimum of two non-zero row counts is non-zero");
        let subscription = self
            .engine
            .observe(
                query,
                Some(Window::Expandable {
                    initial,
                    max: maximum_rows,
                }),
            )
            .map_err(map_binding_engine_error)?;
        let cancel = subscription.cancel_handle();
        let handle = Arc::new(NmpBindingHandle {
            logical_id: Arc::clone(&request.workspace_binding_id),
            cancel,
            worker: Mutex::new(None),
        });
        let thread_handle = Arc::clone(&handle);
        let maximum_frame_bytes = request.maximum_frame_bytes;
        let worker = thread::Builder::new()
            .name("nmp-binding".to_owned())
            .spawn(move || {
                let _permit = permit;
                let mut generation = 0_u64;
                while let Ok(frame) = subscription.recv() {
                    generation = generation.saturating_add(1);
                    let snapshot = match project_frame(generation, frame, maximum_frame_bytes) {
                        Ok(snapshot) => snapshot,
                        Err(reason) => {
                            sink.close(Some(Arc::from(reason)));
                            return;
                        }
                    };
                    match sink.push_latest(snapshot) {
                        Ok(()) => {}
                        Err(BindingSinkError::Closed) => return,
                        Err(BindingSinkError::FrameTooLarge) => {
                            sink.close(Some(Arc::from("binding sink refused an oversized frame")));
                            return;
                        }
                    }
                }
                sink.close(None);
            })
            .map_err(|error| {
                thread_handle.cancel.cancel();
                HostDataError::ThreadUnavailable {
                    component: Arc::from("nmp-binding"),
                    reason: Arc::from(error.to_string()),
                }
            })?;
        *handle.worker.lock() = Some(worker);
        Ok(handle)
    }

    fn accept_write(
        &self,
        approved: ApprovedWrite,
        receipt_sink: Arc<dyn ReceiptEventSink>,
    ) -> Result<AcceptedWrite, HostDataError> {
        let frozen_account = approved.account.clone();
        let intent = approved_write_intent(&approved)?;
        self.accept_intent(frozen_account, intent, receipt_sink)
    }

    fn reattach_receipt(
        &self,
        receipt_id: WriteReceiptId,
        receipt_sink: Arc<dyn ReceiptEventSink>,
    ) -> Result<ReceiptReattachment, HostDataError> {
        self.ensure_open()?;
        let raw_id = receipt_id
            .0
            .parse::<u64>()
            .map_err(|_| HostDataError::ReceiptUnreadable {
                reason: Arc::from("receipt id is not a valid NMP receipt identifier"),
            })?;
        let permit = self.workers.reserve("nmp-receipt-reattach")?;
        match self
            .engine
            .reattach_receipt(ReceiptId(raw_id))
            .map_err(map_receipt_engine_error)?
        {
            NmpReceiptReattachment::Attached {
                id,
                statuses,
                next_cursor,
            } => {
                let (worker, control) = self.spawn_receipt_drain(
                    "nmp-receipt-reattach",
                    statuses,
                    next_cursor,
                    id,
                    receipt_id.clone(),
                    receipt_sink,
                    permit,
                )?;
                Ok(ReceiptReattachment::Attached(Arc::new(
                    ReceiptDrainHandle {
                        receipt_id,
                        control,
                        worker: Mutex::new(Some(worker)),
                    },
                )))
            }
            NmpReceiptReattachment::NotFound => Ok(ReceiptReattachment::NotFound),
            NmpReceiptReattachment::RetainedButUnreadable => {
                Err(HostDataError::ReceiptUnreadable {
                    reason: Arc::from("NMP retained the receipt but its evidence is unreadable"),
                })
            }
        }
    }
}

impl PublicIdentityDataPlane for NmpDataPlane {
    fn freeze_public_identity(&self) -> Result<PublicIdentity, PublicIdentityError> {
        self.refresh_identity()
    }

    fn read_public_identity(
        &self,
        frozen: &PublicIdentity,
        query: PublicIdentityQuery,
        cancellation: &nmp_native_runtime_core::Cancellation,
        limits: PublicIdentityReadLimits,
    ) -> Result<PublicIdentityRead, PublicIdentityError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(PublicIdentityError::Closed);
        }
        if cancellation.is_cancelled() {
            return Err(PublicIdentityError::Cancelled);
        }
        validate_identity_read_limits(limits)?;
        let kind = supported_identity_kind(&query).ok_or_else(|| {
            PublicIdentityError::QueryUnavailable {
                query: Arc::from(public_identity_query_name(&query)),
            }
        })?;
        let Some(account) = &frozen.account else {
            return identity_read_without_account(frozen.clone(), &query, limits);
        };
        let author = nmp::PublicKey::from_str(&account.0)
            .map_err(|_| PublicIdentityError::InvalidSourceData)?;
        let filter = Filter {
            kinds: Some(BTreeSet::from([kind])),
            authors: Some(Binding::Literal(BTreeSet::from([author.to_string()]))),
            ..Filter::default()
        };
        let window_size =
            NonZeroUsize::new(limits.maximum_items).ok_or(PublicIdentityError::LimitExceeded)?;
        let subscription = self
            .engine
            .observe(
                identity_refresh::public_identity_live_query(filter)?,
                Some(Window::Expandable {
                    initial: window_size,
                    max: window_size,
                }),
            )
            .map_err(map_identity_engine_error)?;
        if cancellation.is_cancelled() {
            subscription.cancel();
            return Err(PublicIdentityError::Cancelled);
        }
        let frame = identity_refresh::receive_identity_frame(
            subscription,
            cancellation,
            &self.closed,
            self.identity_network_refresh,
        )?;
        if cancellation.is_cancelled() {
            return Err(PublicIdentityError::Cancelled);
        }
        project_identity_frame(frozen.clone(), &query, frame, limits)
    }

    fn observe_public_identity(
        &self,
        sink: Arc<dyn PublicIdentityChangeSink>,
    ) -> Result<PublicIdentitySubscription, PublicIdentityError> {
        self.refresh_identity()?;
        let (current, id) = {
            let mut state = self.identity.lock();
            if self.closed.load(Ordering::Acquire) {
                return Err(PublicIdentityError::Closed);
            }
            if state.observers.len() >= MAX_IDENTITY_OBSERVERS {
                return Err(PublicIdentityError::ObserverCapacity {
                    capacity: MAX_IDENTITY_OBSERVERS,
                });
            }
            state.next_observer_id = state.next_observer_id.checked_add(1).ok_or_else(|| {
                PublicIdentityError::Failed {
                    reason: Arc::from("identity observer identifier space is exhausted"),
                }
            })?;
            let id = state.next_observer_id;
            state.observers.insert(id, sink);
            (
                PublicIdentity {
                    generation: state.generation,
                    account: state.current.clone(),
                },
                id,
            )
        };
        Ok(PublicIdentitySubscription {
            current,
            observation: Arc::new(NmpIdentityObservation {
                id,
                state: Arc::downgrade(&self.identity),
                closed: AtomicBool::new(false),
            }),
        })
    }
}

struct NmpIdentityObservation {
    id: u64,
    state: Weak<Mutex<IdentityState>>,
    closed: AtomicBool,
}

impl fmt::Debug for NmpIdentityObservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NmpIdentityObservation")
            .field("id", &self.id)
            .field("closed", &self.closed.load(Ordering::Acquire))
            .finish()
    }
}

impl PublicIdentityObservation for NmpIdentityObservation {
    fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(state) = self.state.upgrade() {
            state.lock().observers.remove(&self.id);
        }
    }
}

impl Drop for NmpIdentityObservation {
    fn drop(&mut self) {
        if !*self.closed.get_mut()
            && let Some(state) = self.state.upgrade()
        {
            state.lock().observers.remove(&self.id);
        }
    }
}

impl Drop for NmpDataPlane {
    fn drop(&mut self) {
        self.close();
    }
}

fn installed_account<'a>(
    accounts: &'a AccountState,
    handle: &LocalAccountHandle,
) -> Result<&'a InstalledAccount, AccountLifecycleError> {
    let Some(installed) = accounts.installations.get(&handle.installation_id) else {
        return Err(AccountLifecycleError::StaleInstallation);
    };
    if installed.handle != *handle {
        return Err(AccountLifecycleError::StaleInstallation);
    }
    Ok(installed)
}

fn parse_account_public_key(
    account: &nmp_native_runtime_core::AccountRef,
) -> Result<nmp::PublicKey, AccountLifecycleError> {
    nmp::PublicKey::from_str(&account.0).map_err(|_| AccountLifecycleError::Failed {
        reason: Arc::from("adapter-owned local account has an invalid public key"),
    })
}

fn map_account_engine_error(error: EngineError) -> AccountLifecycleError {
    match error {
        EngineError::EngineClosed => AccountLifecycleError::Closed,
        EngineError::InvalidSecretKey => AccountLifecycleError::InvalidSecretKey,
        EngineError::AuthCapabilityRegistryFull { limit } => {
            AccountLifecycleError::Capacity { limit }
        }
        EngineError::AuthCapabilityInstanceExhausted => AccountLifecycleError::InstanceExhausted,
        other => AccountLifecycleError::Failed {
            reason: Arc::from(other.to_string()),
        },
    }
}

fn map_public_identity_error(error: PublicIdentityError) -> AccountLifecycleError {
    match error {
        PublicIdentityError::Closed => AccountLifecycleError::Closed,
        other => AccountLifecycleError::Failed {
            reason: Arc::from(other.to_string()),
        },
    }
}

struct NmpBindingHandle {
    logical_id: Arc<str>,
    cancel: ObservationCancel,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for NmpBindingHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NmpBindingHandle")
            .field("logical_id", &self.logical_id)
            .field("worker_active", &self.worker.lock().is_some())
            .finish()
    }
}

impl HostBindingHandle for NmpBindingHandle {
    fn logical_id(&self) -> &str {
        &self.logical_id
    }

    fn close(&self) {
        self.cancel.cancel();
        if let Some(worker) = self.worker.lock().take() {
            let _ = worker.join();
        }
    }
}

impl Drop for NmpBindingHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(worker) = self.worker.get_mut().take() {
            let _ = worker.join();
        }
    }
}

/// Receipt delivery is independently stoppable; the durable NMP obligation is
/// not. The pinned direct-Rust FIFO exposes a public close operation that
/// wakes the drain and detaches only this exact observer.
#[derive(Debug)]
struct ReceiptDrainHandle {
    receipt_id: WriteReceiptId,
    control: Arc<ReceiptDeliveryControl>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl ReceiptObservation for ReceiptDrainHandle {
    fn receipt_id(&self) -> &WriteReceiptId {
        &self.receipt_id
    }

    fn stop_delivery(&self) {
        self.control.stop();
        if let Some(worker) = self.worker.lock().take() {
            let _ = worker.join();
        }
    }
}

impl Drop for ReceiptDrainHandle {
    fn drop(&mut self) {
        self.control.stop();
        if let Some(worker) = self.worker.get_mut().take() {
            let _ = worker.join();
        }
    }
}

#[derive(Default)]
struct ReceiptDeliveryControl {
    stopped: AtomicBool,
    current: Mutex<Option<Arc<FifoReceiver<WriteStatus>>>>,
}

impl fmt::Debug for ReceiptDeliveryControl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReceiptDeliveryControl")
            .field("stopped", &self.stopped.load(Ordering::Acquire))
            .field("receiver_attached", &self.current.lock().is_some())
            .finish()
    }
}

impl ReceiptDeliveryControl {
    fn install(&self, receiver: Arc<FifoReceiver<WriteStatus>>) {
        if self.stopped.load(Ordering::Acquire) {
            receiver.close();
            return;
        }
        *self.current.lock() = Some(receiver);
        if self.stopped.load(Ordering::Acquire) {
            if let Some(receiver) = self.current.lock().take() {
                receiver.close();
            }
        }
    }

    fn stop(&self) {
        if !self.stopped.swap(true, Ordering::AcqRel) {
            if let Some(receiver) = self.current.lock().take() {
                receiver.close();
            }
        }
    }
}

#[derive(Debug)]
struct WorkerAdmission {
    active: AtomicUsize,
    maximum: usize,
}

impl WorkerAdmission {
    fn reserve(self: &Arc<Self>, component: &'static str) -> Result<WorkerPermit, HostDataError> {
        let reserved = self
            .active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < self.maximum).then_some(active + 1)
            });
        if reserved.is_err() {
            return Err(HostDataError::ExecutorSaturated {
                component: Arc::from(component),
                capacity: self.maximum,
            });
        }
        Ok(WorkerPermit {
            admission: Arc::clone(self),
        })
    }
}

#[derive(Debug)]
struct WorkerPermit {
    admission: Arc<WorkerAdmission>,
}

impl Drop for WorkerPermit {
    fn drop(&mut self) {
        self.admission.active.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionParameters {
    #[serde(default)]
    kinds: Vec<u16>,
    #[serde(default)]
    authors: Vec<String>,
    since: Option<u64>,
    until: Option<u64>,
}

fn collection_query(request: &BindingRequest) -> Result<LiveQuery, HostDataError> {
    if request.family.as_ref() != EVENT_COLLECTION_FAMILY
        || request.schema.as_ref() != EVENT_COLLECTION_SCHEMA
    {
        return Err(HostDataError::BindingRefused {
            reason: Arc::from(format!(
                "unsupported binding family/schema: {}/{}",
                request.family, request.schema
            )),
        });
    }
    let parameters: CollectionParameters = serde_json::from_str(request.parameters.as_str())
        .map_err(|error| HostDataError::BindingRefused {
            reason: Arc::from(format!("invalid event collection parameters: {error}")),
        })?;
    for author in &parameters.authors {
        if author.len() != 64
            || author
                .bytes()
                .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
        {
            return Err(HostDataError::BindingRefused {
                reason: Arc::from("authors must be lowercase 32-byte hex public keys"),
            });
        }
    }
    let filter = Filter {
        kinds: (!parameters.kinds.is_empty()).then(|| parameters.kinds.into_iter().collect()),
        authors: (!parameters.authors.is_empty())
            .then(|| Binding::Literal(parameters.authors.into_iter().collect())),
        since: parameters.since,
        until: parameters.until,
        ..Filter::default()
    };
    Ok(LiveQuery(Demand::from_filter(filter)))
}

fn project_frame(
    source_generation: u64,
    frame: nmp::Frame,
    maximum_frame_bytes: usize,
) -> Result<HostBindingSnapshot, String> {
    let window = frame
        .window
        .ok_or_else(|| "NMP adapter requires a bounded window frame".to_owned())?;
    let rows = window
        .rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "event": row.event,
                "sources": row.sources.iter().map(ToString::to_string).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let load = match window.load {
        WindowLoad::Idle => serde_json::json!({"state": "idle"}),
        WindowLoad::Requesting => serde_json::json!({"state": "requesting"}),
        WindowLoad::Returned { added } => {
            serde_json::json!({"state": "returned", "added": added})
        }
        WindowLoad::AtBound { max } => {
            serde_json::json!({"state": "at_bound", "max": max})
        }
        _ => serde_json::json!({"state": "unknown_future"}),
    };
    let value_json = serde_json::json!({
        "schema": EVENT_COLLECTION_SCHEMA,
        "rows": rows,
        "windowLoad": load,
    });
    let evidence_sources = frame
        .evidence
        .sources
        .iter()
        .map(|source| {
            serde_json::json!({
                "relay": source.relay.to_string(),
                "access": format!("{:?}", source.access),
                "reconciledThrough": source.reconciled_through.map(|value| value.as_secs()),
                "status": source_status_name(source.status),
            })
        })
        .collect::<Vec<_>>();
    let shortfall = frame
        .evidence
        .shortfall
        .iter()
        .map(shortfall_json)
        .collect::<Vec<_>>();
    let evidence_json = serde_json::json!({
        "sources": evidence_sources,
        "shortfall": shortfall,
    });
    let value_raw = serde_json::to_string(&value_json).map_err(|error| error.to_string())?;
    let evidence_raw = serde_json::to_string(&evidence_json).map_err(|error| error.to_string())?;
    let combined = value_raw.len().saturating_add(evidence_raw.len());
    if combined > maximum_frame_bytes {
        return Err(format!(
            "NMP binding frame is {combined} bytes; negotiated maximum is {maximum_frame_bytes}"
        ));
    }
    let value =
        BoundedJson::from_raw(value_raw, maximum_frame_bytes).map_err(|error| error.to_string())?;
    let scoped_evidence = BoundedJson::from_raw(evidence_raw, maximum_frame_bytes)
        .map_err(|error| error.to_string())?;
    Ok(HostBindingSnapshot {
        source_generation,
        value,
        scoped_evidence,
    })
}

fn source_status_name(status: SourceStatus) -> &'static str {
    match status {
        SourceStatus::Requesting => "requesting",
        SourceStatus::Connecting => "connecting",
        SourceStatus::Disconnected => "disconnected",
        SourceStatus::AwaitingAuth { .. } => "awaiting_auth",
        SourceStatus::AuthDenied => "auth_denied",
        SourceStatus::Error => "error",
    }
}

fn shortfall_json(shortfall: &ShortfallFact) -> serde_json::Value {
    match shortfall {
        ShortfallFact::NoPlannedSource { atom } => serde_json::json!({
            "kind": "no_planned_source",
            "atom": format!("{atom:?}"),
        }),
        ShortfallFact::NoResolvedDemand => {
            serde_json::json!({"kind": "no_resolved_demand"})
        }
        ShortfallFact::LocalLimit { atom } => serde_json::json!({
            "kind": "local_limit",
            "atom": format!("{atom:?}"),
        }),
    }
}

fn approved_write_intent(approved: &ApprovedWrite) -> Result<WriteIntent, HostDataError> {
    let account =
        nmp::PublicKey::from_str(&approved.account.0).map_err(|_| HostDataError::WriteRefused {
            reason: Arc::from("approved account is not a valid Nostr public key"),
        })?;
    let correlation =
        nmp::CorrelationToken::try_from(approved.approval_id.as_ref()).map_err(|error| {
            HostDataError::WriteRefused {
                reason: Arc::from(format!("invalid approval correlation token: {error}")),
            }
        })?;
    let (payload, author) =
        if let Ok(signed) = serde_json::from_str::<nmp::Event>(approved.draft.as_str()) {
            (WritePayload::Signed(signed.clone()), signed.pubkey)
        } else {
            let unsigned: nmp::UnsignedEvent = serde_json::from_str(approved.draft.as_str())
                .map_err(|error| HostDataError::WriteRefused {
                    reason: Arc::from(format!("invalid approved event: {error}")),
                })?;
            (WritePayload::Unsigned(unsigned.clone()), unsigned.pubkey)
        };
    if author != account {
        return Err(HostDataError::WriteRefused {
            reason: Arc::from("approved draft author does not match the frozen account"),
        });
    }
    Ok(WriteIntent {
        payload,
        durability: Durability::Durable,
        routing: WriteRouting::AuthorOutbox,
        identity_override: Some(account),
        correlation: Some(correlation),
    })
}

const MAX_RECEIPT_RELAYS: usize = 64;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReceiptProjection {
    schema: &'static str,
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    frozen_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    conflict: Option<ReceiptConflict>,
    relays: BTreeMap<String, RelayProjection>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReceiptConflict {
    expected: Option<String>,
    actual: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RelayProjection {
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    attempt: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    reason_truncated: bool,
    terminal: bool,
}

impl Default for ReceiptProjection {
    fn default() -> Self {
        Self {
            schema: "nostr.write.receipt/1",
            state: "observing",
            frozen_pubkey: None,
            event_id: None,
            failure: None,
            conflict: None,
            relays: BTreeMap::new(),
        }
    }
}

impl ReceiptProjection {
    fn apply(&mut self, status: &WriteStatus) -> Result<BoundedJson, String> {
        match status {
            WriteStatus::Accepted => self.state = "accepted",
            WriteStatus::Cancelled => self.state = "cancelled",
            WriteStatus::AwaitingCapability { pubkey } => {
                self.state = "awaiting_capability";
                self.frozen_pubkey = Some(pubkey.to_string());
            }
            WriteStatus::Signed(event_id) => {
                self.state = "signed";
                self.event_id = Some(event_id.to_string());
            }
            WriteStatus::Routed(relays) => {
                if relays.len() > MAX_RECEIPT_RELAYS {
                    return Err(format!(
                        "receipt routed to {} relays; the projection maximum is {MAX_RECEIPT_RELAYS}",
                        relays.len()
                    ));
                }
                self.state = "delivering";
                for relay in relays {
                    self.relay(relay.to_string())?;
                }
            }
            WriteStatus::AwaitingRelay { relay } => {
                self.set_relay(relay.to_string(), "awaiting_relay", None, None, None, false)?;
            }
            WriteStatus::AwaitingAuth { relay } => {
                self.set_relay(relay.to_string(), "awaiting_auth", None, None, None, false)?;
            }
            WriteStatus::RetryEligible {
                relay,
                attempt,
                eligible_at,
            } => {
                self.set_relay(
                    relay.to_string(),
                    "retry_eligible",
                    Some(*attempt),
                    Some(eligible_at.as_secs()),
                    None,
                    false,
                )?;
            }
            WriteStatus::HandoffAmbiguous {
                relay,
                attempt,
                observed_at,
            } => {
                self.set_relay(
                    relay.to_string(),
                    "handoff_ambiguous",
                    Some(*attempt),
                    Some(observed_at.as_secs()),
                    None,
                    false,
                )?;
            }
            WriteStatus::Sent {
                relay,
                attempt,
                written_at,
            } => {
                self.set_relay(
                    relay.to_string(),
                    "sent",
                    Some(*attempt),
                    Some(written_at.as_secs()),
                    None,
                    false,
                )?;
            }
            WriteStatus::Acked(relay) => {
                self.set_relay(relay.to_string(), "acked", None, None, None, true)?;
            }
            WriteStatus::Rejected(relay, reason) => {
                let (reason, truncated) = bounded_text(reason, 1_024);
                self.set_relay(
                    relay.to_string(),
                    "rejected",
                    None,
                    None,
                    Some(reason),
                    true,
                )?;
                if let Some(lane) = self.relays.get_mut(&relay.to_string()) {
                    lane.reason_truncated = truncated;
                }
            }
            WriteStatus::GaveUp(relay) => {
                self.set_relay(relay.to_string(), "gave_up", None, None, None, true)?;
            }
            WriteStatus::PersistenceBlocked(relay) => {
                self.set_relay(
                    relay.to_string(),
                    "persistence_blocked",
                    None,
                    None,
                    None,
                    false,
                )?;
            }
            WriteStatus::RoutePersistenceBlocked(relay) => {
                self.set_relay(
                    relay.to_string(),
                    "route_persistence_blocked",
                    None,
                    None,
                    None,
                    false,
                )?;
            }
            WriteStatus::OutcomeUnknown(relay) => {
                self.set_relay(relay.to_string(), "outcome_unknown", None, None, None, true)?;
            }
            WriteStatus::ReplaceableConflict { expected, actual } => {
                self.state = "replaceable_conflict";
                self.conflict = Some(ReceiptConflict {
                    expected: expected.map(|value| value.to_string()),
                    actual: actual.map(|value| value.to_string()),
                });
            }
            WriteStatus::Failed(reason) => {
                self.state = "failed";
                self.failure = Some(bounded_text(reason, 1_024).0);
            }
        }
        self.recompute_delivery_state();
        receipt_projection_json(self)
    }

    fn relay(&mut self, relay: String) -> Result<&mut RelayProjection, String> {
        if !self.relays.contains_key(&relay) && self.relays.len() >= MAX_RECEIPT_RELAYS {
            return Err(format!(
                "receipt exceeds the projection maximum of {MAX_RECEIPT_RELAYS} relays"
            ));
        }
        Ok(self.relays.entry(relay).or_insert(RelayProjection {
            state: "routed",
            attempt: None,
            observed_at: None,
            reason: None,
            reason_truncated: false,
            terminal: false,
        }))
    }

    fn set_relay(
        &mut self,
        relay: String,
        state: &'static str,
        attempt: Option<u64>,
        observed_at: Option<u64>,
        reason: Option<String>,
        terminal: bool,
    ) -> Result<(), String> {
        let lane = self.relay(relay)?;
        *lane = RelayProjection {
            state,
            attempt,
            observed_at,
            reason,
            reason_truncated: false,
            terminal,
        };
        Ok(())
    }

    fn recompute_delivery_state(&mut self) {
        if self.relays.is_empty()
            || matches!(
                self.state,
                "cancelled"
                    | "failed"
                    | "replaceable_conflict"
                    | "awaiting_capability"
                    | "accepted"
                    | "signed"
            )
        {
            return;
        }
        if self.relays.values().all(|relay| relay.terminal) {
            let acknowledgements = self
                .relays
                .values()
                .filter(|relay| relay.state == "acked")
                .count();
            self.state = if acknowledgements == self.relays.len() {
                "delivered"
            } else if acknowledgements > 0 {
                "partial_delivery"
            } else {
                "exhausted"
            };
        } else {
            self.state = "delivering";
        }
    }
}

fn receipt_projection_json(projection: &ReceiptProjection) -> Result<BoundedJson, String> {
    const MAX_RECEIPT_STATUS_BYTES: usize = 16 * 1_024;
    let value = serde_json::to_value(projection).map_err(|error| error.to_string())?;
    BoundedJson::from_value(&value, MAX_RECEIPT_STATUS_BYTES).map_err(|error| error.to_string())
}

fn bounded_text(value: &str, maximum_bytes: usize) -> (String, bool) {
    if value.len() <= maximum_bytes {
        return (value.to_owned(), false);
    }
    let mut boundary = maximum_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    (value[..boundary].to_owned(), true)
}

fn map_open_engine_error(error: EngineError) -> HostDataError {
    map_engine_error_with(error, |reason| HostDataError::BindingRefused { reason })
}

fn map_binding_engine_error(error: EngineError) -> HostDataError {
    map_engine_error_with(error, |reason| HostDataError::BindingRefused { reason })
}

fn map_write_engine_error(error: EngineError) -> HostDataError {
    map_engine_error_with(error, |reason| HostDataError::WriteRefused { reason })
}

fn map_receipt_engine_error(error: EngineError) -> HostDataError {
    map_engine_error_with(error, |reason| HostDataError::ReceiptUnreadable { reason })
}

fn map_engine_error_with(
    error: EngineError,
    contextual: impl FnOnce(Arc<str>) -> HostDataError,
) -> HostDataError {
    match error {
        EngineError::ThreadUnavailable { component, reason } => HostDataError::ThreadUnavailable {
            component: Arc::from(component),
            reason: Arc::from(reason),
        },
        EngineError::EngineClosed => HostDataError::ServiceClosed,
        other => contextual(Arc::from(other.to_string())),
    }
}

fn drain_receipt_pages(
    engine: Arc<Engine>,
    raw_receipt_id: ReceiptId,
    statuses: FifoReceiver<WriteStatus>,
    mut next_cursor: Option<ReceiptReplayCursor>,
    receipt_id: WriteReceiptId,
    sink: Arc<dyn ReceiptEventSink>,
    control: Option<Arc<ReceiptDeliveryControl>>,
) {
    let mut statuses = Arc::new(statuses);
    let mut projection = ReceiptProjection::default();
    if let Some(control) = &control {
        control.install(Arc::clone(&statuses));
    }
    loop {
        while let Ok(status) = statuses.recv() {
            let state = match projection.apply(&status) {
                Ok(state) => state,
                Err(reason) => {
                    sink.close(Some(Arc::from(reason)));
                    return;
                }
            };
            match sink.push_latest(ReceiptSnapshot {
                receipt_id: receipt_id.clone(),
                state,
            }) {
                Ok(()) => {}
                Err(ReceiptSinkError::Closed) => return,
                Err(ReceiptSinkError::FrameTooLarge) => {
                    sink.close(Some(Arc::from(
                        "receipt status exceeded the negotiated frame bound",
                    )));
                    return;
                }
            }
        }

        if control
            .as_ref()
            .is_some_and(|control| control.stopped.load(Ordering::Acquire))
        {
            sink.close(None);
            return;
        }
        let Some(cursor) = next_cursor.take() else {
            sink.close(None);
            return;
        };
        match engine.reattach_receipt_from(raw_receipt_id, cursor) {
            Ok(NmpReceiptReattachment::Attached {
                statuses: next_statuses,
                next_cursor: following_cursor,
                ..
            }) => {
                statuses = Arc::new(next_statuses);
                if let Some(control) = &control {
                    control.install(Arc::clone(&statuses));
                }
                next_cursor = following_cursor;
            }
            Ok(NmpReceiptReattachment::NotFound) => {
                sink.close(Some(Arc::from(
                    "receipt disappeared while continuing bounded replay",
                )));
                return;
            }
            Ok(NmpReceiptReattachment::RetainedButUnreadable) => {
                sink.close(Some(Arc::from(
                    "receipt became unreadable while continuing bounded replay",
                )));
                return;
            }
            Err(error) => {
                sink.close(Some(Arc::from(error.to_string())));
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        sync::atomic::AtomicBool,
        time::{Duration, Instant},
    };

    use parking_lot::Condvar;

    use super::*;

    fn request(parameters: serde_json::Value) -> BindingRequest {
        BindingRequest {
            workspace_binding_id: Arc::from("feed"),
            family: Arc::from(EVENT_COLLECTION_FAMILY),
            schema: Arc::from(EVENT_COLLECTION_SCHEMA),
            parameters: BoundedJson::from_value(&parameters, 1024).unwrap(),
            maximum_rows: 40,
            maximum_frame_bytes: 256 * 1024,
        }
    }

    fn identity_row(kind: u16, content: &str, tags: serde_json::Value) -> nmp::Row {
        let event: nmp::Event = serde_json::from_value(serde_json::json!({
            "id": "b330bfaefd2ddf268ebe4196403e6163533c54f41dabc3518bdc1a896c68f40e",
            "pubkey": "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5",
            "created_at": 1,
            "kind": kind,
            "tags": tags,
            "content": content,
            "sig": "78f9225eec934bbcc65c9ba3ca441ac78472a0edd567aa9df404d8a273b88cda46f2e4b3c9e94bb2e83550dff705ae76423c025319dc9b04f87f772cfa0f6ce3",
        }))
        .unwrap();
        nmp::Row {
            event,
            sources: BTreeSet::new(),
        }
    }

    #[test]
    fn identity_profile_projects_only_the_pinned_public_fields() {
        let row = identity_row(
            0,
            r#"{"name":"Alice","display_name":"Alice A.","about":"hi","picture":"https://example.test/a.png","unknown":"ignored","website":42}"#,
            serde_json::json!([]),
        );
        let profile = project_profile(&[row]).unwrap();
        assert_eq!(profile["name"], "Alice");
        assert_eq!(profile["displayName"], "Alice A.");
        assert_eq!(profile["about"], "hi");
        assert_eq!(profile["picture"], "https://example.test/a.png");
        assert!(profile.get("unknown").is_none());
        assert!(profile.get("website").is_none());
    }

    #[test]
    fn identity_follows_are_validated_deduplicated_and_bounded() {
        let followed = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
        let row = identity_row(
            3,
            "",
            serde_json::json!([
                ["p", followed],
                ["p", followed, "wss://hint.example"],
                ["p", "not-a-pubkey"],
                ["e", "ignored"]
            ]),
        );
        assert_eq!(
            project_follows(std::slice::from_ref(&row), 1).unwrap(),
            serde_json::json!([followed])
        );
        assert!(matches!(
            project_follows(
                &[identity_row(
                    3,
                    "",
                    serde_json::json!([
                        ["p", followed],
                        [
                            "p",
                            "f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9"
                        ]
                    ]),
                )],
                1
            ),
            Err(PublicIdentityError::LimitExceeded)
        ));
    }

    #[test]
    fn identity_nip65_permissions_merge_without_accepting_invalid_relays() {
        let row = identity_row(
            10_002,
            "",
            serde_json::json!([
                ["r", "wss://both.example"],
                ["r", "wss://split.example", "read"],
                ["r", "wss://split.example", "write"],
                ["r", "https://not-a-relay.example"],
                ["r", "wss://unknown.example", "other"]
            ]),
        );
        let relays = project_relay_list(&[row], 2).unwrap();
        assert_eq!(
            relays,
            serde_json::json!({
                "wss://both.example": {"read": true, "write": true},
                "wss://split.example": {"read": true, "write": true},
            })
        );
    }

    #[test]
    fn signed_out_identity_reads_are_empty_with_honest_scoped_shortfall() {
        let frozen = PublicIdentity {
            generation: 4,
            account: None,
        };
        let read = identity_read_without_account(
            frozen.clone(),
            &PublicIdentityQuery::Follows,
            PublicIdentityReadLimits {
                maximum_items: 8,
                maximum_sources: 8,
                maximum_frame_bytes: 4_096,
            },
        )
        .unwrap();
        assert_eq!(read.frozen_identity, frozen);
        assert_eq!(read.value.decode().unwrap(), serde_json::json!([]));
        assert_eq!(
            read.scoped_evidence.decode().unwrap()["shortfall"][0]["kind"],
            "no_active_account"
        );
        assert!(!read.scoped_evidence.as_str().contains("synced"));
        assert!(!read.scoped_evidence.as_str().contains("complete"));
    }

    #[test]
    fn identity_read_stays_frozen_across_an_active_account_retarget() {
        let plane = NmpDataPlane::open(EngineConfig::default(), 2).unwrap();
        let first = nmp_native_runtime_core::AccountRef(Arc::from(
            "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5",
        ));
        let second = nmp_native_runtime_core::AccountRef(Arc::from(
            "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
        ));
        let frozen = plane
            .set_active_public_identity(Some(first.clone()))
            .unwrap();
        plane.set_active_public_identity(Some(second)).unwrap();

        let read = plane
            .read_public_identity(
                &frozen,
                PublicIdentityQuery::Follows,
                &nmp_native_runtime_core::Cancellation::new(),
                PublicIdentityReadLimits {
                    maximum_items: 8,
                    maximum_sources: 8,
                    maximum_frame_bytes: 16 * 1024,
                },
            )
            .unwrap();
        assert_eq!(read.frozen_identity, frozen);
        assert_eq!(read.frozen_identity.account, Some(first));
        assert_eq!(read.value.decode().unwrap(), serde_json::json!([]));
        assert!(!read.scoped_evidence.as_str().contains("synced"));
        assert!(!read.scoped_evidence.as_str().contains("complete"));

        assert!(matches!(
            plane.read_public_identity(
                &read.frozen_identity,
                PublicIdentityQuery::Badges,
                &nmp_native_runtime_core::Cancellation::new(),
                PublicIdentityReadLimits {
                    maximum_items: 8,
                    maximum_sources: 8,
                    maximum_frame_bytes: 16 * 1024,
                },
            ),
            Err(PublicIdentityError::QueryUnavailable { .. })
        ));
        plane.close();
    }

    #[test]
    fn identity_follows_read_uses_the_nmp_facade_canonical_store() {
        let plane = NmpDataPlane::open(EngineConfig::default(), 2).unwrap();
        // A valid public kind-3 fixture. Engine acceptance below independently
        // verifies the id and signature before the adapter can observe it.
        let event: nmp::Event = serde_json::from_value(serde_json::json!({
            "id": "7260ac7aa1fcd3b71002d521a440bee500428482fd182b71c6f092083b8bdead",
            "pubkey": "5d14b37435f05775bad136df0c51ccdcdc6f96482f0fea8404eeaf29ca5a8846",
            "created_at": 1784850107_u64,
            "kind": 3,
            "tags": [
                ["p", "5d14b37435f05775bad136df0c51ccdcdc6f96482f0fea8404eeaf29ca5a8846"],
                ["p", "04c915daefee38317fa734444acee390a8269fe5810b2241e5e6dd343dfbecc9"],
                ["client", "Primal Android"]
            ],
            "content": "",
            "sig": "ab429ae0945f7093078c88a7ae66ed9927ceae0f5eed8451becd79b190d5459ebb44bb266b2d157037b00819707f4a89ed8bd1606cb4d3475d796bf39a33bc04",
        }))
        .unwrap();
        let statuses = plane
            .engine
            .publish(WriteIntent {
                payload: WritePayload::Signed(event.clone()),
                durability: Durability::Durable,
                routing: WriteRouting::AuthorOutbox,
                identity_override: None,
                correlation: None,
            })
            .unwrap();
        loop {
            match statuses.recv_timeout(Duration::from_secs(2)).unwrap() {
                WriteStatus::Signed(id) if id == event.id => break,
                WriteStatus::Failed(reason) => panic!("signed fixture was rejected: {reason}"),
                _ => {}
            }
        }
        let account = nmp_native_runtime_core::AccountRef(Arc::from(event.pubkey.to_string()));
        let frozen = plane.set_active_public_identity(Some(account)).unwrap();
        let read = plane
            .read_public_identity(
                &frozen,
                PublicIdentityQuery::Follows,
                &nmp_native_runtime_core::Cancellation::new(),
                PublicIdentityReadLimits {
                    maximum_items: 8,
                    maximum_sources: 8,
                    maximum_frame_bytes: 16 * 1024,
                },
            )
            .unwrap();
        assert_eq!(
            read.value.decode().unwrap(),
            serde_json::json!([
                "04c915daefee38317fa734444acee390a8269fe5810b2241e5e6dd343dfbecc9",
                "5d14b37435f05775bad136df0c51ccdcdc6f96482f0fea8404eeaf29ca5a8846"
            ])
        );
        plane.close();
    }

    #[derive(Debug, Default)]
    struct IdentityChanges {
        values: Mutex<Vec<PublicIdentity>>,
        closed: AtomicBool,
    }

    impl PublicIdentityChangeSink for IdentityChanges {
        fn changed(&self, identity: PublicIdentity) {
            self.values.lock().push(identity);
        }

        fn close(&self) {
            self.closed.store(true, Ordering::Release);
        }
    }

    #[test]
    fn identity_observation_is_change_only_bounded_and_tears_down() {
        let plane = NmpDataPlane::open(EngineConfig::default(), 2).unwrap();
        let sink = Arc::new(IdentityChanges::default());
        let sink_dyn: Arc<dyn PublicIdentityChangeSink> = sink.clone();
        let subscription = plane.observe_public_identity(sink_dyn).unwrap();
        assert_eq!(subscription.current.account, None);

        let account = nmp_native_runtime_core::AccountRef(Arc::from(
            "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5",
        ));
        plane
            .set_active_public_identity(Some(account.clone()))
            .unwrap();
        plane
            .set_active_public_identity(Some(account.clone()))
            .unwrap();
        plane.set_active_public_identity(None).unwrap();
        assert_eq!(sink.values.lock().len(), 2);
        assert_eq!(sink.values.lock()[0].account, Some(account));
        assert_eq!(sink.values.lock()[1].account, None);

        plane.close();
        assert!(sink.closed.load(Ordering::Acquire));
        subscription.observation.close();
    }

    #[test]
    fn local_account_lifecycle_owns_exact_instances_and_pushes_identity() {
        let plane = NmpDataPlane::open(EngineConfig::default(), 2).unwrap();
        let sink = Arc::new(IdentityChanges::default());
        let subscription = plane
            .observe_public_identity(sink.clone() as Arc<dyn PublicIdentityChangeSink>)
            .unwrap();

        let first = plane
            .register_local_account(&format!("{:064x}", 7_u8))
            .unwrap();
        assert_eq!(
            plane.local_account_snapshot().unwrap().installations,
            vec![first.clone()]
        );
        let active = plane.activate_local_account(&first).unwrap();
        assert_eq!(active.account, Some(first.account.clone()));

        // Same-key registration replaces the NMP capability. The old public
        // handle cannot deactivate or remove the replacement.
        let replacement = plane
            .register_local_account(&format!("{:064x}", 7_u8))
            .unwrap();
        assert_ne!(first.installation_id, replacement.installation_id);
        assert_eq!(first.account, replacement.account);
        assert!(matches!(
            plane.activate_local_account(&first),
            Err(AccountLifecycleError::StaleInstallation)
        ));
        assert!(matches!(
            plane.remove_local_account(&first),
            Err(AccountLifecycleError::StaleInstallation)
        ));
        assert_eq!(
            plane.local_account_snapshot().unwrap().installations,
            vec![replacement.clone()]
        );

        let logged_out = plane.logout_local_account().unwrap();
        assert_eq!(logged_out.account, None);
        assert_eq!(sink.values.lock().len(), 2);
        assert_eq!(sink.values.lock()[0].account, Some(first.account));
        assert_eq!(sink.values.lock()[1].account, None);

        assert_eq!(
            plane.remove_local_account(&replacement).unwrap().account,
            None
        );
        assert!(
            plane
                .local_account_snapshot()
                .unwrap()
                .installations
                .is_empty()
        );
        subscription.observation.close();
        plane.close();
    }

    #[test]
    fn read_only_accounts_accept_npub_and_hex_without_registering_a_signer() {
        let plane = NmpDataPlane::open(EngineConfig::default(), 2).unwrap();
        let npub = "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6";
        let first = plane.register_read_only_account(npub).unwrap();
        assert_eq!(first.kind, LocalAccountKind::ReadOnly);
        assert_eq!(first.account.0.len(), 64);
        assert_eq!(
            plane.activate_local_account(&first).unwrap().account,
            Some(first.account.clone())
        );

        let replacement = plane
            .register_read_only_account(first.account.0.as_ref())
            .unwrap();
        assert_eq!(replacement.kind, LocalAccountKind::ReadOnly);
        assert_eq!(replacement.account, first.account);
        assert_ne!(replacement.installation_id, first.installation_id);
        assert!(matches!(
            plane.activate_local_account(&first),
            Err(AccountLifecycleError::StaleInstallation)
        ));
        assert_eq!(
            plane.local_account_snapshot().unwrap().installations,
            vec![replacement.clone()]
        );
        plane.remove_local_account(&replacement).unwrap();
        assert_eq!(
            plane.freeze_public_identity().unwrap().account,
            None,
            "removing an active keyless identity must log out"
        );
        plane.close();
    }

    #[test]
    fn read_only_account_refusals_are_typed_and_profile_capacity_is_combined() {
        let plane = NmpDataPlane::open(EngineConfig::default(), 2).unwrap();
        assert_eq!(
            plane.register_read_only_account("pablo@example.com"),
            Err(AccountLifecycleError::Nip05ResolutionUnavailable)
        );
        assert_eq!(
            plane.register_read_only_account("NOT-A-PUBLIC-KEY"),
            Err(AccountLifecycleError::InvalidPublicKey)
        );

        for secret in 1_u8..=MAX_PROFILE_ACCOUNTS as u8 {
            plane
                .register_local_account(&format!("{:064x}", secret))
                .unwrap();
        }
        assert_eq!(
            plane.register_read_only_account(
                "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5"
            ),
            Err(AccountLifecycleError::Capacity {
                limit: MAX_PROFILE_ACCOUNTS
            })
        );
        assert_eq!(
            plane.local_account_snapshot().unwrap().installations.len(),
            MAX_PROFILE_ACCOUNTS
        );
        plane.close();
    }

    #[derive(Debug, Default)]
    struct DiscardReceiptSink;

    impl ReceiptEventSink for DiscardReceiptSink {
        fn push_latest(&self, _snapshot: ReceiptSnapshot) -> Result<(), ReceiptSinkError> {
            Ok(())
        }

        fn close(&self, _reason: Option<Arc<str>>) {}
    }

    #[test]
    fn accepted_write_keeps_its_frozen_account_after_account_switch_and_logout() {
        let plane = NmpDataPlane::open(EngineConfig::default(), 2).unwrap();
        let first = plane
            .register_local_account(&format!("{:064x}", 11_u8))
            .unwrap();
        let second = plane
            .register_local_account(&format!("{:064x}", 12_u8))
            .unwrap();
        plane.activate_local_account(&first).unwrap();
        let author = nmp::PublicKey::from_str(&first.account.0).unwrap();
        let draft = nmp::UnsignedEvent::new(
            author,
            nmp::Timestamp::from(1_u64),
            nmp::Kind::TextNote,
            Vec::new(),
            "frozen author".to_owned(),
        );
        // The native approval has already frozen `first`; a later switch must
        // not retarget that durable acceptance into `second`.
        plane.activate_local_account(&second).unwrap();
        let accepted = plane
            .accept_write(
                ApprovedWrite {
                    approval_id: Arc::from("approval-frozen-author"),
                    origin_principal: nmp_native_runtime_core::Principal::new(
                        "a".repeat(64),
                        "composer",
                        "b".repeat(64),
                    )
                    .unwrap(),
                    origin_session: nmp_native_runtime_core::SessionId(1),
                    account: first.account.clone(),
                    draft: BoundedJson::from_value(
                        &serde_json::to_value(&draft).unwrap(),
                        16 * 1024,
                    )
                    .unwrap(),
                },
                Arc::new(DiscardReceiptSink),
            )
            .unwrap();
        assert_eq!(accepted.frozen_account, first.account);
        assert_eq!(plane.logout_local_account().unwrap().account, None);
        assert_eq!(accepted.frozen_account.0.as_ref(), draft.pubkey.to_string());
        plane.close();
    }

    #[test]
    fn local_account_lifecycle_fails_closed_after_profile_close() {
        let plane = NmpDataPlane::open(EngineConfig::default(), 2).unwrap();
        plane.close();
        assert!(matches!(
            plane.register_local_account(&format!("{:064x}", 7_u8)),
            Err(AccountLifecycleError::Closed)
        ));
        assert!(matches!(
            plane.logout_local_account(),
            Err(AccountLifecycleError::Closed)
        ));
        assert!(matches!(
            plane.local_account_snapshot(),
            Err(AccountLifecycleError::Closed)
        ));
    }

    #[test]
    fn event_collection_query_is_window_compatible() {
        let query = collection_query(&request(serde_json::json!({
            "kinds": [1],
            "authors": ["ab".repeat(32)],
        })))
        .unwrap();
        assert_eq!(query.0.selection.kinds, Some(BTreeSet::from([1])));
        assert_eq!(query.0.selection.limit, None);
    }

    #[test]
    fn malformed_author_is_refused_before_observation() {
        let error = collection_query(&request(serde_json::json!({
            "authors": ["not-a-key"],
        })))
        .unwrap_err();
        assert!(matches!(error, HostDataError::BindingRefused { .. }));
    }

    #[test]
    fn worker_admission_has_zero_queue() {
        let admission = Arc::new(WorkerAdmission {
            active: AtomicUsize::new(0),
            maximum: 1,
        });
        let permit = admission.reserve("test").unwrap();
        assert!(matches!(
            admission.reserve("test"),
            Err(HostDataError::ExecutorSaturated { capacity: 1, .. })
        ));
        drop(permit);
        assert!(admission.reserve("test").is_ok());
    }

    #[test]
    fn receipt_delivery_stop_wakes_blocked_fifo_without_cancelling_a_write() {
        let (_producer, receiver) = nmp::fifo_channel::<WriteStatus>();
        let receiver = Arc::new(receiver);
        let control = Arc::new(ReceiptDeliveryControl::default());
        control.install(Arc::clone(&receiver));
        let (done_tx, done_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let closed = receiver.recv().is_err();
            done_tx.send(closed).unwrap();
        });

        control.stop();
        assert!(
            done_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("public FIFO close should wake the exact observer")
        );
        worker.join().unwrap();
    }

    #[test]
    fn receipt_projection_preserves_mixed_per_relay_evidence() {
        let first = nmp::RelayUrl::parse("wss://first.example").unwrap();
        let second = nmp::RelayUrl::parse("wss://second.example").unwrap();
        let mut projection = ReceiptProjection::default();

        projection
            .apply(&WriteStatus::Routed(BTreeSet::from([
                first.clone(),
                second.clone(),
            ])))
            .unwrap();
        projection
            .apply(&WriteStatus::Acked(first.clone()))
            .unwrap();
        let state = projection
            .apply(&WriteStatus::Rejected(second.clone(), "policy".to_owned()))
            .unwrap()
            .decode()
            .unwrap();

        assert_eq!(state["state"], "partial_delivery");
        assert_eq!(state["relays"][first.to_string()]["state"], "acked");
        assert_eq!(state["relays"][second.to_string()]["state"], "rejected");
        assert_eq!(state["relays"][second.to_string()]["reason"], "policy");
    }

    #[derive(Debug, Default)]
    struct LatestBindingSink {
        latest: Mutex<Option<HostBindingSnapshot>>,
        changed: Condvar,
        closed: AtomicBool,
    }

    impl BindingEventSink for LatestBindingSink {
        fn push_latest(&self, snapshot: HostBindingSnapshot) -> Result<(), BindingSinkError> {
            if self.closed.load(Ordering::Acquire) {
                return Err(BindingSinkError::Closed);
            }
            *self.latest.lock() = Some(snapshot);
            self.changed.notify_all();
            Ok(())
        }

        fn close(&self, _reason: Option<Arc<str>>) {
            self.closed.store(true, Ordering::Release);
            self.changed.notify_all();
        }
    }

    impl LatestBindingSink {
        fn wait_for_snapshot(&self, deadline: Instant) -> Option<HostBindingSnapshot> {
            let mut latest = self.latest.lock();
            while latest.is_none() && !self.closed.load(Ordering::Acquire) {
                let now = Instant::now();
                if now >= deadline {
                    return None;
                }
                self.changed
                    .wait_for(&mut latest, deadline.saturating_duration_since(now));
            }
            latest.take()
        }
    }

    #[test]
    fn bounded_nmp_binding_delivers_honest_scoped_evidence_and_tears_down() {
        let plane = NmpDataPlane::open(EngineConfig::default(), 2).unwrap();
        let sink = Arc::new(LatestBindingSink::default());
        let handle = plane
            .open_binding(request(serde_json::json!({"kinds": [1]})), sink.clone())
            .unwrap();

        let snapshot = sink
            .wait_for_snapshot(Instant::now() + Duration::from_secs(2))
            .expect("in-memory NMP observation should emit its initial bounded frame");
        let evidence = snapshot.scoped_evidence.as_str();
        assert!(evidence.contains("shortfall"));
        assert!(!evidence.contains("synced"));
        assert!(!evidence.contains("complete"));

        handle.close();
        assert_eq!(plane.active_bridge_workers(), 0);
        plane.close();
    }
}
