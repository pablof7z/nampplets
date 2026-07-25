//! Shared fixtures for `crates/runtime-app` integration tests.
//!
//! Both the plain `#[test]` suites (`tests/kernel_*.rs`) and the cucumber
//! scenario runner (`tests/bdd.rs`) drive the exact same `Rig` so that BDD
//! scenarios exercise the identical bootstrap path as the existing tests
//! they were ported from. Each binary only exercises a subset of this
//! module's surface, so unused-by-this-binary items are expected.
#![allow(dead_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use nmp_native_nap_bridge::{
    BridgeLimits, Provider, ProviderCall, ProviderDescriptor, ProviderError,
    ProviderPlatformAvailability, ProviderPushSender, ProviderRequest, ProviderSession,
    ProviderSessionContext, ProviderSessionEnd,
};
use nmp_native_providers::{
    ShellEnvironment, ShellEnvironmentError, ShellEnvironmentLimits, ShellEnvironmentSource,
    ShellProvider, ShellProviderLimits,
};
use nmp_native_runtime_app::{
    AppLimits, ExecutableArtifact, KernelClock, PlatformCommand, RuntimeApp, RuntimeAppConfig,
};
use nmp_native_runtime_core::{
    BoundedJson, Capability, CapabilityRequest, CapabilityRequirement, GrantDecision, GrantLimits,
    Principal, ResourceLimits, Sensitivity, SessionId,
};
use nmp_native_runtime_store::{InstalledBuild, RuntimeStore, StoreLimits};
use nmp_native_surface::BindingLimits;
use nmp_native_test_harness::FakeHostDataPlane;
use parking_lot::Mutex;
use serde_json::Value;
use tempfile::TempDir;

#[derive(Debug)]
pub struct TestClock(AtomicU64);

impl TestClock {
    pub fn new(now: u64) -> Self {
        Self(AtomicU64::new(now))
    }
}

impl KernelClock for TestClock {
    fn now_millis(&self) -> u64 {
        self.0.fetch_add(1, Ordering::AcqRel)
    }
}

#[derive(Debug)]
pub struct TestArtifact {
    pub kind: u16,
    pub author: String,
    pub d_tag: String,
    pub aggregate: String,
}

impl ExecutableArtifact for TestArtifact {
    fn manifest_kind(&self) -> u16 {
        self.kind
    }

    fn manifest_author(&self) -> &str {
        &self.author
    }

    fn d_tag(&self) -> Option<&str> {
        Some(&self.d_tag)
    }

    fn aggregate_hash(&self) -> &str {
        &self.aggregate
    }

    fn contains_logical_path(&self, logical_path: &str) -> bool {
        logical_path == "/index.html"
    }
}

#[derive(Debug)]
pub struct CapturingProvider {
    pub descriptor: ProviderDescriptor,
    pub seen: Mutex<Vec<(Principal, SessionId, Value)>>,
    pub opened: Mutex<Vec<ProviderSessionContext>>,
    pub ready: Mutex<Vec<ProviderSessionContext>>,
    pub closed: Mutex<Vec<(ProviderSessionContext, ProviderSessionEnd)>>,
    pub revoked: Mutex<Vec<ProviderSessionContext>>,
    pub outbound: Mutex<BTreeMap<SessionId, ProviderPushSender>>,
    pub streaming: bool,
}

impl CapturingProvider {
    pub fn new(streaming: bool) -> Self {
        Self {
            descriptor: ProviderDescriptor {
                domain: canary(),
                protocol_versions: BTreeSet::from([Arc::from("internal-canary/1")]),
                actions: BTreeSet::from([Arc::from("ping")]),
                sensitive: false,
                dependencies: BTreeSet::new(),
                platform_availability: ProviderPlatformAvailability::Available,
            },
            seen: Mutex::new(Vec::new()),
            opened: Mutex::new(Vec::new()),
            ready: Mutex::new(Vec::new()),
            closed: Mutex::new(Vec::new()),
            revoked: Mutex::new(Vec::new()),
            outbound: Mutex::new(BTreeMap::new()),
            streaming,
        }
    }

    pub fn sender(&self, session: SessionId) -> ProviderPushSender {
        self.outbound.lock().get(&session).cloned().unwrap()
    }

