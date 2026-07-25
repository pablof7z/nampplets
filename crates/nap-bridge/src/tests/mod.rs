use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use nmp_native_runtime_core::{
    ApprovedWrite, BoundedJson, Cancellation, Capability, ExecutionProfile, GrantDecision,
    GrantLedger, GrantLimits, Principal, ReceiptEventSink, ResourceClass, ResourceLimits,
    ResourceTracker, Sensitivity, SessionId,
};
use parking_lot::Mutex;

use super::*;

#[derive(Debug)]
struct EchoProvider {
    descriptor: ProviderDescriptor,
    calls: Arc<AtomicUsize>,
}

impl Provider for EchoProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn call(&self, request: ProviderRequest) -> Result<ProviderCall, ProviderError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(ProviderCall::completed(Some(
            BoundedJson::from_value(&request.payload, 1024).unwrap(),
        )))
    }
}

fn fixture(burst: u32) -> (ProviderRegistry, Principal, Arc<GrantLedger>, Capability) {
    let resources = Arc::new(ResourceTracker::new(ResourceLimits::default()).unwrap());
    let grants =
        Arc::new(GrantLedger::new(GrantLimits::default(), Arc::clone(&resources)).unwrap());
    let activity = Arc::new(MemoryActivitySink::bounded(32));
    let mut registry = ProviderRegistry::new(
        BridgeLimits {
            message_burst: burst,
            ..BridgeLimits::default()
        },
        resources,
        Arc::clone(&grants),
        activity,
    )
    .unwrap();
    let domain = Capability::new("storage").unwrap();
    registry
        .register(Arc::new(EchoProvider {
            descriptor: ProviderDescriptor {
                domain: domain.clone(),
                protocol_versions: BTreeSet::from([Arc::from("1")]),
                actions: BTreeSet::from([Arc::from("get")]),
                sensitive: false,
                dependencies: BTreeSet::new(),
                platform_availability: ProviderPlatformAvailability::Available,
            },
            calls: Arc::new(AtomicUsize::new(0)),
        }))
        .unwrap();
    let principal = Principal::new("a".repeat(64), "app", "b".repeat(64)).unwrap();
    grants
        .set(
            principal.clone(),
            domain.clone(),
            Sensitivity::Ordinary,
            GrantDecision::AllowExactBuild,
        )
        .unwrap();
    (registry, principal, grants, domain)
}

mod dispatch;
mod lifecycle;
