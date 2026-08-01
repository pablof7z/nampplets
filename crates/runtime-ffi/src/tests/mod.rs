//! Boundary tests for the UniFFI runtime projection.

mod accounts;
mod artifact;
mod catalog;
mod envelope;
mod intent;
mod intent_restore;
mod library;
mod native_capabilities;
mod operator_relays;
mod permissions;
mod profile_preferences;
mod receipt_slot;
mod receipts;
mod snapshot_delivery;
mod snapshot_integrity;
mod test_support;
mod workspace;

use std::{
    collections::BTreeMap,
    fs,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use nmp_native_artifact::{
    AggregateVerifier as _, ManifestCoordinate, Nip5aPathTagsAggregate, Sha256Digest, VerifiedFile,
};
use nmp_native_runtime_app::{AppLimits, ExecutableArtifact, PlatformCommand, PlatformEvent};
use nmp_native_runtime_core::{
    BoundedJson, Capability, CapabilityRequest, CapabilityRequirement, Principal, SessionId,
};
use nmp_native_runtime_store::{InstalledBuild, WorkspaceRecord};
use nostr::{EventBuilder, Keys, Kind, Tag};
use parking_lot::Mutex;
use serde_json::Value;
use tempfile::TempDir;

use crate::{
    catalog_coordinate::parse_catalog_coordinate, projection::project_event,
    workspace::workspace_record_from_ffi, *,
};

use test_support::*;

#[derive(Debug)]
struct FixtureAppearance;

impl NativeAppearanceSource for FixtureAppearance {
    fn current(&self) -> Option<NativeAppearanceSnapshot> {
        Some(NativeAppearanceSnapshot {
            dark: true,
            increased_contrast: false,
            reduced_transparency: false,
            accent_red: 88,
            accent_green: 166,
            accent_blue: 255,
        })
    }
}

#[derive(Debug)]
struct RecordingSettings {
    requests: Arc<Mutex<Vec<NativeSettingsRequest>>>,
}

impl NativeSettingsExecutor for RecordingSettings {
    fn try_open(&self, request: NativeSettingsRequest) -> NativeSettingsOpenResult {
        self.requests.lock().push(request);
        NativeSettingsOpenResult::Accepted
    }
}

#[derive(Debug)]
struct RecordingIncActions {
    requests: Arc<Mutex<Vec<NativeIncActionRequest>>>,
    ends: Arc<Mutex<Vec<NativeIncActionEnd>>>,
    result: NativeIncActionEnqueueResult,
}

impl NativeIncActionExecutor for RecordingIncActions {
    fn try_enqueue(&self, request: NativeIncActionRequest) -> NativeIncActionEnqueueResult {
        self.requests.lock().push(request);
        self.result
    }

    fn session_ended(&self, end: NativeIncActionEnd) {
        self.ends.lock().push(end);
    }
}

fn controller(temp: &TempDir) -> Arc<RuntimeController> {
    RuntimeController::open(
        RuntimeConfig {
            runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
            nmp_store_path: None,
            artifact_cache_path: temp.path().join("artifacts").display().to_string(),
            ..RuntimeConfig::default()
        },
        Box::new(FixtureSource(BTreeMap::from([(
            DIGEST.to_owned(),
            INDEX.to_vec(),
        )]))),
    )
    .unwrap()
}

fn controller_with_native_capabilities(
    temp: &TempDir,
    requests: Arc<Mutex<Vec<NativeSettingsRequest>>>,
) -> Arc<RuntimeController> {
    RuntimeController::open_with_native_capabilities(
        RuntimeConfig {
            runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
            nmp_store_path: None,
            artifact_cache_path: temp.path().join("artifacts").display().to_string(),
            ..RuntimeConfig::default()
        },
        Box::new(FixtureSource(BTreeMap::from([(
            DIGEST.to_owned(),
            INDEX.to_vec(),
        )]))),
        Box::new(FixtureAppearance),
        Box::new(RecordingSettings { requests }),
    )
    .unwrap()
}

fn controller_with_all_native_capabilities(
    temp: &TempDir,
    requests: Arc<Mutex<Vec<NativeIncActionRequest>>>,
    ends: Arc<Mutex<Vec<NativeIncActionEnd>>>,
    result: NativeIncActionEnqueueResult,
) -> Arc<RuntimeController> {
    RuntimeController::open_with_all_native_capabilities(
        RuntimeConfig {
            runtime_store_path: temp.path().join("runtime.sqlite3").display().to_string(),
            nmp_store_path: None,
            artifact_cache_path: temp.path().join("artifacts").display().to_string(),
            ..RuntimeConfig::default()
        },
        Box::new(FixtureSource(BTreeMap::from([(
            DIGEST.to_owned(),
            INDEX.to_vec(),
        )]))),
        Box::new(FixtureAppearance),
        Box::new(RecordingSettings {
            requests: Arc::new(Mutex::new(Vec::new())),
        }),
        Box::new(RecordingIncActions {
            requests,
            ends,
            result,
        }),
    )
    .unwrap()
}

fn workspace_definition(id: &str) -> RuntimeWorkspaceDefinition {
    RuntimeWorkspaceDefinition {
        schema_version: WORKSPACE_SCHEMA_VERSION,
        workspace_id: id.to_owned(),
        axis: RuntimeWorkspaceAxis::Horizontal,
        slots: vec![
            RuntimeWorkspaceSlot {
                slot_id: "feed".to_owned(),
                role: RuntimeWorkspaceRole::Feed,
                renderer: RuntimeWorkspaceRenderer::LegacyNapplet,
                handler_id: "good-morning".to_owned(),
                manifest_author: Some(AUTHOR.to_owned()),
                d_tag: Some("good-morning".to_owned()),
                aggregate_hash: Some("a".repeat(64)),
                binding_parameters_json: r#"{"window":{"limit":100}}"#.to_owned(),
                navigation_json: r#"{"selection":null}"#.to_owned(),
                visible: true,
                order: 0,
                size_points: 640,
                minimum_points: 320,
                maximum_points: 1_200,
            },
            RuntimeWorkspaceSlot {
                slot_id: "detail".to_owned(),
                role: RuntimeWorkspaceRole::Detail,
                renderer: RuntimeWorkspaceRenderer::Native,
                handler_id: "native-detail".to_owned(),
                manifest_author: None,
                d_tag: None,
                aggregate_hash: None,
                binding_parameters_json: "{}".to_owned(),
                navigation_json: "{}".to_owned(),
                visible: true,
                order: 1,
                size_points: 360,
                minimum_points: 240,
                maximum_points: 900,
            },
        ],
        focused_slot_id: Some("feed".to_owned()),
        activity_drawer_visible: true,
        preferences_json: r#"{"sidebar":"home"}"#.to_owned(),
        retained_receipt_ids: Vec::new(),
    }
}

fn exact_coordinate(artifact: &VerifiedArtifact) -> RuntimeExactBuildCoordinate {
    RuntimeExactBuildCoordinate {
        manifest_author: artifact.author(),
        d_tag: artifact.d_tag().expect("named fixture"),
        aggregate_hash: artifact.aggregate_hash(),
    }
}

fn install_permission_fixture(controller: &Arc<RuntimeController>) -> RuntimeExactBuildCoordinate {
    let artifact = controller
        .verify_artifact(
            EVENT.to_vec(),
            ArtifactCoordinate::Named {
                author: AUTHOR.to_owned(),
                d_tag: "good-morning".to_owned(),
            },
        )
        .artifact
        .unwrap();
    let principal = artifact.principal.clone().unwrap();
    let executable: Arc<dyn ExecutableArtifact> = artifact.handle.clone();
    controller.app.dispatch(PlatformCommand::InstallVerified {
        build: InstalledBuild {
            principal,
            title: Arc::from("Good Morning Protocol"),
            manifest_metadata: BoundedJson::from_value(&serde_json::json!({"kind": 35129}), 1_024)
                .unwrap(),
            capability_requests: vec![
                CapabilityRequest {
                    capability: Capability::new("identity").unwrap(),
                    requirement: CapabilityRequirement::Required,
                },
                CapabilityRequest {
                    capability: Capability::new("missing").unwrap(),
                    requirement: CapabilityRequirement::Optional,
                },
            ],
        },
        artifact: executable,
    });
    exact_coordinate(&artifact)
}

/// Installs the fixture declaring `lists`, the domain a napplet uses to change
/// the user's follow/mute/bookmark lists.
fn install_lists_fixture(controller: &Arc<RuntimeController>) -> RuntimeExactBuildCoordinate {
    let artifact = controller
        .verify_artifact(
            EVENT.to_vec(),
            ArtifactCoordinate::Named {
                author: AUTHOR.to_owned(),
                d_tag: "good-morning".to_owned(),
            },
        )
        .artifact
        .unwrap();
    let principal = artifact.principal.clone().unwrap();
    let executable: Arc<dyn ExecutableArtifact> = artifact.handle.clone();
    controller.app.dispatch(PlatformCommand::InstallVerified {
        build: InstalledBuild {
            principal,
            title: Arc::from("Lists napplet"),
            manifest_metadata: BoundedJson::from_value(&serde_json::json!({"kind": 35129}), 1_024)
                .unwrap(),
            capability_requests: vec![CapabilityRequest {
                capability: Capability::new("lists").unwrap(),
                requirement: CapabilityRequirement::Required,
            }],
        },
        artifact: executable,
    });
    exact_coordinate(&artifact)
}

fn install_and_launch(
    controller: &Arc<RuntimeController>,
    domains: &[&str],
) -> (Arc<VerifiedArtifact>, u64) {
    let artifact = controller
        .verify_artifact(
            EVENT.to_vec(),
            ArtifactCoordinate::Named {
                author: AUTHOR.to_owned(),
                d_tag: "good-morning".to_owned(),
            },
        )
        .artifact
        .unwrap();
    controller.install(Arc::clone(&artifact));
    // Every domain the fixture's own `napplet-requires` meta declares is
    // required, so all of them must be granted before it will launch. The
    // runtime no longer softens any of them to optional on the strength of
    // this build's identity.
    for domain in GOOD_MORNING_DECLARED_DOMAINS
        .into_iter()
        .chain(domains.iter().copied())
    {
        controller.set_grant(
            Arc::clone(&artifact),
            domain.to_owned(),
            RuntimeSensitivity::Ordinary,
            RuntimeGrantDecision::AllowExactBuild,
        );
    }
    controller.launch(Arc::clone(&artifact), RuntimeExecutionProfile::Legacy);
    let session = controller.snapshot_value().sessions[0].id;
    controller.mapped_envelope(session, br#"{"type":"shell.ready"}"#.to_vec());
    (artifact, session)
}

fn response_of_type(controller: &RuntimeController, expected: &str) -> Value {
    controller
        .app
        .events_after(0)
        .events
        .into_iter()
        .rev()
        .find_map(|event| match event.event {
            PlatformEvent::EnvelopeHandled {
                response: Some(response),
                ..
            } if response.decode().ok()?.get("type")? == expected => response.decode().ok(),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing `{expected}` response"))
}

/// Signs a synthetic single-file napplet manifest locally so a test can
/// declare exactly the `requires`/`archetype` tags it needs without
/// depending on any published fixture's immutable tag set.
fn signed_manifest_event(
    d_tag: &str,
    content: &[u8],
    extra_tags: Vec<Vec<String>>,
) -> (Vec<u8>, String, String) {
    let digest = Sha256Digest::of(content);
    let aggregate = Nip5aPathTagsAggregate
        .compute(&[VerifiedFile {
            path: Arc::from("/index.html"),
            digest: digest.clone(),
            bytes: Arc::from(content),
        }])
        .unwrap();
    let mut tags = vec![
        vec!["d".to_owned(), d_tag.to_owned()],
        vec![
            "path".to_owned(),
            "/index.html".to_owned(),
            digest.as_str().to_owned(),
        ],
        vec![
            "x".to_owned(),
            aggregate.as_str().to_owned(),
            "aggregate".to_owned(),
        ],
        vec!["server".to_owned(), "https://blossom.example/".to_owned()],
    ];
    tags.extend(extra_tags);
    let keys = Keys::generate();
    let event = EventBuilder::new(Kind::Custom(35_129), "")
        .tags(
            tags.into_iter()
                .map(|tag| Tag::parse(tag).unwrap())
                .collect::<Vec<_>>(),
        )
        .sign_with_keys(&keys)
        .unwrap();
    (
        serde_json::to_vec(&event).unwrap(),
        event.pubkey.to_hex(),
        digest.as_str().to_owned(),
    )
}