    pub fn with_dependencies(mut self, dependencies: impl IntoIterator<Item = Capability>) -> Self {
        self.descriptor.dependencies = dependencies.into_iter().collect();
        self
    }
}

impl Provider for CapturingProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn call(&self, request: ProviderRequest) -> Result<ProviderCall, ProviderError> {
        self.seen
            .lock()
            .push((request.principal, request.session, request.payload));
        if self.streaming {
            Ok(ProviderCall::streaming(None, request.work))
        } else {
            Ok(ProviderCall::completed(None))
        }
    }

    fn session_opened(&self, session: ProviderSession) -> Result<(), ProviderError> {
        self.opened.lock().push(session.context.clone());
        self.outbound
            .lock()
            .insert(session.context.session, session.outbound);
        Ok(())
    }

    fn session_ready(&self, session: &ProviderSessionContext) -> Result<(), ProviderError> {
        self.ready.lock().push(session.clone());
        Ok(())
    }

    fn session_closed(&self, session: &ProviderSessionContext, reason: ProviderSessionEnd) {
        self.closed.lock().push((session.clone(), reason));
    }

    fn session_revoked(&self, session: &ProviderSessionContext) {
        self.revoked.lock().push(session.clone());
    }
}

#[derive(Debug)]
pub struct FixedShellEnvironment {
    pub override_domains: Option<BTreeSet<Capability>>,
}

impl ShellEnvironmentSource for FixedShellEnvironment {
    fn environment(
        &self,
        _principal: &Principal,
        _session: SessionId,
        offered_domains: &BTreeSet<Capability>,
    ) -> Result<ShellEnvironment, ShellEnvironmentError> {
        ShellEnvironment::new(
            self.override_domains
                .as_ref()
                .unwrap_or(offered_domains)
                .iter()
                .cloned(),
            [Arc::from("settings")],
            ShellEnvironmentLimits::default(),
        )
    }
}

#[derive(Debug)]
pub struct Rig {
    pub _directory: TempDir,
    pub store: Arc<RuntimeStore>,
    pub host: Arc<FakeHostDataPlane>,
    pub provider: Arc<CapturingProvider>,
    pub shell_provider: Arc<ShellProvider>,
    pub app: Arc<RuntimeApp>,
}

impl Rig {
    pub fn new(streaming: bool) -> Self {
        let directory = TempDir::new().unwrap();
        let store = Arc::new(
            RuntimeStore::open(directory.path().join("runtime.db"), StoreLimits::default())
                .unwrap(),
        );
        let host = Arc::new(FakeHostDataPlane::new(16));
        let provider = Arc::new(CapturingProvider::new(streaming));
        let (app, shell_provider) = open_app(Arc::clone(&store), host.clone(), provider.clone());
        Self {
            _directory: directory,
            store,
            host,
            provider,
            shell_provider,
            app,
        }
    }

    pub fn install(&self, principal: Principal) {
        self.install_with_requests(principal, Vec::new());
    }

    pub fn install_with_requests(
        &self,
        principal: Principal,
        capability_requests: Vec<CapabilityRequest>,
    ) {
        self.app.dispatch(PlatformCommand::InstallVerified {
            build: InstalledBuild {
                principal: principal.clone(),
                title: Arc::from("Test napplet"),
                manifest_metadata: json(serde_json::json!({"kind": 34128})),
                capability_requests,
            },
            artifact: Arc::new(TestArtifact {
                kind: 35_129,
                author: principal.manifest_author().to_owned(),
                d_tag: principal.d_tag().to_owned(),
                aggregate: principal.aggregate_hash().to_owned(),
            }),
        });
    }

    pub fn allow_runtime(&self, principal: Principal) {
        self.app.dispatch(PlatformCommand::SetGrant {
            principal,
            capability: canary(),
            sensitivity: Sensitivity::Ordinary,
            decision: GrantDecision::AllowExactBuild,
        });
    }

    pub fn launch(&self, principal: Principal) -> SessionId {
        self.app.dispatch(PlatformCommand::Launch {
            principal,
            profile: nmp_native_runtime_core::ExecutionProfile::Legacy,
            required_domains: BTreeSet::from([canary()]),
        });
        self.app.snapshot().sessions.last().unwrap().id
    }

    pub fn ready(&self, session: SessionId) {
        self.app.dispatch(PlatformCommand::MappedEnvelope {
            session,
            bytes: ready(),
        });
    }
}

