use std::{collections::BTreeMap, fs, io::Cursor};

use nostr::{EventBuilder, Keys, Kind, Tag};
use serde_json::Value;
use tempfile::TempDir;

use super::*;
use crate::{
    AggregateVerifier as _, ArtifactCache as _, ArtifactError, ArtifactLimits, BlobSourceError,
    FileArtifactCache, INDEX_PATH, Nip5aPathTagsAggregate,
};

const PUBLISHED_EVENT: &[u8] =
    include_bytes!("../../../../conformance/napplet-corpus/published/good-morning/event.json");
const PUBLISHED_INDEX: &[u8] =
    include_bytes!("../../../../conformance/napplet-corpus/published/good-morning/index.html");
const PUBLISHED_AUTHOR: &str = "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5";
const PUBLISHED_ID: &str = "b330bfaefd2ddf268ebe4196403e6163533c54f41dabc3518bdc1a896c68f40e";
const PUBLISHED_AGGREGATE: &str =
    "828a6df02afd56782ea20f805084acce65c53f7c37554948c1e0a64aa5a2b0a8";
const EXTERNAL_INDEX: &[u8] =
    include_bytes!("../../../../conformance/napplet-corpus/reference/external-assets/index.html");
const EXTERNAL_SCRIPT: &[u8] =
    include_bytes!("../../../../conformance/napplet-corpus/reference/external-assets/main.js");
const EXTERNAL_STYLE: &[u8] =
    include_bytes!("../../../../conformance/napplet-corpus/reference/external-assets/style.css");
const EXTERNAL_AGGREGATE: &str = "0136a6481a347a856d877c8729650222cc6ca8110095f35a9f2bd016b3534d81";

#[derive(Debug)]
struct FixtureSource {
    response: FixtureResponse,
}

#[derive(Debug)]
enum FixtureResponse {
    Bytes(Vec<u8>),
    Redirect,
    WrongSource(Vec<u8>),
}

impl ManifestBlobSource for FixtureSource {
    fn fetch(&self, request: &BlobFetchRequest) -> Result<BlobFetchResponse, BlobSourceError> {
        let selected = request.candidate_urls().next().unwrap().to_owned();
        Ok(match &self.response {
            FixtureResponse::Bytes(bytes) => {
                BlobFetchResponse::ok(selected, Box::new(Cursor::new(bytes.clone())))
            }
            FixtureResponse::Redirect => {
                BlobFetchResponse::redirect(selected, 302, "https://example.invalid/evil")
            }
            FixtureResponse::WrongSource(bytes) => BlobFetchResponse::ok(
                "https://example.invalid/unapproved",
                Box::new(Cursor::new(bytes.clone())),
            ),
        })
    }
}

#[derive(Debug)]
struct DigestMapSource(BTreeMap<String, Vec<u8>>);

impl ManifestBlobSource for DigestMapSource {
    fn fetch(&self, request: &BlobFetchRequest) -> Result<BlobFetchResponse, BlobSourceError> {
        let bytes = self
            .0
            .get(request.digest().as_str())
            .ok_or_else(|| BlobSourceError {
                reason: "fixture digest not found".to_owned(),
            })?
            .clone();
        Ok(BlobFetchResponse::ok(
            request.candidate_urls().next().unwrap(),
            Box::new(Cursor::new(bytes)),
        ))
    }
}

fn coordinate() -> ManifestCoordinate {
    ManifestCoordinate::named(PUBLISHED_AUTHOR, "good-morning").unwrap()
}

fn signed_named_manifest(tags: Vec<Vec<String>>) -> (Vec<u8>, ManifestCoordinate) {
    let keys = Keys::generate();
    let event = EventBuilder::new(Kind::Custom(NAPPLET_KIND_NAMED), "")
        .tags(
            tags.into_iter()
                .map(|tag| Tag::parse(tag).unwrap())
                .collect::<Vec<_>>(),
        )
        .sign_with_keys(&keys)
        .unwrap();
    let coordinate = ManifestCoordinate::named(&event.pubkey.to_hex(), "fixture").unwrap();
    (serde_json::to_vec(&event).unwrap(), coordinate)
}

