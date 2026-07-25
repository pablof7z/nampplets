//! Native capability callbacks and the Rust-owned adapters that bound them.

use std::{fmt, io::Cursor, sync::Arc};

use nmp_native_artifact::{
    BlobFetchRequest, BlobFetchResponse, BlobSourceError, ManifestBlobSource,
};
use nmp_native_provider_inc::{
    IncNativeAction, IncNativeActionKind, IncNativeActionOrigin, IncNativeActionSessionEnd,
    IncNativeActionSink, IncNativeActionSinkError,
};
use nmp_native_providers::{
    SettingsExecutor as ProviderSettingsExecutor, SettingsExecutorError,
    SettingsRequest as ProviderSettingsRequest, ThemeSnapshot, ThemeSource,
};
use parking_lot::Mutex;

#[derive(Clone, Debug, uniffi::Record)]
pub struct ArtifactFetchRequest {
    pub logical_path: String,
    pub expected_sha256: String,
    pub candidate_urls: Vec<String>,
    pub maximum_bytes: u64,
    pub redirects_allowed: bool,
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum ArtifactFetchResponse {
    Body {
        source_url: String,
        http_status: u16,
        bytes: Vec<u8>,
    },
    Redirect {
        source_url: String,
        http_status: u16,
        location: String,
    },
    Refused {
        reason: String,
    },
}

#[uniffi::export(callback_interface)]
pub trait ArtifactSource: Send + Sync {
    fn fetch(&self, request: ArtifactFetchRequest) -> ArtifactFetchResponse;
}

/// Raw host appearance facts. Native reports OS state; Rust owns the mapping
/// to the pinned NAP-THEME payload.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NativeAppearanceSnapshot {
    pub dark: bool,
    pub increased_contrast: bool,
    pub reduced_transparency: bool,
    pub accent_red: u8,
    pub accent_green: u8,
    pub accent_blue: u8,
}

#[uniffi::export(callback_interface)]
pub trait NativeAppearanceSource: Send + Sync {
    fn current(&self) -> Option<NativeAppearanceSnapshot>;
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativeSettingsRequest {
    pub manifest_author: String,
    pub d_tag: String,
    pub aggregate_hash: String,
    pub session_id: u64,
    pub section: Option<String>,
    pub schema_json: String,
    pub values_json: String,
}

#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum NativeSettingsOpenResult {
    Accepted,
    Saturated,
    Unavailable,
    Closed,
}

#[uniffi::export(callback_interface)]
pub trait NativeSettingsExecutor: Send + Sync {
    fn try_open(&self, request: NativeSettingsRequest) -> NativeSettingsOpenResult;
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativeIncActionRequest {
    pub manifest_author: String,
    pub d_tag: String,
    pub aggregate_hash: String,
    pub session_id: u64,
    pub source_window_id: u64,
    pub kind: String,
    pub payload_json: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativeIncActionEnd {
    pub manifest_author: String,
    pub d_tag: String,
    pub aggregate_hash: String,
    pub session_id: u64,
    pub source_window_id: u64,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum NativeIncActionEnqueueResult {
    Accepted,
    Backpressure,
    Closed,
}

#[uniffi::export(callback_interface)]
pub trait NativeIncActionExecutor: Send + Sync {
    fn try_enqueue(&self, request: NativeIncActionRequest) -> NativeIncActionEnqueueResult;
    fn session_ended(&self, end: NativeIncActionEnd);
}

pub(crate) struct CallbackArtifactSource {
    pub(crate) callback: Arc<dyn ArtifactSource>,
}

impl fmt::Debug for CallbackArtifactSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CallbackArtifactSource")
            .finish_non_exhaustive()
    }
}

impl ManifestBlobSource for CallbackArtifactSource {
    fn fetch(&self, request: &BlobFetchRequest) -> Result<BlobFetchResponse, BlobSourceError> {
        let maximum_bytes = request.maximum_bytes();
        let response = self.callback.fetch(ArtifactFetchRequest {
            logical_path: request.logical_path().to_owned(),
            expected_sha256: request.digest().as_str().to_owned(),
            candidate_urls: request.candidate_urls().map(str::to_owned).collect(),
            maximum_bytes: maximum_bytes as u64,
            redirects_allowed: false,
        });
        match response {
            ArtifactFetchResponse::Body {
                source_url,
                http_status,
                bytes,
            } => {
                if bytes.len() > maximum_bytes {
                    return Err(BlobSourceError {
                        reason: format!(
                            "artifact source returned {} bytes; the maximum is {maximum_bytes}",
                            bytes.len()
                        ),
                    });
                }
                Ok(BlobFetchResponse::status(
                    source_url,
                    http_status,
                    Box::new(Cursor::new(bytes)),
                ))
            }
            ArtifactFetchResponse::Redirect {
                source_url,
                http_status,
                location,
            } => Ok(BlobFetchResponse::redirect(
                source_url,
                http_status,
                location,
            )),
            ArtifactFetchResponse::Refused { reason } => Err(BlobSourceError { reason }),
        }
    }
}

#[derive(Debug)]
pub(crate) struct RuntimeThemeSource {
    current: Mutex<Option<ThemeSnapshot>>,
}

impl RuntimeThemeSource {
    pub(crate) fn new(current: ThemeSnapshot) -> Self {
        Self {
            current: Mutex::new(Some(current)),
        }
    }

