use super::{
    Arc, ArtifactBlobRequest, ArtifactCandidate, ArtifactSubject, BASE64, BTreeMap, Barrier,
    HashSet, OwnerScope, PackageIdentity, PnprClient, RegistryDeclaration, ResolveArtifactsOptions,
    Sha512, SigningKey, TestRegistry, deps, options, signed_artifact_fixture,
    signed_artifact_fixture_for_platform, start_pnpr, start_pnpr_artifacts, start_pnpr_inner,
};
use base64::Engine as _;
use p256::pkcs8::EncodePublicKey as _;
use sha2::Digest as _;

#[tokio::test]
async fn resolves_a_package() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

    let client = PnprClient::new(pnpr_url);

    let outcome = client
        .resolve(options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("install should succeed");

    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(
        packages.keys().any(|key| key.to_string().starts_with("@foo/no-deps@1.0.0")),
        "lockfile should contain @foo/no-deps@1.0.0, got: {:?}",
        packages.keys().map(ToString::to_string).collect::<Vec<_>>(),
    );

    assert!(outcome.stats.total_packages >= 1);
}

#[tokio::test]
async fn handshake_rejects_a_non_pnpr_server() {
    // A plain registry has no `/-/pnpr` route and 404s the handshake.
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/-/pnpr").with_status(404).create_async().await;

    let client = PnprClient::new(server.url());
    let err = client.handshake().await.expect_err("a non-pnpr server should be rejected");
    assert!(err.to_string().contains("not a pnpr server"), "got: {err}");
    mock.assert_async().await;
}

#[tokio::test]
async fn artifact_capability_is_disabled_by_default() {
    let (pnpr_url, _pnpr_auth, _storage) =
        start_pnpr_inner(None, Vec::new(), Vec::new(), false).await;
    let client = PnprClient::new(pnpr_url);
    client.handshake().await.expect("resolver capability");
    let error = client
        .handshake_artifacts()
        .await
        .expect_err("artifact capability must require an explicit opt-in");
    assert!(error.to_string().contains("does not advertise shared artifact protocol"));
}

#[tokio::test]
async fn artifact_handshake_is_independent_from_the_resolver_protocol() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/-/pnpr")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"pnpr":{"versions":[],"artifacts":[0]}}"#)
        .create_async()
        .await;

    PnprClient::new(server.url())
        .handshake_artifacts()
        .await
        .expect("artifact-only capability is supported");
    mock.assert_async().await;
}

