use super::{
    ArtifactBlobRequest, ArtifactCandidate, ArtifactSubject, BTreeMap, HashSet, OwnerScope,
    PackageIdentity, ResolveArtifactsOptions, SignedFixture, signed_artifact_fixture,
};
use base64::Engine as _;
use p256::pkcs8::EncodePublicKey as _;
use pnpm_pnpr_client::{
    ArtifactBlobSource, ArtifactBuildPolicy, ArtifactPublication, ArtifactStore, PnprClientError,
    TurborepoArtifactStore,
};
use pnpm_testing_utils::turborepo_cache::TurborepoCache;

const AUTHORIZATION: &str = "Bearer turborepo-token";
/// A consumer advertises every glibc floor it meets, newest first.
const NEWER_GLIBC: &[&str] =
    &["pnpm:v1:linux-x64-node22-glibc2.35", "pnpm:v1:linux-x64-node22-glibc2.17"];

fn store(cache: &TurborepoCache) -> ArtifactStore {
    ArtifactStore::Turborepo(
        TurborepoArtifactStore::new(
            cache.url(),
            Some("team_acme"),
            Some(AUTHORIZATION.to_string()),
        )
        .expect("a loopback store accepts credentials"),
    )
}

fn lookup(public_key: &[u8], supported_tags: &[&str]) -> ResolveArtifactsOptions {
    ResolveArtifactsOptions {
        candidates: vec![ArtifactCandidate {
            key: "dependency-side-effects:v1:deps=abc".to_string(),
            subject: ArtifactSubject::dependency_side_effects(
                PackageIdentity { name: "native-addon".to_string(), version: "1.0.0".to_string() },
                "sha512-source",
            ),
            owner: OwnerScope::organization("pnpr-client"),
        }],
        supported_tags: supported_tags
            .iter()
            .map(ToString::to_string)
            .collect(),
        trusted_keys: BTreeMap::from([("acme-2026".to_string(), public_key.to_vec())]),
        quarantined_envelope_digests: BTreeMap::new(),
        on_rejected_artifact: None,
        authorization: None,
        build_policy: ArtifactBuildPolicy {
            eligible_packages: HashSet::from(["native-addon".to_string()]),
            allowed_builds: HashSet::from(["native-addon".to_string()]),
            ignore_scripts: false,
        },
    }
}

/// A build published against an older glibc serves a consumer with a newer
/// one, because both fall in the same os-arch-Node scope the artifact is
/// stored under.
#[tokio::test]
async fn a_published_artifact_resolves_on_a_compatible_machine() {
    let cache = TurborepoCache::start();
    let store = store(&cache);
    let (request, public_key, blob, _blobs) = signed_artifact_fixture();

    store.publish_artifact(&request).await.expect("publish");

    let resolved = store
        .resolve_artifacts(lookup(&public_key, NEWER_GLIBC))
        .await
        .expect("resolve");
    let artifact =
        resolved.get("dependency-side-effects:v1:deps=abc").expect("the artifact resolves");
    let downloaded = store
        .download_artifact_blob(
            &ArtifactBlobRequest {
                owner: artifact.payload.owner.clone(),
                integrity: artifact.payload.manifest.added[0].integrity.clone(),
            },
            artifact.payload.manifest.added[0].size,
        )
        .await
        .expect("download");
    assert_eq!(downloaded, blob);

    let requests = cache.requests();
    dbg!(&requests);
    assert!(
        requests
            .iter()
            .all(|request| {
                request.query.as_deref() == Some("teamId=team_acme")
                    && request.authorization.as_deref() == Some(AUTHORIZATION)
            }),
    );
}

#[tokio::test]
async fn an_artifact_for_another_platform_is_a_miss() {
    let cache = TurborepoCache::start();
    let store = store(&cache);
    let (request, public_key, _, _blobs) = signed_artifact_fixture();
    store.publish_artifact(&request).await.expect("publish");

    let resolved = store
        .resolve_artifacts(lookup(
            &public_key,
            &["pnpm:v1:linux-arm64-node22-glibc2.35", "pnpm:v1:linux-arm64-node22-glibc2.17"],
        ))
        .await
        .expect("resolve");
    assert!(resolved.is_empty());
}

