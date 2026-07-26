use std::collections::BTreeMap;

use crate::{
    ArtifactFetchRequest, ArtifactFetchResponse, ArtifactSource, RuntimeController,
    RuntimeSnapshot, RuntimeSnapshotProjection,
};

pub(super) trait RuntimeControllerSnapshotTestExt {
    fn snapshot_value(&self) -> RuntimeSnapshot;
}

impl RuntimeControllerSnapshotTestExt for RuntimeController {
    fn snapshot_value(&self) -> RuntimeSnapshot {
        match self.snapshot() {
            RuntimeSnapshotProjection::Snapshot { snapshot } => snapshot,
            RuntimeSnapshotProjection::Refused {
                revision,
                closed,
                refusal,
            } => panic!(
                "runtime snapshot revision {revision} (closed={closed}) was refused: {}: {}",
                refusal.code, refusal.detail
            ),
        }
    }
}

pub(super) const EVENT: &[u8] =
    include_bytes!("../../../../conformance/napplet-corpus/published/good-morning/event.json");
pub(super) const INDEX: &[u8] =
    include_bytes!("../../../../conformance/napplet-corpus/published/good-morning/index.html");
pub(super) const AUTHOR: &str = "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5";
pub(super) const DIGEST: &str = "ffd35eea5c84d03cdda74c23e1bbb2c40500f503833503aa688036faa52f3808";

pub(super) struct FixtureSource(pub(super) BTreeMap<String, Vec<u8>>);

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