#[test]
fn pinned_published_manifest_verifies_signature_id_and_exact_aggregate() {
    let verified = ManifestEventVerifier::pinned()
        .verify_json(PUBLISHED_EVENT, &coordinate())
        .unwrap();
    assert_eq!(verified.event_id().as_str(), PUBLISHED_ID);
    assert_eq!(verified.author().as_str(), PUBLISHED_AUTHOR);
    assert_eq!(verified.aggregate().as_str(), PUBLISHED_AGGREGATE);
    assert_eq!(verified.mode(), ArtifactMode::SingleFile);

    let path = &verified.artifact.paths[0];
    let aggregate = Nip5aPathTagsAggregate
        .compute(&[crate::VerifiedFile {
            path: Arc::from(path.path.as_str()),
            digest: path.sha256.clone(),
            bytes: Arc::from(PUBLISHED_INDEX),
        }])
        .unwrap();
    assert_eq!(aggregate.as_str(), PUBLISHED_AGGREGATE);
}

#[test]
fn signed_resolver_seals_verified_bytes_without_exposing_native_paths() {
    let temp = TempDir::new().unwrap();
    let cache = FileArtifactCache::open(temp.path()).unwrap();
    let source = FixtureSource {
        response: FixtureResponse::Bytes(PUBLISHED_INDEX.to_vec()),
    };
    let resolver = SignedArtifactResolver::new(
        ManifestEventVerifier::pinned(),
        ArtifactLimits::default(),
        ArtifactSourcePolicy::manifest_https_only(8).unwrap(),
        &source,
        &cache,
    )
    .unwrap();
    let handle = resolver
        .resolve_json(PUBLISHED_EVENT, &coordinate())
        .unwrap();

    assert_eq!(
        handle.read_verified(INDEX_PATH, 4 * 1_024 * 1_024).unwrap(),
        PUBLISHED_INDEX
    );
    assert_eq!(handle.index().entries().len(), 1);
    assert_eq!(handle.index().entries().next().unwrap().path(), INDEX_PATH);
    assert!(cache.contains(handle.manifest().aggregate()));
}

#[test]
fn external_asset_fixture_resolves_every_pinned_path() {
    let files = [
        (INDEX_PATH, EXTERNAL_INDEX),
        ("/main.js", EXTERNAL_SCRIPT),
        ("/style.css", EXTERNAL_STYLE),
    ];
    let mut tags = vec![vec!["d".to_owned(), "fixture".to_owned()]];
    let mut source = BTreeMap::new();
    for (path, bytes) in files {
        let digest = Sha256Digest::of(bytes);
        tags.push(vec![
            "path".to_owned(),
            path.to_owned(),
            digest.as_str().to_owned(),
        ]);
        source.insert(digest.as_str().to_owned(), bytes.to_vec());
    }
    tags.push(vec![
        "x".to_owned(),
        EXTERNAL_AGGREGATE.to_owned(),
        "aggregate".to_owned(),
    ]);
    tags.push(vec![
        "server".to_owned(),
        "https://blossom.example/".to_owned(),
    ]);
    let (event, coordinate) = signed_named_manifest(tags);
    let temp = TempDir::new().unwrap();
    let cache = FileArtifactCache::open(temp.path()).unwrap();
    let source = DigestMapSource(source);
    let resolver = SignedArtifactResolver::new(
        ManifestEventVerifier::pinned(),
        ArtifactLimits::default(),
        ArtifactSourcePolicy::manifest_https_only(8).unwrap(),
        &source,
        &cache,
    )
    .unwrap();

    let handle = resolver.resolve_json(&event, &coordinate).unwrap();
    assert_eq!(handle.index().mode(), ArtifactMode::ExternalAssets);
    assert_eq!(handle.index().aggregate().as_str(), EXTERNAL_AGGREGATE);
    assert_eq!(
        handle.read_verified("/main.js", 1_024).unwrap(),
        EXTERNAL_SCRIPT
    );
    assert_eq!(
        handle.read_verified("/style.css", 1_024).unwrap(),
        EXTERNAL_STYLE
    );
}