/// The server is not trusted for what it serves: an artifact signed by a key
/// the consumer does not trust, or altered after signing, is a miss.
#[tokio::test]
async fn an_untrusted_or_altered_artifact_is_a_miss() {
    let cache = TurborepoCache::start();
    let store = store(&cache);
    let (request, public_key, _, _blobs) = signed_artifact_fixture();
    store.publish_artifact(&request).await.expect("publish");

    let other_key = super::SigningKey::from_slice(&[9; 32]).expect("another key");
    let other_public_key = p256::PublicKey::from(other_key.verifying_key())
        .to_public_key_der()
        .expect("encode the other key")
        .as_bytes()
        .to_vec();
    let untrusted = store
        .resolve_artifacts(lookup(&other_public_key, NEWER_GLIBC))
        .await
        .expect("resolve");
    assert!(untrusted.is_empty());

    let envelope_hash = last_put(&cache);
    let mut altered: pnpm_pnpr_client::SignedArtifactEnvelope =
        serde_json::from_slice(&cache.artifacts()[&envelope_hash]).unwrap();
    let mut payload: serde_json::Value =
        serde_json::from_slice(&super::BASE64.decode(&altered.payload).unwrap()).unwrap();
    payload["builderId"] = "attacker".into();
    altered.payload = super::BASE64.encode(serde_json::to_vec(&payload).unwrap());
    cache.store(&envelope_hash, serde_json::to_vec(&altered).unwrap());
    let tampered = store
        .resolve_artifacts(lookup(&public_key, NEWER_GLIBC))
        .await
        .expect("resolve");
    assert!(tampered.is_empty());
}

#[test]
fn credentials_are_refused_for_a_cleartext_server() {
    let error = TurborepoArtifactStore::new(
        "http://cache.example.com",
        None,
        Some(AUTHORIZATION.to_string()),
    )
    .err()
    .expect("credentials over cleartext are refused");
    assert!(error.to_string().contains("HTTPS or a loopback"), "{error}");
}

/// A workspace task's output takes the same path through a pnpr server as a
/// dependency build does.
#[tokio::test]
async fn a_workspace_task_artifact_round_trips_through_pnpr() {
    let (pnpr_url, authorization, _storage) = super::start_pnpr_artifacts().await;
    let store = ArtifactStore::Pnpr {
        client: pnpm_pnpr_client::PnprClient::new(pnpr_url),
        authorization: Some(authorization),
    };
    let (request, public_key, blob, _blobs) = workspace_task_fixture();
    store.handshake().await.expect("handshake");
    store.publish_artifact(&request).await.expect("publish");

    let resolved = store
        .resolve_artifacts(ResolveArtifactsOptions {
            candidates: vec![ArtifactCandidate {
                key: request.key.clone(),
                subject: ArtifactSubject::workspace_task("packages/app", "build"),
                owner: OwnerScope::organization("pnpr-client"),
            }],
            supported_tags: Vec::new(),
            trusted_keys: BTreeMap::from([("acme-2026".to_string(), public_key)]),
            quarantined_envelope_digests: BTreeMap::new(),
            on_rejected_artifact: None,
            authorization: None,
            build_policy: ArtifactBuildPolicy {
                eligible_packages: HashSet::new(),
                allowed_builds: HashSet::new(),
                ignore_scripts: false,
            },
        })
        .await
        .expect("resolve");
    let artifact = resolved.get(&request.key).expect("the task artifact resolves");
    let downloaded = store
        .download_artifact_blob(
            &ArtifactBlobRequest {
                owner: artifact.payload.owner.clone(),
                integrity: artifact.payload.manifest.added[0].integrity.clone(),
            },
            artifact.payload.manifest.added[0].size,
        )
        .await
        .expect("download");
    assert_eq!(downloaded, blob);
}

fn workspace_task_fixture() -> SignedFixture {
    workspace_task_fixture_with(b"dist output".to_vec())
}