pub fn open_app(
    store: Arc<RuntimeStore>,
    host: Arc<FakeHostDataPlane>,
    provider: Arc<CapturingProvider>,
) -> (Arc<RuntimeApp>, Arc<ShellProvider>) {
    open_app_with_shell_source(
        store,
        host,
        provider,
        Arc::new(FixedShellEnvironment {
            override_domains: None,
        }),
    )
}

pub fn open_app_with_shell_domains(
    store: Arc<RuntimeStore>,
    host: Arc<FakeHostDataPlane>,
    provider: Arc<CapturingProvider>,
    shell_domains: BTreeSet<Capability>,
) -> (Arc<RuntimeApp>, Arc<ShellProvider>) {
    open_app_with_shell_source(
        store,
        host,
        provider,
        Arc::new(FixedShellEnvironment {
            override_domains: Some(shell_domains),
        }),
    )
}

pub fn open_app_with_shell_source(
    store: Arc<RuntimeStore>,
    host: Arc<FakeHostDataPlane>,
    provider: Arc<CapturingProvider>,
    shell_environment: Arc<dyn ShellEnvironmentSource>,
) -> (Arc<RuntimeApp>, Arc<ShellProvider>) {
    let data_plane: Arc<dyn nmp_native_runtime_core::HostDataPlane> = host;
    let provider: Arc<dyn Provider> = provider;
    let shell_provider =
        Arc::new(ShellProvider::new(shell_environment, ShellProviderLimits::default()).unwrap());
    let app = RuntimeApp::open(RuntimeAppConfig {
        limits: AppLimits::default(),
        resource_limits: ResourceLimits::default(),
        grant_limits: GrantLimits::default(),
        bridge_limits: BridgeLimits::default(),
        binding_limits: BindingLimits::default(),
        store,
        data_plane,
        clock: Arc::new(TestClock::new(1_000)),
        shell_provider: shell_provider.clone(),
        providers: vec![provider],
    })
    .unwrap();
    (app, shell_provider)
}

pub fn principal(hash: char) -> Principal {
    Principal::new("a".repeat(64), "test-napplet", hash.to_string().repeat(64)).unwrap()
}

pub fn shell() -> Capability {
    Capability::new("shell").unwrap()
}

pub fn canary() -> Capability {
    Capability::new("canary").unwrap()
}

pub fn request(capability: Capability, requirement: CapabilityRequirement) -> CapabilityRequest {
    CapabilityRequest {
        capability,
        requirement,
    }
}

pub fn permission(
    capability: Capability,
    decision: GrantDecision,
) -> nmp_native_runtime_app::PermissionDecision {
    nmp_native_runtime_app::PermissionDecision {
        capability,
        decision,
    }
}

pub fn json(value: Value) -> BoundedJson {
    BoundedJson::from_value(&value, 16 * 1024).unwrap()
}

pub fn ping(payload: Value) -> Arc<[u8]> {
    Arc::from(
        serde_json::to_vec(&serde_json::json!({
            "type": "canary.ping",
            "id": "request-1",
            "payload": payload
        }))
        .unwrap(),
    )
}

pub fn ready() -> Arc<[u8]> {
    Arc::from(serde_json::to_vec(&serde_json::json!({"type": "shell.ready"})).unwrap())
}

pub fn mapped(value: Value) -> Arc<[u8]> {
    Arc::from(serde_json::to_vec(&value).unwrap())
}

pub fn wait_for_event(
    app: &Arc<RuntimeApp>,
    mut predicate: impl FnMut(&nmp_native_runtime_app::PlatformEvent) -> bool,
) -> nmp_native_runtime_app::PlatformEvent {
    let mut observer = app.observe();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    if let Some(event) = app
                        .events_after(0)
                        .events
                        .into_iter()
                        .map(|event| event.event)
                        .find(|event| predicate(event))
                    {
                        return event;
                    }
                    observer.changed().await.unwrap();
                }
            })
            .await
            .expect("event-driven app observation timed out")
        })
}

#[derive(Debug)]
pub struct WriteReceiptIdForTest;

impl WriteReceiptIdForTest {
    pub fn value() -> nmp_native_runtime_core::WriteReceiptId {
        nmp_native_runtime_core::WriteReceiptId(Arc::from("fake-receipt-999"))
    }
}
