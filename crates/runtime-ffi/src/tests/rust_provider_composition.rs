use std::collections::{BTreeMap, BTreeSet};

use nmp_native_nap_bridge::{
    Provider, ProviderCall, ProviderDescriptor, ProviderError, ProviderPlatformAvailability,
    ProviderRequest,
};
use nmp_native_providers::PINNED_NAP_PROTOCOL;

use super::*;

#[derive(Debug)]
struct TestResourceProvider {
    descriptor: ProviderDescriptor,
}

impl TestResourceProvider {
    fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor {
                domain: Capability::new("resource").unwrap(),
                protocol_versions: BTreeSet::from([Arc::from(PINNED_NAP_PROTOCOL)]),
                actions: BTreeSet::from([Arc::from("info")]),
                sensitive: true,
                dependencies: BTreeSet::new(),
                platform_availability: ProviderPlatformAvailability::Available,
            },
        }
    }
}

impl Provider for TestResourceProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn call(&self, _request: ProviderRequest) -> Result<ProviderCall, ProviderError> {
        Ok(ProviderCall::completed(None))
    }
}

fn controller_with_rust_resource_provider(temp: &TempDir) -> Arc<RuntimeController> {
    RuntimeController::open_with_settings_and_rust_providers(
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
        Box::new(RecordingSettings {
            requests: Arc::new(Mutex::new(Vec::new())),
        }),
        vec![Arc::new(TestResourceProvider::new())],
    )
    .unwrap()
}

#[test]
fn rust_native_provider_uses_the_ordinary_permission_and_lifecycle_registry() {
    let temp = TempDir::new().unwrap();
    let controller = controller_with_rust_resource_provider(&temp);
    let artifact = controller
        .verify_artifact(
            EVENT.to_vec(),
            ArtifactCoordinate::Named {
                author: AUTHOR.to_owned(),
                d_tag: D_TAG.to_owned(),
            },
        )
        .artifact
        .unwrap();
    controller.install(Arc::clone(&artifact));
    let review = controller
        .permission_review(exact_coordinate(&artifact))
        .review
        .unwrap();
    let resource = review
        .capabilities
        .iter()
        .find(|capability| capability.domain == "resource")
        .unwrap();
    assert_eq!(
        resource.platform_availability,
        RuntimePermissionPlatformAvailability::Available
    );
    assert_eq!(
        resource.controller,
        RuntimePermissionDecisionController::User
    );
}
