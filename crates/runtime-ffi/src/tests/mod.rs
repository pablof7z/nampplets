//! Boundary tests for the UniFFI runtime projection.

mod accounts;
mod artifact;
mod catalog;
mod envelope;
mod library;
mod native_capabilities;
mod permissions;
mod workspace;

use std::{collections::BTreeMap, fs, sync::Arc};

use nmp_native_artifact::ManifestCoordinate;
use nmp_native_runtime_app::{AppLimits, ExecutableArtifact, PlatformCommand, PlatformEvent};
use nmp_native_runtime_core::{
    BoundedJson, Capability, CapabilityRequest, CapabilityRequirement, Principal, SessionId,
};
use nmp_native_runtime_store::{InstalledBuild, WorkspaceRecord};
use parking_lot::Mutex;
use serde_json::Value;
use tempfile::TempDir;

use crate::{
    projection::{parse_catalog_coordinate, project_event},
    workspace::workspace_record_from_ffi,
    *,
};

const EVENT: &[u8] =
    include_bytes!("../../../../conformance/napplet-corpus/published/good-morning/event.json");
const INDEX: &[u8] =
    include_bytes!("../../../../conformance/napplet-corpus/published/good-morning/index.html");
const AUTHOR: &str = "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5";
const DIGEST: &str = "ffd35eea5c84d03cdda74c23e1bbb2c40500f503833503aa688036faa52f3808";

struct FixtureSource(BTreeMap<String, Vec<u8>>);

impl ArtifactSource for FixtureSource {
    fn fetch(&self, request: ArtifactFetchRequest) -> ArtifactFetchResponse {
        let bytes = self
            .0
            .get(&request.expected_sha256)
            .cloned()
            .unwrap_or_default();
        ArtifactFetchResponse::Body {
            source_url: request.candidate_urls[0].clone(),
            http_status: 200,
            bytes,
        }
    }
}

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
    for domain in ["identity", "inc", "outbox"]
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
    let session = controller.snapshot().sessions[0].id;
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