#[test]
fn mutated_id_and_signature_are_distinct_refusals() {
    let mut wrong_id: Value = serde_json::from_slice(PUBLISHED_EVENT).unwrap();
    wrong_id["id"] = Value::String("0".repeat(64));
    assert!(matches!(
        ManifestEventVerifier::pinned()
            .verify_json(&serde_json::to_vec(&wrong_id).unwrap(), &coordinate()),
        Err(ManifestError::InvalidEventId)
    ));

    let mut wrong_signature: Value = serde_json::from_slice(PUBLISHED_EVENT).unwrap();
    wrong_signature["sig"] = Value::String("0".repeat(128));
    assert!(matches!(
        ManifestEventVerifier::pinned().verify_json(
            &serde_json::to_vec(&wrong_signature).unwrap(),
            &coordinate()
        ),
        Err(ManifestError::InvalidEventSignature)
    ));
}

#[test]
fn wrong_coordinate_and_duplicate_critical_tags_fail_closed() {
    let wrong_author = ManifestCoordinate::named(&"0".repeat(64), "good-morning").unwrap();
    assert!(matches!(
        ManifestEventVerifier::pinned().verify_json(PUBLISHED_EVENT, &wrong_author),
        Err(ManifestError::CoordinateAuthor)
    ));

    let path_hash = Sha256Digest::of(PUBLISHED_INDEX);
    let (duplicate, duplicate_coordinate) = signed_named_manifest(vec![
        vec!["d".to_owned(), "fixture".to_owned()],
        vec![
            "path".to_owned(),
            INDEX_PATH.to_owned(),
            path_hash.as_str().to_owned(),
        ],
        vec![
            "x".to_owned(),
            PUBLISHED_AGGREGATE.to_owned(),
            "aggregate".to_owned(),
        ],
        vec![
            "x".to_owned(),
            PUBLISHED_AGGREGATE.to_owned(),
            "aggregate".to_owned(),
        ],
    ]);
    assert!(matches!(
        ManifestEventVerifier::pinned().verify_json(&duplicate, &duplicate_coordinate),
        Err(ManifestError::DuplicateOrInvalidAggregate)
    ));

    let (wrong_aggregate, wrong_aggregate_coordinate) = signed_named_manifest(vec![
        vec!["d".to_owned(), "fixture".to_owned()],
        vec![
            "path".to_owned(),
            INDEX_PATH.to_owned(),
            path_hash.as_str().to_owned(),
        ],
        vec!["x".to_owned(), "0".repeat(64), "aggregate".to_owned()],
    ]);
    assert!(matches!(
        ManifestEventVerifier::pinned().verify_json(&wrong_aggregate, &wrong_aggregate_coordinate),
        Err(ManifestError::Artifact(
            ArtifactError::AggregateMismatch { .. }
        ))
    ));
}