#[tokio::test]
async fn publishes_resolves_and_verifies_an_organization_artifact() {
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr_artifacts().await;
    let client = PnprClient::new(pnpr_url);
    client.handshake_artifacts().await.expect("artifact capability");

    let (publish, public_key, expected_blob) = signed_artifact_fixture();
    client.publish_artifact(&publish, Some(&pnpr_auth)).await.expect("publish signed artifact");

    let candidate = ArtifactCandidate {
        key: publish.key.clone(),
        subject: ArtifactSubject::dependency_side_effects(
            PackageIdentity { name: "native-addon".to_string(), version: "1.0.0".to_string() },
            "sha512-source",
        ),
        owner: OwnerScope::organization("pnpr-client"),
    };
    let package_name = "native-addon".to_string();
    let untrusted_key = SigningKey::from_slice(&[8; 32]).expect("alternate fixture private key");
    let untrusted_public_key = p256::PublicKey::from(untrusted_key.verifying_key())
        .to_public_key_der()
        .expect("encode alternate fixture public key")
        .as_bytes()
        .to_vec();
    let untrusted = client
        .resolve_artifacts(ResolveArtifactsOptions {
            candidates: vec![candidate.clone()],
            supported_tags: vec!["pnpm:v1:linux-x64-node22-glibc2.17".to_string()],
            eligible_packages: HashSet::from([package_name.clone()]),
            allowed_builds: HashSet::from([package_name.clone()]),
            ignore_scripts: false,
            trusted_keys: BTreeMap::from([("acme-2026".to_string(), untrusted_public_key)]),
            quarantined_envelope_digests: BTreeMap::new(),
            on_rejected_artifact: None,
            authorization: Some(pnpr_auth.clone()),
        })
        .await
        .expect("an invalid signature is a cache miss");
    assert!(untrusted.is_empty());

    let mut mismatched_candidate = candidate.clone();
    let ArtifactSubject::DependencySideEffects { package, .. } = &mut mismatched_candidate.subject
    else {
        unreachable!()
    };
    package.version = "2.0.0".to_string();
    let mismatched = client
        .resolve_artifacts(ResolveArtifactsOptions {
            candidates: vec![mismatched_candidate],
            supported_tags: vec!["pnpm:v1:linux-x64-node22-glibc2.17".to_string()],
            eligible_packages: HashSet::from([package_name.clone()]),
            allowed_builds: HashSet::from([package_name.clone()]),
            ignore_scripts: false,
            trusted_keys: BTreeMap::from([("acme-2026".to_string(), public_key.clone())]),
            quarantined_envelope_digests: BTreeMap::new(),
            on_rejected_artifact: None,
            authorization: Some(pnpr_auth.clone()),
        })
        .await
        .expect("a mismatched signed package identity is a cache miss");
    assert!(mismatched.is_empty());

    let selected = client
        .resolve_artifacts(ResolveArtifactsOptions {
            candidates: vec![candidate.clone()],
            supported_tags: vec!["pnpm:v1:linux-x64-node22-glibc2.17".to_string()],
            eligible_packages: HashSet::from([package_name.clone()]),
            allowed_builds: HashSet::from([package_name.clone()]),
            ignore_scripts: false,
            trusted_keys: BTreeMap::from([("acme-2026".to_string(), public_key.clone())]),
            quarantined_envelope_digests: BTreeMap::new(),
            on_rejected_artifact: None,
            authorization: Some(pnpr_auth.clone()),
        })
        .await
        .expect("resolve signed artifact");
    let artifact = selected.get(&publish.key).expect("trusted compatible variant selected");
    assert_eq!(artifact.payload.owner, OwnerScope::organization("pnpr-client"));
    assert_eq!(artifact.envelope_digest.len(), 64);
    let quarantined_digest = artifact.envelope_digest.clone();
    let quarantined = client
        .resolve_artifacts(ResolveArtifactsOptions {
            candidates: vec![candidate.clone()],
            supported_tags: vec!["pnpm:v1:linux-x64-node22-glibc2.17".to_string()],
            eligible_packages: HashSet::from([package_name.clone()]),
            allowed_builds: HashSet::from([package_name.clone()]),
            ignore_scripts: false,
            trusted_keys: BTreeMap::from([("acme-2026".to_string(), public_key.clone())]),
            quarantined_envelope_digests: BTreeMap::from([(
                candidate.key.clone(),
                HashSet::from([quarantined_digest.clone()]),
            )]),
            on_rejected_artifact: None,
            authorization: Some(pnpr_auth.clone()),
        })
        .await
        .expect("a quarantined variant is a cache miss");
    assert!(quarantined.is_empty());
    let bytes = client
        .download_artifact_blob(
            &ArtifactBlobRequest {
                owner: candidate.owner,
                integrity: artifact.payload.manifest.added[0].integrity.clone(),
            },
            Some(&pnpr_auth),
        )
        .await
        .expect("download verified blob");
    assert_eq!(bytes, expected_blob);
}