/// A signed task result whose one output file holds `blob`.
fn workspace_task_fixture_with(blob: Vec<u8>) -> SignedFixture {
    use p256::pkcs8::EncodePrivateKey as _;
    let integrity =
        format!("sha512-{}", super::BASE64.encode(<super::Sha512 as sha2::Digest>::digest(&blob)));
    let payload = pnpm_pnpr_client::ArtifactPayload {
        kind: pnpm_pnpr_client::WORKSPACE_TASK_ARTIFACT_KIND.to_string(),
        subject: ArtifactSubject::workspace_task("packages/app", "build"),
        // Named by its contents, since a pnpr server keeps one artifact per key.
        input_key: format!(
            "{}{}",
            pnpm_pnpr_client::WORKSPACE_TASK_INPUT_KEY_PREFIX,
            pnpm_pnpr_client::blob_id(&integrity).unwrap(),
        ),
        owner: OwnerScope::organization("pnpr-client"),
        builder_id: "ci/main/1".to_string(),
        builder_profile: pnpm_pnpr_client::BuilderProfile {
            image_digest: None,
            architecture_baseline: "x86-64-v2".to_string(),
            environment: BTreeMap::new(),
        },
        compatibility: pnpm_pnpr_client::CompatibilityConstraints::Universal,
        manifest: pnpm_pnpr_client::ArtifactManifest {
            added: vec![pnpm_pnpr_client::ArtifactFile {
                path: "outputs/dist/index.js".to_string(),
                integrity: integrity.clone(),
                mode: 0o644,
                size: blob.len() as u64,
            }],
            deleted: Vec::new(),
        },
    };
    let secret = p256::SecretKey::from_slice(&[7; 32]).expect("fixture private key");
    let envelope = pnpm_pnpr_client::SignedArtifactEnvelope::sign(
        &payload,
        "acme-2026",
        secret
            .to_pkcs8_der()
            .expect("encode private key")
            .as_bytes(),
    )
    .expect("sign");
    let public_key = secret
        .public_key()
        .to_public_key_der()
        .expect("encode public key")
        .as_bytes()
        .to_vec();
    let blobs = tempfile::TempDir::new().expect("create a blob directory");
    let path = blobs.path().join("index.js");
    std::fs::write(&path, &blob).expect("write the blob file");
    let publication = ArtifactPublication {
        key: payload.input_key,
        envelope,
        blobs: vec![ArtifactBlobSource { integrity, size: blob.len() as u64, path }],
    };
    (publication, public_key, blob, blobs)
}

/// What a consumer that trusts `public_key` asks for the task result `key`.
fn task_lookup(key: &str, public_key: Vec<u8>) -> ResolveArtifactsOptions {
    ResolveArtifactsOptions {
        candidates: vec![ArtifactCandidate {
            key: key.to_string(),
            subject: ArtifactSubject::workspace_task("packages/app", "build"),
            owner: OwnerScope::organization("pnpr-client"),
        }],
        supported_tags: Vec::new(),
        trusted_keys: BTreeMap::from([("acme-2026".to_string(), public_key)]),
        quarantined_envelope_digests: BTreeMap::new(),
        on_rejected_artifact: None,
        authorization: None,
        build_policy: ArtifactBuildPolicy {
            eligible_packages: HashSet::new(),
            allowed_builds: HashSet::new(),
            ignore_scripts: false,
        },
    }
}

/// A task result's blobs are uploaded from their files and downloaded into
/// files, through either transport: one large enough that a pnpr server
/// writes it in parts, and an empty one.
#[tokio::test]
async fn a_large_task_result_streams_through_either_transport() {
    let large: Vec<u8> = (0..9 * 1024 * 1024_u32)
        .map(|index| (index % 251) as u8)
        .collect();
    let cache = TurborepoCache::start();
    let (pnpr_url, authorization, _storage) = super::start_pnpr_artifacts().await;
    let stores = [
        store(&cache),
        ArtifactStore::Pnpr {
            client: pnpm_pnpr_client::PnprClient::new(pnpr_url),
            authorization: Some(authorization),
        },
    ];
    let empty = Vec::new();
    for (store, contents) in stores
        .iter()
        .flat_map(|store| [(store, &large), (store, &empty)])
    {
        let (publication, public_key, blob, _blobs) = workspace_task_fixture_with(contents.clone());
        store.publish_artifact(&publication).await.expect("publish");
        let resolved = store
            .resolve_artifacts(task_lookup(&publication.key, public_key))
            .await
            .expect("resolve");
        let file = &resolved[&publication.key].payload.manifest.added[0];
        let destination = tempfile::TempDir::new().unwrap();
        let path = destination.path().join("index.js");
        let request = ArtifactBlobRequest {
            owner: OwnerScope::organization("pnpr-client"),
            integrity: file.integrity.clone(),
        };
        store.download_artifact_blob_to(&request, file.size, &path).await.expect("download");
        assert!(std::fs::read(&path).unwrap() == blob, "the downloaded file holds the blob");
    }
}

