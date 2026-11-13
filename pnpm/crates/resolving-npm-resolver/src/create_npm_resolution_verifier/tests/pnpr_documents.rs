use super::{
    LockfileResolution, PkgName, REVISION_ONE_DIGEST, REVISION_TWO_DIGEST, RegistryResolution,
    ResolutionVerification, TarballRevision, assert_eq, create_npm_resolution_verifier, ctx,
    default_opts, now_at, revision_integrity,
};
use pnpm_resolving_resolver_base::ResolutionVerifier;

fn declare_pnpr_registry(opts: &mut super::CreateNpmResolutionVerifierOptions, registry: &str) {
    opts.registry_options_by_url = Some(std::collections::BTreeMap::from([(
        registry.to_string(),
        pnpm_lockfile::RegistryOptions {
            server_type: Some(pnpm_lockfile::RegistryServerType::Pnpr),
            ..Default::default()
        },
    )]));
}

#[tokio::test]
async fn pnpr_document_verifies_current_and_historical_revisions() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let first_integrity = revision_integrity(REVISION_ONE_DIGEST);
    let second_integrity = revision_integrity(REVISION_TWO_DIGEST);
    let install_v2 = serde_json::json!({
        "name": "revision-pkg",
        "distTags": { "latest": "1.0.0" },
        "modified": "2025-01-01T00:00:00.000Z",
        "timesSince": "2024-12-01T00:00:00.000Z",
        "versions": {
            "1.0.0": {
                "revision": 2,
                "integrity": second_integrity.to_string(),
                "manifest": {},
            },
        },
    });
    let doc_mock = server
        .mock("GET", "/revision-pkg")
        .match_header("accept", crate::fetch_full_metadata::ACCEPT_PNPR_DOC)
        .with_status(200)
        .with_header("content-type", "application/vnd.pnpr.install-v2+json")
        .with_body(install_v2.to_string())
        .expect(1)
        .create_async()
        .await;
    let r1_record = serde_json::json!({
        "name": "revision-pkg",
        "version": "1.0.0",
        "integrity": first_integrity.to_string(),
        "manifest": {},
    });
    let r1_mock = server
        .mock("GET", "/revision-pkg/1.0.0+r1")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r1_record.to_string())
        .expect(1)
        .create_async()
        .await;
    let r9_mock = server
        .mock("GET", "/revision-pkg/1.0.0+r9")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;

    let mut opts = default_opts(&registry);
    opts.release_age.minimum_minutes = Some(1);
    opts.now = Some(now_at("2025-02-01T00:00:00Z"));
    declare_pnpr_registry(&mut opts, &registry);
    let verifier = create_npm_resolution_verifier(opts);
    let name: PkgName = "revision-pkg".parse().expect("parse");

    let current = LockfileResolution::Registry(RegistryResolution {
        integrity: second_integrity.clone(),
        revision: Some(TarballRevision::try_from(2).expect("revision")),
    });
    assert_eq!(verifier.verify(&current, ctx(&name, "1.0.0")).await, ResolutionVerification::Ok);

    let bad_current = LockfileResolution::Registry(RegistryResolution {
        integrity: first_integrity.clone(),
        revision: Some(TarballRevision::try_from(2).expect("revision")),
    });
    assert!(matches!(
        verifier.verify(&bad_current, ctx(&name, "1.0.0")).await,
        ResolutionVerification::Err { code: "TARBALL_REVISION_MISMATCH", .. },
    ));

    let historical = LockfileResolution::Registry(RegistryResolution {
        integrity: first_integrity,
        revision: Some(TarballRevision::try_from(1).expect("revision")),
    });
    assert_eq!(verifier.verify(&historical, ctx(&name, "1.0.0")).await, ResolutionVerification::Ok);

    let unknown = LockfileResolution::Registry(RegistryResolution {
        integrity: second_integrity,
        revision: Some(TarballRevision::try_from(9).expect("revision")),
    });
    assert!(matches!(
        verifier.verify(&unknown, ctx(&name, "1.0.0")).await,
        ResolutionVerification::Err { code: "TARBALL_REVISION_MISMATCH", .. },
    ));

    doc_mock.assert_async().await;
    r1_mock.assert_async().await;
    r9_mock.assert_async().await;
}

