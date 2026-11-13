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

    // Current revision passes inline from install-v2 without fetching +r2.
    let current = LockfileResolution::Registry(RegistryResolution {
        integrity: second_integrity.clone(),
        revision: Some(TarballRevision::try_from(2).expect("revision")),
    });
    assert_eq!(verifier.verify(&current, ctx(&name, "1.0.0")).await, ResolutionVerification::Ok);

    // Mismatched integrity on current revision fails closed.
    let bad_current = LockfileResolution::Registry(RegistryResolution {
        integrity: first_integrity.clone(),
        revision: Some(TarballRevision::try_from(2).expect("revision")),
    });
    assert!(matches!(
        verifier.verify(&bad_current, ctx(&name, "1.0.0")).await,
        ResolutionVerification::Err { code: "TARBALL_REVISION_MISMATCH", .. },
    ));

    // Historical revision fetches +r1 release record and passes when integrity matches.
    let historical = LockfileResolution::Registry(RegistryResolution {
        integrity: first_integrity,
        revision: Some(TarballRevision::try_from(1).expect("revision")),
    });
    assert_eq!(verifier.verify(&historical, ctx(&name, "1.0.0")).await, ResolutionVerification::Ok);

    // Unknown revision 404s on +r9 and fails closed with TARBALL_REVISION_MISMATCH.
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