/// A file that changed after its artifact was signed fails the upload, so a
/// server that stores whatever it is sent never holds a blob that cannot be
/// restored.
#[tokio::test]
async fn a_file_changed_after_signing_is_not_stored() {
    let cache = TurborepoCache::start();
    let store = store(&cache);
    let (publication, _, blob, _blobs) = workspace_task_fixture();
    let altered = vec![b'x'; blob.len()];
    std::fs::write(&publication.blobs[0].path, altered).unwrap();
    let error = store.publish_artifact(&publication).await.expect_err("refused");
    eprintln!("{error}");
    assert!(error.to_string().contains("changed after its artifact was signed"));
    assert!(cache.artifacts().is_empty(), "nothing was stored");
}

/// A publication whose blob files do not match its signed manifest is refused
/// before anything is sent.
#[tokio::test]
async fn a_publication_that_does_not_match_its_manifest_is_refused() {
    let cache = TurborepoCache::start();
    let store = store(&cache);
    let (publication, _, _, _blobs) = workspace_task_fixture();
    let mut missing = publication.clone();
    missing.blobs.clear();
    let mut resized = publication.clone();
    resized.blobs[0].size += 1;
    let mut doubled = publication.clone();
    doubled.blobs.push(publication.blobs[0].clone());
    for invalid in [missing, resized, doubled] {
        store.publish_artifact(&invalid).await.expect_err("refused");
    }
    assert!(cache.requests().is_empty(), "nothing reached the server");
}

/// The organizations a pnpr server declares decide who reads and who
/// publishes, whatever the accounts are called.
#[tokio::test]
async fn a_pnpr_organization_separates_readers_from_publishers() {
    let (pnpr_url, publisher, _storage) = super::start_pnpr_artifacts().await;
    let reader = format!("Bearer {}", super::register_token(&pnpr_url, "reader").await);
    let stranger = format!("Bearer {}", super::register_token(&pnpr_url, "stranger").await);
    let store = |authorization: &str| ArtifactStore::Pnpr {
        client: pnpm_pnpr_client::PnprClient::new(&pnpr_url),
        authorization: Some(authorization.to_string()),
    };
    let (request, public_key, _, _blobs) = workspace_task_fixture();
    store(&publisher).publish_artifact(&request).await.expect("the publisher publishes");
    assert!(store(&reader).publish_artifact(&request).await.is_err(), "a reader must not publish");

    let lookup = || ResolveArtifactsOptions {
        candidates: vec![ArtifactCandidate {
            key: request.key.clone(),
            subject: ArtifactSubject::workspace_task("packages/app", "build"),
            owner: OwnerScope::organization("pnpr-client"),
        }],
        supported_tags: Vec::new(),
        trusted_keys: BTreeMap::from([("acme-2026".to_string(), public_key.clone())]),
        quarantined_envelope_digests: BTreeMap::new(),
        on_rejected_artifact: None,
        authorization: None,
        build_policy: ArtifactBuildPolicy {
            eligible_packages: HashSet::new(),
            allowed_builds: HashSet::new(),
            ignore_scripts: false,
        },
    };
    let read = store(&reader)
        .resolve_artifacts(lookup())
        .await
        .expect("resolve");
    assert!(read.contains_key(&request.key), "a reader restores the organization's artifact");
    let hidden = store(&stranger)
        .resolve_artifacts(lookup())
        .await
        .expect("resolve");
    assert!(hidden.is_empty(), "an account outside the organization sees nothing");
}

/// A slot filled by someone without the signing key is not a slot lost: the
/// next publication replaces what is there.
#[tokio::test]
async fn a_publication_replaces_an_unverifiable_object() {
    let cache = TurborepoCache::start();
    let store = store(&cache);
    let (request, public_key, _, _blobs) = signed_artifact_fixture();
    store.publish_artifact(&request).await.expect("publish");
    let hashes: Vec<String> = cache.artifacts().into_keys().collect();
    for hash in &hashes {
        cache.store(hash, b"junk".to_vec());
    }
    assert!(
        store
            .resolve_artifacts(lookup(&public_key, NEWER_GLIBC))
            .await
            .expect("resolve")
            .is_empty(),
    );

    store.publish_artifact(&request).await.expect("publish again");
    let resolved = store
        .resolve_artifacts(lookup(&public_key, NEWER_GLIBC))
        .await
        .expect("resolve");
    assert!(resolved.contains_key(&request.key));
}