#[tokio::test]
async fn trust_ignore_after_beyond_pnpr_window_falls_back_to_full_packument() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let integrity = revision_integrity(REVISION_ONE_DIGEST);
    let install_v2 = serde_json::json!({
        "name": "settled-pkg",
        "distTags": { "latest": "1.5.0" },
        "modified": "2025-01-01T00:00:00.000Z",
        "timesSince": "2025-01-02T00:00:00.000Z",
        "versions": {
            "1.0.0": {
                "trust": "trustedPublisher",
                "integrity": integrity.to_string(),
                "manifest": {},
            },
            "1.5.0": {
                "integrity": integrity.to_string(),
                "manifest": {},
            },
        },
    });
    let doc_mock = server
        .mock("GET", "/settled-pkg")
        .match_header("accept", crate::fetch_full_metadata::ACCEPT_PNPR_DOC)
        .with_status(200)
        .with_header("content-type", "application/vnd.pnpr.install-v2+json")
        .with_body(install_v2.to_string())
        .expect(0)
        .create_async()
        .await;
    let full_packument = serde_json::json!({
        "name": "settled-pkg",
        "dist-tags": { "latest": "1.5.0" },
        "time": {
            "1.0.0": "2024-06-01T00:00:00.000Z",
            "1.5.0": "2024-11-01T00:00:00.000Z",
        },
        "versions": {
            "1.0.0": {
                "name": "settled-pkg",
                "version": "1.0.0",
                "_npmUser": { "trustedPublisher": { "id": "github" } },
                "dist": {
                    "integrity": integrity.to_string(),
                    "tarball": format!("{registry}settled-pkg@1.0.0"),
                    "attestations": { "provenance": { "predicateType": "https://slsa.dev/provenance/v1" } },
                },
            },
            "1.5.0": {
                "name": "settled-pkg",
                "version": "1.5.0",
                "dist": {
                    "integrity": integrity.to_string(),
                    "tarball": format!("{registry}settled-pkg@1.5.0"),
                },
            },
        },
    });
    let full_mock = server
        .mock("GET", "/settled-pkg")
        .match_header("accept", crate::fetch_full_metadata::ACCEPT_FULL_DOC)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(full_packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let mut opts = default_opts(&registry);
    opts.now = Some(now_at("2025-02-01T00:00:00Z"));
    opts.trust.policy = Some(pnpm_resolving_resolver_base::TrustPolicy::NoDowngrade);
    opts.trust.ignore_after = Some(60 * 24 * 45);
    declare_pnpr_registry(&mut opts, &registry);
    let verifier = create_npm_resolution_verifier(opts);
    let name: PkgName = "settled-pkg".parse().expect("parse");
    let resolution = LockfileResolution::Registry(RegistryResolution { integrity, revision: None });

    assert!(matches!(
        verifier.verify(&resolution, ctx(&name, "1.5.0")).await,
        ResolutionVerification::Ok,
    ));
    doc_mock.assert_async().await;
    full_mock.assert_async().await;
}

#[tokio::test]
async fn release_age_beyond_pnpr_window_reads_the_full_packument_time() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let integrity = revision_integrity(REVISION_ONE_DIGEST);
    let install_v2 = serde_json::json!({
        "name": "settled-pkg",
        "distTags": { "latest": "1.0.0" },
        "modified": "2025-01-20T00:00:00.000Z",
        "timesSince": "2025-01-02T00:00:00.000Z",
        "versions": {
            "1.0.0": {
                "integrity": integrity.to_string(),
                "manifest": {},
            },
        },
    });
    server
        .mock("GET", "/settled-pkg")
        .match_header("accept", crate::fetch_full_metadata::ACCEPT_PNPR_DOC)
        .with_status(200)
        .with_header("content-type", "application/vnd.pnpr.install-v2+json")
        .with_body(install_v2.to_string())
        .create_async()
        .await;
    let full_packument = serde_json::json!({
        "name": "settled-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2024-06-01T00:00:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "settled-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": integrity.to_string(),
                    "tarball": format!("{registry}settled-pkg@1.0.0"),
                },
            },
        },
    });
    let full_mock = server
        .mock("GET", "/settled-pkg")
        .match_header("accept", crate::fetch_full_metadata::ACCEPT_FULL_DOC)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(full_packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let mut opts = default_opts(&registry);
    opts.now = Some(now_at("2025-02-01T00:00:00Z"));
    opts.release_age.minimum_minutes = Some(60 * 24 * 45);
    declare_pnpr_registry(&mut opts, &registry);
    let verifier = create_npm_resolution_verifier(opts);
    let name: PkgName = "settled-pkg".parse().expect("parse");
    let resolution = LockfileResolution::Registry(RegistryResolution { integrity, revision: None });

    assert_eq!(verifier.verify(&resolution, ctx(&name, "1.0.0")).await, ResolutionVerification::Ok);
    full_mock.assert_async().await;
}

#[tokio::test]
async fn undeclared_registry_cannot_spoof_install_v2_marker_to_skip_history_check() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let second_integrity = revision_integrity(REVISION_TWO_DIGEST);
    let spoofed = serde_json::json!({
        "name": "revision-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2020-01-01T00:00:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "revision-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": second_integrity.to_string(),
                    "tarball": format!("{registry}revision-pkg@1.0.0+r2"),
                    "revision": 2,
                    "pnpr": { "installV2": "complete" },
                },
            },
        },
    });
    let _mock = server
        .mock("GET", "/revision-pkg")
        .with_status(200)
        .with_body(spoofed.to_string())
        .create_async()
        .await;

    let mut opts = default_opts(&registry);
    opts.release_age.minimum_minutes = Some(1);
    let verifier = create_npm_resolution_verifier(opts);
    let name: PkgName = "revision-pkg".parse().expect("parse");
    let resolution = LockfileResolution::Registry(RegistryResolution {
        integrity: second_integrity,
        revision: Some(TarballRevision::try_from(2).expect("revision")),
    });
    assert!(matches!(
        verifier.verify(&resolution, ctx(&name, "1.0.0")).await,
        ResolutionVerification::Err { code: "TARBALL_REVISION_MISMATCH", .. },
    ));
}
