use super::{
    ArtifactBlobRequest, ArtifactCandidate, ArtifactSubject, BTreeMap, HashSet, OwnerScope,
    PackageIdentity, ResolveArtifactsOptions, signed_artifact_fixture,
};
use base64::Engine as _;
use p256::pkcs8::EncodePublicKey as _;
use pnpm_pnpr_client::{
    ArtifactBuildPolicy, ArtifactStore, PublishArtifactRequest, TurborepoArtifactStore,
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
    let (request, public_key, blob) = signed_artifact_fixture();

    store.publish_artifact(&request).await.expect("publish");
    store.publish_artifact(&request).await.expect("a second publication is a no-op");

    let resolved = store
        .resolve_artifacts(lookup(&public_key, NEWER_GLIBC))
        .await
        .expect("resolve");
    let artifact =
        resolved.get("dependency-side-effects:v1:deps=abc").expect("the artifact resolves");
    let downloaded = store
        .download_artifact_blob(&ArtifactBlobRequest {
            owner: artifact.payload.owner.clone(),
            integrity: artifact.payload.manifest.added[0].integrity.clone(),
        })
        .await
        .expect("download");
    assert_eq!(downloaded, blob);

    let requests = cache.requests();
    dbg!(&requests);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.method == "PUT")
            .count(),
        1,
    );
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
    let (request, public_key, _) = signed_artifact_fixture();
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
    let (request, public_key, _) = signed_artifact_fixture();
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

    let (hash, body) = cache
        .artifacts()
        .into_iter()
        .next()
        .expect("one stored artifact");
    let mut altered: PublishArtifactRequest = serde_json::from_slice(&body).unwrap();
    let mut payload: serde_json::Value =
        serde_json::from_slice(&super::BASE64.decode(&altered.envelope.payload).unwrap()).unwrap();
    payload["builderId"] = "attacker".into();
    altered.envelope.payload = super::BASE64.encode(serde_json::to_vec(&payload).unwrap());
    cache.store(&hash, serde_json::to_vec(&altered).unwrap());
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
    let (request, public_key, blob) = workspace_task_fixture();
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
        .download_artifact_blob(&ArtifactBlobRequest {
            owner: artifact.payload.owner.clone(),
            integrity: artifact.payload.manifest.added[0].integrity.clone(),
        })
        .await
        .expect("download");
    assert_eq!(downloaded, blob);
}

fn workspace_task_fixture() -> (PublishArtifactRequest, Vec<u8>, Vec<u8>) {
    use p256::pkcs8::EncodePrivateKey as _;
    let blob = b"dist output".to_vec();
    let integrity =
        format!("sha512-{}", super::BASE64.encode(<super::Sha512 as sha2::Digest>::digest(&blob)));
    let payload = pnpm_pnpr_client::ArtifactPayload {
        kind: pnpm_pnpr_client::WORKSPACE_TASK_ARTIFACT_KIND.to_string(),
        subject: ArtifactSubject::workspace_task("packages/app", "build"),
        input_key: format!("{}abc", pnpm_pnpr_client::WORKSPACE_TASK_INPUT_KEY_PREFIX),
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
    (
        PublishArtifactRequest {
            key: payload.input_key,
            envelope,
            blobs: vec![pnpm_pnpr_client::ArtifactBlobUpload {
                integrity,
                data: super::BASE64.encode(&blob),
            }],
        },
        public_key,
        blob,
    )
}
