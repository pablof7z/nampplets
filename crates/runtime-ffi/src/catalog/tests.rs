use super::feed::connecting_catalog_frame;
use super::*;
use nmp::{EngineConfig, RelayUrl, WindowLoad};
use nmp_native_artifact::{INDEX_PATH, ManifestCoordinate};
use tempfile::TempDir;

const LIVE_STL_PREVIEW_AUTHOR: &str =
    "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5";
const LIVE_STL_PREVIEW_D_TAG: &str = "stl-preview";
const LIVE_STL_PREVIEW_EVENT_ID: &str =
    "824b14a84042b4f911061d158d3de90c38ab8e17655da253239baecae7c994d6";
const LIVE_STL_PREVIEW_AGGREGATE: &str =
    "eea534010867c1fa6c41c012ea237b0ad77ec428693172010b124cb7f2048ade";

#[test]
fn initial_feed_snapshot_is_connecting_instead_of_terminally_empty() {
    let frame = connecting_catalog_frame();
    assert!(frame.candidates.is_empty());
    assert_eq!(frame.window_load, WindowLoad::Requesting);
}

#[ignore = "requires explicit public relay and HTTPS access"]
#[test]
fn live_exact_manifest_confirms_into_a_verified_artifact() {
    let configured = std::env::var("NMP_LIVE_CATALOG_RELAYS")
        .expect("set NMP_LIVE_CATALOG_RELAYS to a comma-separated operator relay set");
    let relays = configured
        .split(',')
        .map(str::trim)
        .filter(|relay| !relay.is_empty())
        .map(|relay| {
            RelayUrl::parse(relay).expect("live relay URLs must be valid");
            relay.to_owned()
        })
        .collect::<Vec<_>>();
    assert!(
        !relays.is_empty(),
        "configure at least one live catalog relay"
    );

    let temp = TempDir::new().expect("the live proof needs an isolated artifact cache");
    let data_plane = Arc::new(
        NmpDataPlane::open(
            EngineConfig {
                indexer_relays: relays.clone(),
                app_relays: relays,
                ..EngineConfig::default()
            },
            4,
        )
        .expect("the pinned NMP facade must open the configured relay plan"),
    );
    let artifact_limits = ArtifactLimits::default();
    let artifact_cache = Arc::new(
        FileArtifactCache::open(temp.path().join("artifacts"))
            .expect("the isolated artifact cache must open"),
    );
    let service = RuntimeCatalogService::new(
        Arc::clone(&data_plane),
        artifact_cache,
        artifact_limits,
        1024 * 1024,
        8,
    )
    .expect("the bounded runtime catalog service must open");

    let coordinate = ManifestCoordinate::named(LIVE_STL_PREVIEW_AUTHOR, LIVE_STL_PREVIEW_D_TAG)
        .expect("the live proof coordinate must remain valid");
    let review = service
        .begin_review(coordinate)
        .expect("the exact NMP lookup must produce a review");
    assert_eq!(review.event_id, LIVE_STL_PREVIEW_EVENT_ID);
    assert_eq!(review.manifest_author, LIVE_STL_PREVIEW_AUTHOR);
    assert_eq!(review.d_tag.as_deref(), Some(LIVE_STL_PREVIEW_D_TAG));
    assert_eq!(review.aggregate_hash, LIVE_STL_PREVIEW_AGGREGATE);

    let confirmed = service
        .confirm_review(&review.token)
        .expect("the reviewed HTTPS artifact must verify and enter the cache");
    assert_eq!(confirmed.confirmation.event_id, LIVE_STL_PREVIEW_EVENT_ID);
    assert_eq!(
        confirmed.confirmation.manifest_author,
        LIVE_STL_PREVIEW_AUTHOR
    );
    assert_eq!(
        confirmed.confirmation.d_tag.as_deref(),
        Some(LIVE_STL_PREVIEW_D_TAG)
    );
    assert_eq!(
        confirmed.confirmation.aggregate_hash,
        LIVE_STL_PREVIEW_AGGREGATE
    );

    let handle = confirmed.into_handle();
    assert_eq!(
        handle.index().event_id().as_str(),
        LIVE_STL_PREVIEW_EVENT_ID
    );
    assert_eq!(handle.index().author().as_str(), LIVE_STL_PREVIEW_AUTHOR);
    assert_eq!(handle.index().d_tag(), Some(LIVE_STL_PREVIEW_D_TAG));
    assert_eq!(
        handle.index().aggregate().as_str(),
        LIVE_STL_PREVIEW_AGGREGATE
    );
    let index = handle
        .read_verified(INDEX_PATH, artifact_limits.maximum_file_bytes)
        .expect("the cached /index.html must re-verify");
    assert!(
        !index.is_empty(),
        "the verified /index.html must not be empty"
    );

    service.close();
    data_plane.close();
    println!(
        "NMP_LIVE_VERIFIED_ARTIFACT_EVENT={} aggregate={} index_bytes={}",
        LIVE_STL_PREVIEW_EVENT_ID,
        LIVE_STL_PREVIEW_AGGREGATE,
        index.len()
    );
}