    pub(crate) fn replace(&self, current: ThemeSnapshot) {
        *self.current.lock() = Some(current);
    }
}

impl ThemeSource for RuntimeThemeSource {
    fn current(&self) -> Option<ThemeSnapshot> {
        self.current.lock().clone()
    }
}

pub(crate) struct CallbackSettingsExecutor {
    pub(crate) callback: Arc<dyn NativeSettingsExecutor>,
}

impl fmt::Debug for CallbackSettingsExecutor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CallbackSettingsExecutor")
            .finish_non_exhaustive()
    }
}

impl ProviderSettingsExecutor for CallbackSettingsExecutor {
    fn try_open(&self, request: ProviderSettingsRequest) -> Result<(), SettingsExecutorError> {
        let result = self.callback.try_open(NativeSettingsRequest {
            manifest_author: request.principal.manifest_author().to_owned(),
            d_tag: request.principal.d_tag().to_owned(),
            aggregate_hash: request.principal.aggregate_hash().to_owned(),
            session_id: request.session.0,
            section: request.section.as_deref().map(str::to_owned),
            schema_json: request.schema.as_str().to_owned(),
            values_json: request.values.as_str().to_owned(),
        });
        match result {
            NativeSettingsOpenResult::Accepted => Ok(()),
            NativeSettingsOpenResult::Saturated => Err(SettingsExecutorError::Saturated),
            NativeSettingsOpenResult::Unavailable => Err(SettingsExecutorError::Unavailable),
            NativeSettingsOpenResult::Closed => Err(SettingsExecutorError::Closed),
        }
    }
}

pub(crate) struct CallbackIncNativeActions {
    pub(crate) callback: Arc<dyn NativeIncActionExecutor>,
}

impl fmt::Debug for CallbackIncNativeActions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CallbackIncNativeActions")
            .finish_non_exhaustive()
    }
}

impl IncNativeActionSink for CallbackIncNativeActions {
    fn try_enqueue(&self, action: IncNativeAction) -> Result<(), IncNativeActionSinkError> {
        let request = NativeIncActionRequest {
            manifest_author: action.origin.principal.manifest_author().to_owned(),
            d_tag: action.origin.principal.d_tag().to_owned(),
            aggregate_hash: action.origin.principal.aggregate_hash().to_owned(),
            session_id: action.origin.session.0,
            source_window_id: action.origin.source_window.0,
            kind: match action.kind {
                IncNativeActionKind::NoteOpen => "note-open",
                IncNativeActionKind::ProfileOpen => "profile-open",
                IncNativeActionKind::ComposeOpen => "compose-open",
            }
            .to_owned(),
            payload_json: action.payload.as_str().to_owned(),
        };
        match self.callback.try_enqueue(request) {
            NativeIncActionEnqueueResult::Accepted => Ok(()),
            NativeIncActionEnqueueResult::Backpressure => {
                Err(IncNativeActionSinkError::Backpressure)
            }
            NativeIncActionEnqueueResult::Closed => Err(IncNativeActionSinkError::Closed),
        }
    }

    fn session_ended(&self, origin: &IncNativeActionOrigin, reason: IncNativeActionSessionEnd) {
        self.callback.session_ended(NativeIncActionEnd {
            manifest_author: origin.principal.manifest_author().to_owned(),
            d_tag: origin.principal.d_tag().to_owned(),
            aggregate_hash: origin.principal.aggregate_hash().to_owned(),
            session_id: origin.session.0,
            source_window_id: origin.source_window.0,
            reason: match reason {
                IncNativeActionSessionEnd::Closed(reason) => {
                    format!("closed-{}", format!("{reason:?}").to_ascii_lowercase())
                }
                IncNativeActionSessionEnd::Revoked => "revoked".to_owned(),
            },
        });
    }
}
