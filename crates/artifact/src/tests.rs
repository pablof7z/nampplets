use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

use tempfile::TempDir;

use super::*;
use crate::file_cache::{CACHE_BLOBS_DIRECTORY, CACHE_INDEX_FILE};

fn manifest_for(
    files: &[(String, Vec<u8>)],
    aggregate_policy: &dyn AggregateVerifier,
) -> ArtifactManifest {
    let verified = files
        .iter()
        .map(|(path, bytes)| VerifiedFile {
            path: Arc::from(path.as_str()),
            digest: Sha256Digest::of(bytes),
            bytes: Arc::from(bytes.clone()),
        })
        .collect::<Vec<_>>();
    ArtifactManifest {
        aggregate: aggregate_policy.compute(&verified).unwrap(),
        paths: verified
            .into_iter()
            .map(|file| ArtifactPath {
                path: file.path.to_string(),
                sha256: file.digest,
            })
            .collect(),
    }
}

#[test]
fn path_hash_mismatch_never_commits_returned_bytes() {
    let expected = vec![(INDEX_PATH.to_owned(), b"<h1>safe</h1>".to_vec())];
    let source = MemoryBlobSource::new([(INDEX_PATH.to_owned(), b"<h1>evil</h1>".to_vec())]);
    let policy = FramedSha256Aggregate;
    let manifest = manifest_for(&expected, &policy);
    let temp = TempDir::new().unwrap();
    let cache = FileArtifactCache::open(temp.path()).unwrap();
    let resolver =
        ArtifactResolver::new(ArtifactLimits::default(), &source, &policy, &cache).unwrap();

    assert!(matches!(
        resolver.resolve(&manifest),
        Err(ArtifactError::PathHashMismatch { .. })
    ));
    assert!(!cache.contains(&manifest.aggregate));
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn aggregate_mismatch_never_commits_individually_valid_files() {
    let files = vec![(INDEX_PATH.to_owned(), b"<h1>safe</h1>".to_vec())];
    let source = MemoryBlobSource::new(files.clone());
    let policy = FramedSha256Aggregate;
    let mut manifest = manifest_for(&files, &policy);
    manifest.aggregate = Sha256Digest::parse("f".repeat(64)).unwrap();
    let temp = TempDir::new().unwrap();
    let cache = FileArtifactCache::open(temp.path()).unwrap();
    let resolver =
        ArtifactResolver::new(ArtifactLimits::default(), &source, &policy, &cache).unwrap();

    assert!(matches!(
        resolver.resolve(&manifest),
        Err(ArtifactError::AggregateMismatch { .. })
    ));
    assert!(!cache.contains(&manifest.aggregate));
}

#[test]
fn valid_artifact_commits_atomically() {
    let files = vec![
        (
            INDEX_PATH.to_owned(),
            b"<script src=\"/app.js\"></script>".to_vec(),
        ),
        ("/app.js".to_owned(), b"window.app = true".to_vec()),
    ];
    let source = MemoryBlobSource::new(files.clone());
    let policy = FramedSha256Aggregate;
    let manifest = manifest_for(&files, &policy);
    let temp = TempDir::new().unwrap();
    let cache = FileArtifactCache::open(temp.path()).unwrap();
    let resolver =
        ArtifactResolver::new(ArtifactLimits::default(), &source, &policy, &cache).unwrap();

    let cached = resolver.resolve(&manifest).unwrap();
    assert_eq!(cached.files, 2);
    assert_eq!(
        cached.read_verified("/app.js", 1024).unwrap(),
        b"window.app = true"
    );
}

#[test]
fn prepopulated_or_tampered_cache_never_becomes_executable() {
    let files = vec![(INDEX_PATH.to_owned(), b"<h1>safe</h1>".to_vec())];
    let source = MemoryBlobSource::new(files.clone());
    let policy = FramedSha256Aggregate;
    let manifest = manifest_for(&files, &policy);
    let temp = TempDir::new().unwrap();
    let cache = FileArtifactCache::open(temp.path()).unwrap();
    let destination = temp.path().join(manifest.aggregate.as_str());
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join(CACHE_INDEX_FILE), b"{}").unwrap();
    let resolver =
        ArtifactResolver::new(ArtifactLimits::default(), &source, &policy, &cache).unwrap();

    assert!(resolver.resolve(&manifest).is_err());
    assert!(!cache.contains(&manifest.aggregate));

    fs::remove_dir_all(&destination).unwrap();
    let cached = resolver.resolve(&manifest).unwrap();
    let index_entry = cached.index.get(INDEX_PATH).unwrap();
    let blob_path = cached
        .root
        .join(CACHE_BLOBS_DIRECTORY)
        .join(index_entry.digest.as_str());
    let mut permissions = fs::metadata(&blob_path).unwrap().permissions();
    #[cfg(unix)]
    permissions.set_mode(0o600);
    #[cfg(not(unix))]
    permissions.set_readonly(false);
    fs::set_permissions(&blob_path, permissions).unwrap();
    fs::write(&blob_path, b"<h1>evil</h1>").unwrap();

    assert!(matches!(
        cached.read_verified(INDEX_PATH, 1024),
        Err(ArtifactError::CorruptCache { .. })
    ));
    assert!(!cache.contains(&manifest.aggregate));
    assert!(matches!(
        resolver.resolve(&manifest),
        Err(ArtifactError::CorruptCache { .. })
    ));
}

#[test]
fn logical_paths_never_alias_native_paths() {
    let files = vec![
        (INDEX_PATH.to_owned(), b"index".to_vec()),
        ("/A.js".to_owned(), b"upper".to_vec()),
        ("/a.js".to_owned(), b"lower".to_vec()),
        ("/e\u{301}.js".to_owned(), b"decomposed".to_vec()),
        ("/é.js".to_owned(), b"composed".to_vec()),
    ];
    let source = MemoryBlobSource::new(files.clone());
    let policy = FramedSha256Aggregate;
    let manifest = manifest_for(&files, &policy);
    let temp = TempDir::new().unwrap();
    let cache = FileArtifactCache::open(temp.path()).unwrap();
    let resolver =
        ArtifactResolver::new(ArtifactLimits::default(), &source, &policy, &cache).unwrap();

    let cached = resolver.resolve(&manifest).unwrap();
    assert_eq!(cached.read_verified("/A.js", 64).unwrap(), b"upper");
    assert_eq!(cached.read_verified("/a.js", 64).unwrap(), b"lower");
    assert_eq!(
        cached.read_verified("/e\u{301}.js", 64).unwrap(),
        b"decomposed"
    );
    assert_eq!(cached.read_verified("/é.js", 64).unwrap(), b"composed");
}

#[test]
fn deserialized_digest_still_requires_canonical_lowercase_sha256() {
    let manifest = format!(
        r#"{{"aggregate":"{}","paths":[{{"path":"/index.html","sha256":"{}"}}]}}"#,
        "f".repeat(64),
        "A".repeat(64)
    );
    assert!(serde_json::from_str::<ArtifactManifest>(&manifest).is_err());
}