#[test]
fn a_bare_redirect_response_is_refused_before_commit() {
    let temp = TempDir::new().unwrap();
    let cache = FileArtifactCache::open(temp.path()).unwrap();
    let source = FixtureSource {
        response: FixtureResponse::Redirect,
    };
    let resolver = SignedArtifactResolver::new(
        ManifestEventVerifier::pinned(),
        ArtifactLimits::default(),
        ArtifactSourcePolicy::manifest_https_only(8).unwrap(),
        &source,
        &cache,
    )
    .unwrap();
    assert!(matches!(
        resolver.resolve_json(PUBLISHED_EVENT, &coordinate()),
        Err(ManifestError::Artifact(ArtifactError::Source { .. }))
    ));
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn a_response_reporting_a_different_source_url_commits_once_its_hash_matches() {
    // Provenance is not re-pinned to the original candidate list here:
    // the blob source already vetted whatever URL it fetched from
    // (see `SafeManifestBlobSource`), and every file is hash-verified
    // against the manifest-pinned digest below regardless of origin.
    let temp = TempDir::new().unwrap();
    let cache = FileArtifactCache::open(temp.path()).unwrap();
    let source = FixtureSource {
        response: FixtureResponse::WrongSource(PUBLISHED_INDEX.to_vec()),
    };
    let resolver = SignedArtifactResolver::new(
        ManifestEventVerifier::pinned(),
        ArtifactLimits::default(),
        ArtifactSourcePolicy::manifest_https_only(8).unwrap(),
        &source,
        &cache,
    )
    .unwrap();
    let handle = resolver
        .resolve_json(PUBLISHED_EVENT, &coordinate())
        .unwrap();
    assert_eq!(
        handle.read_verified(INDEX_PATH, 4 * 1_024 * 1_024).unwrap(),
        PUBLISHED_INDEX
    );
}

#[test]
fn event_and_source_limits_refuse_without_work() {
    let verifier = ManifestEventVerifier::new(ManifestEventLimits {
        maximum_event_bytes: PUBLISHED_EVENT.len() - 1,
        ..ManifestEventLimits::default()
    })
    .unwrap();
    assert!(matches!(
        verifier.verify_json(PUBLISHED_EVENT, &coordinate()),
        Err(ManifestError::EventTooLarge { .. })
    ));

    let verified = ManifestEventVerifier::pinned()
        .verify_json(PUBLISHED_EVENT, &coordinate())
        .unwrap();
    let policy = ArtifactSourcePolicy::new(
        false,
        false,
        std::iter::empty::<&str>(),
        std::iter::empty::<&str>(),
        1,
    )
    .unwrap();
    assert!(matches!(
        policy.approved_servers(&verified),
        Err(ManifestError::NoApprovedBlobSource)
    ));
}

#[test]
fn declared_requirements_survive_a_bundled_script_that_precedes_the_head_metas() {
    // The shape published napplets actually ship: one large inline module,
    // then the declarations. A meta spelled inside the script is a string
    // literal, not an element, and must not be read as a declaration.
    let document = concat!(
        "<!doctype html>\n<html><head>\n",
        "<script type=\"module\">var s = '<meta name=\"napplet-requires\" ",
        "content=\"keys,upload\">';</script>\n",
        "<style>.a{content:\"<meta>\"}</style>\n",
        "<meta name=\"napplet-type\" content=\"nip29-groups\">\n",
        "<meta name=\"napplet-requires\" content=\"config,intent,relay\">\n",
        "</head><body></body></html>\n"
    );
    assert_eq!(
        embedded_requirements(document.as_bytes()),
        vec!["config", "intent", "relay"]
    );
}

#[test]
fn declared_requirements_keep_only_bounded_inventory_names() {
    let document = concat!(
        "<head><meta name='napplet-requires' content=' RELAY , relay,",
        "not-a-domain,,intent '></head>"
    );
    assert_eq!(
        embedded_requirements(document.as_bytes()),
        vec!["relay", "intent"]
    );
}

#[test]
fn declarations_below_the_head_are_not_read() {
    let document = "<head></head><body><meta name=\"napplet-requires\" content=\"relay\"></body>";
    assert!(embedded_requirements(document.as_bytes()).is_empty());
}

#[test]
fn commented_out_declarations_are_not_read() {
    let document = "<head><!-- <meta name=\"napplet-requires\" content=\"relay\"> --></head>";
    assert!(embedded_requirements(document.as_bytes()).is_empty());
}

#[test]
fn documents_without_a_declaration_yield_no_requirements() {
    assert!(embedded_requirements(PUBLISHED_INDEX).contains(&"identity"));
    assert!(embedded_requirements(b"<html><head></head></html>").is_empty());
    assert!(embedded_requirements(b"").is_empty());
}

#[test]
fn declared_config_schema_survives_html_attribute_escaping() {
    // Serializers escape the JSON a config schema carries, and the trusted
    // shell reads it back through a real parser. The runtime must agree.
    let document = concat!(
        "<head><meta name=\"napplet-config-schema\" content=\"{&quot;type&quot;:",
        "&quot;object&quot;,&quot;properties&quot;:{&quot;relays&quot;:{&quot;type&quot;:",
        "&quot;array&quot;,&quot;default&quot;:[&quot;wss://groups.0xchat.com&quot;]}}}\">",
        "</head>"
    );
    let schema: Value =
        serde_json::from_str(&embedded_config_schema(document.as_bytes()).unwrap()).unwrap();
    assert_eq!(
        schema["properties"]["relays"]["default"][0],
        Value::String("wss://groups.0xchat.com".to_owned())
    );
    assert_eq!(
        embedded_config_schema(b"<head><meta name=\"napplet-type\" content=\"x\"></head>"),
        None
    );
}