#[tokio::test]
async fn artifact_lookup_preserves_script_eligibility_and_allow_build_policy() {
    let (publish, public_key, _) = signed_artifact_fixture();
    let candidate = ArtifactCandidate {
        key: publish.key,
        subject: ArtifactSubject::dependency_side_effects(
            PackageIdentity { name: "native-addon".to_string(), version: "1.0.0".to_string() },
            "sha512-source",
        ),
        owner: OwnerScope::organization("pnpr-client"),
    };
    let package_name = "native-addon".to_string();
    let supported_tags = vec!["pnpm:v1:linux-x64-node22-glibc2.17".to_string()];
    let trusted_keys = BTreeMap::from([("acme-2026".to_string(), public_key)]);
    let client = PnprClient::new("http://127.0.0.1:9/");

    for (ignore_scripts, eligible_packages, allowed_builds) in [
        (true, HashSet::from([package_name.clone()]), HashSet::from([package_name.clone()])),
        (false, HashSet::new(), HashSet::from([package_name.clone()])),
        (false, HashSet::from([package_name]), HashSet::new()),
    ] {
        let selected = client
            .resolve_artifacts(ResolveArtifactsOptions {
                candidates: vec![candidate.clone()],
                supported_tags: supported_tags.clone(),
                eligible_packages,
                allowed_builds,
                ignore_scripts,
                trusted_keys: trusted_keys.clone(),
                quarantined_envelope_digests: BTreeMap::new(),
                on_rejected_artifact: None,
                authorization: None,
            })
            .await
            .expect("a denied remote build must not contact pnpr");
        assert!(selected.is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
async fn concurrent_artifact_publications_apply_the_variant_limit_at_read_time() {
    const PUBLICATIONS: usize = 16;

    let (pnpr_url, pnpr_auth, _storage) = start_pnpr_artifacts().await;
    let (fixture, _, _) = signed_artifact_fixture_for_platform(0);
    let (payload, _) = fixture.envelope.decode_payload().expect("decode fixture payload");
    let candidate =
        ArtifactCandidate { key: fixture.key, subject: payload.subject, owner: payload.owner };
    let barrier = Arc::new(Barrier::new(PUBLICATIONS + 1));
    let mut publications = Vec::with_capacity(PUBLICATIONS);
    for index in 0..PUBLICATIONS {
        let barrier = Arc::clone(&barrier);
        let pnpr_url = pnpr_url.clone();
        let pnpr_auth = pnpr_auth.clone();
        let (publish, _, _) = signed_artifact_fixture_for_platform(index);
        publications.push(tokio::spawn(async move {
            barrier.wait().await;
            PnprClient::new(pnpr_url).publish_artifact(&publish, Some(&pnpr_auth)).await
        }));
    }
    barrier.wait().await;

    for publication in publications {
        publication.await.expect("publication task").expect("publish artifact variant");
    }

    let response = reqwest::Client::new()
        .post(format!("{pnpr_url}-/pnpr/v0/artifacts/resolve"))
        .header(reqwest::header::AUTHORIZATION, pnpr_auth)
        .json(&pnpm_pnpr_client::ResolveArtifactsRequest { candidates: vec![candidate] })
        .send()
        .await
        .expect("resolve artifacts response")
        .error_for_status()
        .expect("successful artifact resolve")
        .json::<serde_json::Value>()
        .await
        .expect("artifact resolve JSON");
    let variants =
        response["artifacts"][0]["variants"].as_array().expect("artifact variants array");
    assert_eq!(variants.len(), pnpm_shared_artifact_protocol::MAX_VARIANTS_PER_CANDIDATE);
}

#[tokio::test]
async fn artifact_blob_misses_and_errors_are_caller_scoped() {
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr_artifacts().await;
    let http = reqwest::Client::new();
    let missing_integrity = format!("sha512-{}", BASE64.encode(Sha512::digest(b"missing")));
    let missing = http
        .post(format!("{pnpr_url}-/pnpr/v0/artifacts/blob"))
        .header(reqwest::header::AUTHORIZATION, &pnpr_auth)
        .json(&ArtifactBlobRequest {
            owner: OwnerScope::organization("pnpr-client"),
            integrity: missing_integrity,
        })
        .send()
        .await
        .expect("missing blob response");
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
    assert_eq!(missing.headers()[reqwest::header::CACHE_CONTROL], "private, no-store");
    assert_eq!(missing.headers()[reqwest::header::VARY], "Authorization");

    let invalid = http
        .post(format!("{pnpr_url}-/pnpr/v0/artifacts/blob"))
        .header(reqwest::header::AUTHORIZATION, &pnpr_auth)
        .json(&serde_json::json!({
            "owner": { "type": "organization", "name": "pnpr-client" },
            "integrity": "not-an-integrity",
        }))
        .send()
        .await
        .expect("invalid blob response");
    assert_eq!(invalid.status(), reqwest::StatusCode::BAD_REQUEST);
    assert_eq!(invalid.headers()[reqwest::header::CACHE_CONTROL], "private, no-store");
    assert_eq!(invalid.headers()[reqwest::header::VARY], "Authorization");
}

#[tokio::test]
async fn organization_artifact_existence_is_not_exposed_to_another_owner() {
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr_artifacts().await;
    let client = PnprClient::new(pnpr_url);
    let (publish, public_key, _) = signed_artifact_fixture();
    client.publish_artifact(&publish, Some(&pnpr_auth)).await.expect("publish artifact");

    let selected = client
        .resolve_artifacts(ResolveArtifactsOptions {
            candidates: vec![ArtifactCandidate {
                key: publish.key,
                subject: ArtifactSubject::dependency_side_effects(
                    PackageIdentity {
                        name: "native-addon".to_string(),
                        version: "1.0.0".to_string(),
                    },
                    "sha512-source",
                ),
                owner: OwnerScope::organization("another-owner"),
            }],
            supported_tags: vec!["pnpm:v1:linux-x64-node22-glibc2.17".to_string()],
            eligible_packages: HashSet::from(["native-addon".to_string()]),
            allowed_builds: HashSet::from(["native-addon".to_string()]),
            ignore_scripts: false,
            trusted_keys: BTreeMap::from([("acme-2026".to_string(), public_key)]),
            quarantined_envelope_digests: BTreeMap::new(),
            on_rejected_artifact: None,
            authorization: Some(pnpr_auth),
        })
        .await
        .expect("cross-owner lookup is a masked miss");
    assert!(selected.is_empty());
}

/// The client describes its registries to the server the way its own
/// `registries` setting does, so a scope the client routes elsewhere resolves
/// from that registry and not from the request's default one.
///
/// This is also the contract test for the two ends of the protocol: the
/// server reads the declarations under the key the client writes them.
#[tokio::test]
async fn resolves_a_scope_from_the_registry_declared_for_it() {
    let registry = TestRegistry::start();
    // A default registry that is allowlisted but serves nothing: reaching it
    // for the scoped package is the failure this test is looking for.
    let dead_default = "http://127.0.0.1:9/";
    let (pnpr_url, pnpr_auth, _storage) =
        start_pnpr_inner(None, Vec::new(), vec![registry.url(), dead_default.to_string()], false)
            .await;

    let mut opts = options(dead_default, &pnpr_auth, deps([("@foo/no-deps", "1.0.0")]));
    opts.registries = BTreeMap::from([(
        registry.url(),
        RegistryDeclaration {
            scopes: Some(vec!["@foo".to_string()]),
            ..RegistryDeclaration::default()
        },
    )]);

    let outcome = PnprClient::new(pnpr_url).resolve(opts).await.expect("install should succeed");

    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(
        packages.keys().any(|key| key.to_string().starts_with("@foo/no-deps@1.0.0")),
        "the declared registry should have served the scope, got: {:?}",
        packages.keys().map(ToString::to_string).collect::<Vec<_>>(),
    );
}

/// A client describes its whole configuration, including scopes a given
/// resolve never reaches. Declaring a registry this pnpr does not serve is
/// therefore not an error by itself — only fetching from one is.
#[tokio::test]
async fn a_declared_registry_the_resolve_never_reaches_is_not_rejected() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

    let mut opts = options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")]));
    opts.registries = BTreeMap::from([(
        "http://169.254.169.254/".to_string(),
        RegistryDeclaration {
            scopes: Some(vec!["@never-resolved".to_string()]),
            ..RegistryDeclaration::default()
        },
    )]);

    let outcome = PnprClient::new(pnpr_url).resolve(opts).await.expect("install should succeed");
    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(packages.keys().any(|key| key.to_string().starts_with("@foo/no-deps@1.0.0")));
}

/// The SSRF boundary still holds where it matters: a scope the resolve *does*
/// reach is refused before the request leaves the server.
#[tokio::test]
async fn a_declared_registry_the_resolve_reaches_is_refused() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

    let mut opts = options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")]));
    opts.registries = BTreeMap::from([(
        "http://169.254.169.254/".to_string(),
        RegistryDeclaration {
            scopes: Some(vec!["@foo".to_string()]),
            ..RegistryDeclaration::default()
        },
    )]);

    let Err(error) = PnprClient::new(pnpr_url).resolve(opts).await else {
        panic!("an off-allowlist registry the resolve reaches must be refused")
    };
    let error = error.to_string();
    assert!(error.contains("is not allowed by this pnpr server"), "{error}");
    assert!(error.contains("169.254.169.254"), "the refused origin is named: {error}");
}
