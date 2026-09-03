use super::{
    EngineComponent, EngineToVerify, FailureCategory, NpmSigningKey, PackageSignature,
    PlatformBinaries, SelfUpdateError, SignatureFailure, build_client, collect_engine_components,
    find_signature_failure, native_engine_wrapper, plain_version, signature_validates_against,
    verify_one,
};
use base64::Engine as _;
use p256::ecdsa::SigningKey;
use pnpm_config::Config;
use pnpm_lockfile::{EnvLockfile, SnapshotDepRef, SpecifierAndResolution};
use pnpm_network::RetryOpts;
use std::{collections::BTreeMap, time::Duration};

fn signing_key() -> SigningKey {
    SigningKey::from_slice(&[0x42; 32]).expect("valid P-256 scalar")
}

fn public_key_b64(key: &SigningKey) -> String {
    use p256::pkcs8::EncodePublicKey;
    let der = key.verifying_key().to_public_key_der().expect("encode SPKI");
    base64::engine::general_purpose::STANDARD.encode(der.as_bytes())
}

fn sign_b64(key: &SigningKey, message: &str) -> String {
    use p256::ecdsa::{Signature, signature::Signer};
    let signature: Signature = key.sign(message.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(signature.to_der().as_bytes())
}

fn component() -> EngineComponent {
    EngineComponent {
        name: "pnpm".to_string(),
        registry: "https://registry.example.com/".to_string(),
        version: "12.0.0".to_string(),
        integrity: "sha512-deadbeef".to_string(),
    }
}

fn signed_message(component: &EngineComponent) -> String {
    format!("{}@{}:{}", component.name, component.version, component.integrity)
}

#[test]
fn verify_one_accepts_only_a_genuine_signature() {
    let key = signing_key();
    let pub_b64 = public_key_b64(&key);
    let message = "pnpm@12.0.0:sha512-deadbeef";
    let sig = sign_b64(&key, message);

    assert!(verify_one(&pub_b64, message, &sig), "a genuine signature validates");
    // A signature over different bytes must not validate the message.
    assert!(!verify_one(&pub_b64, "pnpm@12.0.1:sha512-deadbeef", &sig));
    // Malformed key / signature material is a non-match, not a panic.
    assert!(!verify_one("not-base64!!", message, &sig));
    assert!(!verify_one(&pub_b64, message, "not-base64!!"));
}

#[test]
fn signature_validates_accepts_a_trusted_unexpired_key() {
    let key = signing_key();
    let pub_b64 = public_key_b64(&key);
    let component = component();
    let keys = [NpmSigningKey { keyid: "SHA256:test", key: &pub_b64, expires: None }];
    let signatures = [PackageSignature {
        keyid: "SHA256:test".to_string(),
        sig: sign_b64(&key, &signed_message(&component)),
    }];
    assert!(signature_validates_against(&component, &signatures, None, &keys));
}

#[test]
fn signature_validates_rejects_an_expired_key() {
    let key = signing_key();
    let pub_b64 = public_key_b64(&key);
    let component = component();
    let keys = [NpmSigningKey {
        keyid: "SHA256:test",
        key: &pub_b64,
        expires: Some("2000-01-01T00:00:00.000Z"),
    }];
    let signatures = [PackageSignature {
        keyid: "SHA256:test".to_string(),
        sig: sign_b64(&key, &signed_message(&component)),
    }];
    // Published after the key expired, so even a valid signature is rejected.
    assert!(!signature_validates_against(
        &component,
        &signatures,
        Some("2020-01-01T00:00:00.000Z"),
        &keys,
    ));
}

#[test]
fn signature_validates_rejects_unknown_keyid_and_empty_signatures() {
    let key = signing_key();
    let pub_b64 = public_key_b64(&key);
    let component = component();
    let keys = [NpmSigningKey { keyid: "SHA256:test", key: &pub_b64, expires: None }];

    let unknown = [PackageSignature {
        keyid: "SHA256:unknown".to_string(),
        sig: sign_b64(&key, &signed_message(&component)),
    }];
    assert!(!signature_validates_against(&component, &unknown, None, &keys));
    assert!(!signature_validates_against(&component, &[], None, &keys));
}

#[test]
fn plain_version_reads_only_plain_references() {
    let plain: SnapshotDepRef = "1.2.3".parse().expect("parse plain ref");
    assert_eq!(plain_version(&plain), Some("1.2.3".to_string()));

    let alias: SnapshotDepRef = "foo@1.2.3".parse().expect("parse alias ref");
    assert_eq!(plain_version(&alias), None);

    let link = SnapshotDepRef::Link("packages/x".to_string());
    assert_eq!(plain_version(&link), None);
}

fn pm_deps(entries: &[(&str, &str)]) -> BTreeMap<String, SpecifierAndResolution> {
    entries
        .iter()
        .map(|(name, version)| {
            (
                (*name).to_string(),
                SpecifierAndResolution {
                    specifier: (*version).to_string(),
                    version: (*version).to_string(),
                },
            )
        })
        .collect()
}

#[test]
fn native_engine_wrapper_follows_the_package_that_ships_the_binary() {
    assert_eq!(
        native_engine_wrapper(&pm_deps(&[("pnpm", "11.0.0"), ("@pnpm/exe", "11.0.0")])),
        Some(("@pnpm/exe", "11.0.0")),
    );
    assert_eq!(native_engine_wrapper(&pm_deps(&[("pnpm", "12.0.0")])), Some(("pnpm", "12.0.0")));
    assert_eq!(native_engine_wrapper(&pm_deps(&[("pnpm", "6.16.0")])), None);
    assert_eq!(native_engine_wrapper(&pm_deps(&[])), None);
}

/// Verification covers every package the engine pins, so a lockfile that
/// records only some of them is unverifiable rather than half-verified.
#[test]
fn an_incomplete_engine_package_set_is_unverifiable() {
    let mut env = EnvLockfile::create();
    env.importers
        .entry(EnvLockfile::ROOT_IMPORTER_KEY.to_string())
        .or_default()
        .package_manager_dependencies = Some(pm_deps(&[("pnpm", "11.0.0")]));
    let engine = EngineToVerify {
        label: "pnpm@11.0.0",
        packages: &["pnpm", "@pnpm/exe"],
        platform_binaries: PlatformBinaries::PnpmExe,
    };

    let Err(error) = collect_engine_components(&env, &Config::default(), &engine) else {
        panic!("a half-recorded engine cannot be verified");
    };

    assert!(matches!(error, SelfUpdateError::EngineIdentityUnverifiable { .. }), "{error:?}");
}

#[test]
fn tolerable_without_signature_requires_a_soft_category_and_a_non_canonical_registry() {
    let failure = |category: FailureCategory, registry: &str| SignatureFailure {
        label: "pnpm@12.0.0".to_string(),
        registry: registry.to_string(),
        reason: "reason".to_string(),
        category,
    };
    let mirror = "https://mirror.example.com/";
    assert!(failure(FailureCategory::Unreachable, mirror).tolerable_without_signature());
    assert!(failure(FailureCategory::Uncovered, mirror).tolerable_without_signature());
    assert!(!failure(FailureCategory::Absent, mirror).tolerable_without_signature());
    assert!(!failure(FailureCategory::Invalid, mirror).tolerable_without_signature());
    // The canonical registry always provides signatures for genuine
    // releases, so nothing is tolerated there — under any URL-equivalent
    // spelling of it.
    for canonical in [
        "https://registry.npmjs.org",
        "https://Registry.NPMJS.org:443/",
        "https://registry.npmjs.org:443",
        "https://user:pass@registry.npmjs.org/",
        "https://user:p\rass@registry.npmjs.org/",
    ] {
        assert!(
            !failure(FailureCategory::Unreachable, canonical).tolerable_without_signature(),
            "{canonical} must count as canonical",
        );
        assert!(!failure(FailureCategory::Uncovered, canonical).tolerable_without_signature());
    }
}

fn no_retry() -> RetryOpts {
    RetryOpts {
        retries: 0,
        factor: 2,
        min_timeout: Duration::from_millis(1),
        max_timeout: Duration::from_millis(1),
    }
}

fn packument_body(name: &str, version: &str, signatures_json: &str) -> String {
    format!(
        r#"{{"name":"{name}","time":{{"{version}":"2024-01-01T00:00:00.000Z"}},"versions":{{"{version}":{{"dist":{{"signatures":{signatures_json}}}}}}}}}"#,
    )
}

async fn mock_packument(server: &mut mockito::ServerGuard, signatures_json: &str) -> mockito::Mock {
    server
        .mock("GET", "/pnpm")
        .with_status(200)
        .with_body(packument_body("pnpm", "12.0.0", signatures_json))
        .create_async()
        .await
}

fn signatures_json(key: &SigningKey, message: &str) -> String {
    format!(r#"[{{"keyid":"SHA256:test","sig":"{}"}}]"#, sign_b64(key, message))
}

/// `find_signature_failure` against a mirror at `server` and a fallback at
/// `fallback_registry`, trusting only the test key.
async fn find_failure_with_fallback(
    component: &EngineComponent,
    fallback_registry: &str,
) -> Option<SignatureFailure> {
    let key = signing_key();
    let pub_b64 = public_key_b64(&key);
    let keys = [NpmSigningKey { keyid: "SHA256:test", key: &pub_b64, expires: None }];
    let config = Config::default();
    let client = build_client(&config).expect("build client");
    find_signature_failure(component, fallback_registry, &keys, &client, no_retry(), &config).await
}

#[tokio::test]
async fn falls_back_to_the_canonical_registry_when_the_mirror_serves_no_signatures() {
    let mut mirror = mockito::Server::new_async().await;
    let mut fallback = mockito::Server::new_async().await;
    let component = EngineComponent { registry: format!("{}/", mirror.url()), ..component() };
    let _mirror = mock_packument(&mut mirror, "[]").await;
    let _fallback = mock_packument(
        &mut fallback,
        &signatures_json(&signing_key(), &signed_message(&component)),
    )
    .await;

    let failure = find_failure_with_fallback(&component, &fallback.url()).await;
    assert!(failure.is_none(), "expected a fallback pass, got {:?}", failure.map(|f| f.reason));
}

#[tokio::test]
async fn a_fallback_signature_still_fails_over_a_tampered_integrity() {
    let mut mirror = mockito::Server::new_async().await;
    let mut fallback = mockito::Server::new_async().await;
    let component = EngineComponent { registry: format!("{}/", mirror.url()), ..component() };
    let _mirror = mock_packument(&mut mirror, "[]").await;
    // The fallback signed different bytes than the lockfile pins.
    let _fallback = mock_packument(
        &mut fallback,
        &signatures_json(&signing_key(), "pnpm@12.0.0:sha512-genuine"),
    )
    .await;

    let failure =
        find_failure_with_fallback(&component, &fallback.url()).await.expect("failure expected");
    assert!(matches!(failure.category, FailureCategory::Invalid));
}

#[tokio::test]
async fn reports_unreachable_when_neither_registry_can_provide_a_signature() {
    let mut mirror = mockito::Server::new_async().await;
    let component = EngineComponent { registry: format!("{}/", mirror.url()), ..component() };
    let _mirror = mock_packument(&mut mirror, "[]").await;

    // Nothing listens on the fallback address, so consulting it fails.
    let failure = find_failure_with_fallback(&component, "http://127.0.0.1:9/")
        .await
        .expect("failure expected");
    assert!(matches!(failure.category, FailureCategory::Unreachable));
    assert!(failure.reason.contains("127.0.0.1:9"), "unexpected reason: {}", failure.reason);
}

#[tokio::test]
async fn does_not_retry_an_unavailable_fallback_registry() {
    let mut mirror = mockito::Server::new_async().await;
    let mut fallback = mockito::Server::new_async().await;
    let component = EngineComponent { registry: format!("{}/", mirror.url()), ..component() };
    let _mirror = mock_packument(&mut mirror, "[]").await;
    let fallback_mock =
        fallback.mock("GET", "/pnpm").with_status(502).expect(1).create_async().await;
    let retry_opts = RetryOpts {
        retries: 2,
        factor: 1,
        min_timeout: Duration::from_millis(1),
        max_timeout: Duration::from_millis(1),
    };
    let key = signing_key();
    let pub_b64 = public_key_b64(&key);
    let keys = [NpmSigningKey { keyid: "SHA256:test", key: &pub_b64, expires: None }];
    let config = Config::default();
    let client = build_client(&config).expect("build client");

    let failure =
        find_signature_failure(&component, &fallback.url(), &keys, &client, retry_opts, &config)
            .await
            .expect("failure expected");

    assert!(matches!(failure.category, FailureCategory::Unreachable));
    fallback_mock.assert_async().await;
}

#[tokio::test]
async fn reports_absent_when_a_reachable_fallback_has_no_signed_release() {
    let mut mirror = mockito::Server::new_async().await;
    let mut fallback = mockito::Server::new_async().await;
    let component = EngineComponent { registry: format!("{}/", mirror.url()), ..component() };
    let _mirror = mock_packument(&mut mirror, "[]").await;
    let _fallback = fallback.mock("GET", "/pnpm").with_status(404).create_async().await;

    let failure =
        find_failure_with_fallback(&component, &fallback.url()).await.expect("failure expected");
    assert!(matches!(failure.category, FailureCategory::Absent));
}

#[tokio::test]
async fn verifies_via_the_fallback_when_the_mirror_serves_an_unusable_signature() {
    let mut mirror = mockito::Server::new_async().await;
    let mut fallback = mockito::Server::new_async().await;
    let component = EngineComponent { registry: format!("{}/", mirror.url()), ..component() };
    // e.g. a mirror caching a stale signature from a rotated-out key
    let _mirror =
        mock_packument(&mut mirror, r#"[{"keyid":"SHA256:rotated-out","sig":"c3RhbGU="}]"#).await;
    let _fallback = mock_packument(
        &mut fallback,
        &signatures_json(&signing_key(), &signed_message(&component)),
    )
    .await;

    let failure = find_failure_with_fallback(&component, &fallback.url()).await;
    assert!(failure.is_none(), "expected a fallback pass, got {:?}", failure.map(|f| f.reason));
}

#[tokio::test]
async fn reports_a_non_sha512_integrity_as_uncovered_without_consulting_any_registry() {
    // No servers are mocked: the category is decided before any fetch.
    let component = EngineComponent {
        integrity: "sha1-i+4AKGoXwAoTx+bm3ZqbOJIg7n8=".to_string(),
        ..component()
    };
    let failure = find_failure_with_fallback(&component, "http://127.0.0.1:9/")
        .await
        .expect("failure expected");
    assert!(matches!(failure.category, FailureCategory::Uncovered));
}