/// One artifact the server cannot serve is a miss for that artifact alone.
/// A server that refuses the credentials, or answers no lookup, fails the
/// whole lookup.
#[tokio::test]
async fn a_failed_lookup_is_a_miss_for_that_candidate_alone() {
    let cache = TurborepoCache::start();
    let store = store(&cache);
    let (dependency, public_key, _, _blobs) = signed_artifact_fixture();
    store.publish_artifact(&dependency).await.expect("publish the dependency build");
    let dependency_hash = last_put(&cache);
    let (task, task_public_key, _, _blobs) = workspace_task_fixture();
    store.publish_artifact(&task).await.expect("publish the task result");
    let task_hash = last_put(&cache);
    let both = || {
        let mut options = lookup(&public_key, NEWER_GLIBC);
        options.trusted_keys.insert("acme-2026".to_string(), task_public_key.clone());
        options.candidates.push(ArtifactCandidate {
            key: task.key.clone(),
            subject: ArtifactSubject::workspace_task("packages/app", "build"),
            owner: OwnerScope::organization("pnpr-client"),
        });
        options
    };

    cache.fail(&dependency_hash, 500);
    let resolved = store
        .resolve_artifacts(both())
        .await
        .expect("resolve");
    assert!(resolved.contains_key(&task.key), "the task result still resolves");
    assert!(!resolved.contains_key(&dependency.key));

    cache.fail(&task_hash, 401);
    let Err(error) = store.resolve_artifacts(both()).await else {
        panic!("refused credentials fail the lookup");
    };
    eprintln!("{error}");
    assert!(matches!(&error, PnprClientError::RemoteCache(message) if message.starts_with("GET ")));
}

#[tokio::test]
async fn a_lookup_the_server_answers_no_part_of_fails() {
    let cache = TurborepoCache::start();
    let store = store(&cache);
    let (task, public_key, _, _blobs) = workspace_task_fixture();
    store.publish_artifact(&task).await.expect("publish");
    cache.fail(&last_put(&cache), 503);

    let result = store.resolve_artifacts(ResolveArtifactsOptions {
        candidates: vec![ArtifactCandidate {
            key: task.key.clone(),
            subject: ArtifactSubject::workspace_task("packages/app", "build"),
            owner: OwnerScope::organization("pnpr-client"),
        }],
        supported_tags: Vec::new(),
        ..lookup(&public_key, NEWER_GLIBC)
    })
    .await;
    assert!(result.is_err());
}

/// A blob is checked against its integrity, so one the server altered is
/// refused rather than restored.
#[tokio::test]
async fn an_altered_blob_is_refused() {
    let cache = TurborepoCache::start();
    let store = store(&cache);
    let (request, public_key, blob, _blobs) = signed_artifact_fixture();
    store.publish_artifact(&request).await.expect("publish");
    let blob_hash = first_put(&cache);
    let resolved = store
        .resolve_artifacts(lookup(&public_key, NEWER_GLIBC))
        .await
        .expect("resolve");
    let artifact = &resolved[&request.key];
    let blob_request = ArtifactBlobRequest {
        owner: artifact.payload.owner.clone(),
        integrity: artifact.payload.manifest.added[0].integrity.clone(),
    };
    let size = blob.len() as u64;
    assert_eq!(store.download_artifact_blob(&blob_request, size).await.expect("download"), blob);

    cache.store(&blob_hash, b"altered".to_vec());
    let error = store.download_artifact_blob(&blob_request, size).await.expect_err("altered blob");
    eprintln!("{error}");
    assert!(matches!(error, PnprClientError::Protocol(_)), "a content fault is quarantinable");
    let destination = tempfile::TempDir::new().unwrap();
    let path = destination.path().join("addon.node");
    store.download_artifact_blob_to(&blob_request, size, &path).await.expect_err("altered blob");
    assert!(!path.exists(), "a refused blob leaves no file behind");
}

/// The object a publication stored first: its first blob.
fn first_put(cache: &TurborepoCache) -> String {
    puts(cache)
        .into_iter()
        .next()
        .expect("a publication")
}

/// The object a publication stored last: its envelope in its last scope.
fn last_put(cache: &TurborepoCache) -> String {
    puts(cache).pop().expect("a publication")
}

fn puts(cache: &TurborepoCache) -> Vec<String> {
    cache
        .requests()
        .into_iter()
        .filter(|request| request.method == "PUT")
        .map(|request| request.hash)
        .collect()
}
