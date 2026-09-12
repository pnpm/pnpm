use super::{
    FAKE_INTEGRITY, LockfileResolution, PkgName, ResolutionVerification, TarballResolution, Utc,
    assert_eq, create_npm_resolution_verifier, ctx, default_opts, fake_integrity,
    registry_resolution,
};
use pnpm_resolving_resolver_base::ResolutionVerifier;

#[tokio::test]
async fn verify_short_circuits_non_semver_version() {
    let mut opts = default_opts("https://registry.example/");
    opts.minimum_release_age = Some(60 * 24 * 365);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = registry_resolution();
    let name: PkgName = "acme".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "not-semver")).await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// With `registrySupportsTimeField`, a version's publish timestamp is
/// taken from the `time` map of the abbreviated document the verifier
/// already fetched. The `modified` shortcut cannot answer here (the
/// package was modified inside the cutoff window), and no other source
/// is mocked, so a passing verification proves the timestamp came from
/// the abbreviated document — the attestation round-trip and the
/// full-packument download never happen.
#[tokio::test]
async fn registry_supports_time_field_reads_version_time_from_abbreviated_meta() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let now = Utc::now();
    let meta = serde_json::json!({
        "name": "aged-pkg",
        "dist-tags": { "latest": "1.0.0" },
        // Package touched *inside* the cutoff window: the modified
        // shortcut must fall through.
        "modified": now.to_rfc3339(),
        "time": {
            "created": "2020-01-01T00:00:00.000Z",
            "modified": now.to_rfc3339(),
            // The pinned version itself is years old.
            "1.0.0": "2020-01-01T00:00:00.000Z"
        },
        "versions": {
            "1.0.0": {
                "name": "aged-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("{server_url}/aged-pkg/-/aged-pkg-1.0.0.tgz"),
                }
            }
        }
    });
    let meta_mock = server
        .mock("GET", "/aged-pkg")
        .with_status(200)
        .with_body(meta.to_string())
        // The whole point: one abbreviated fetch answers everything. A
        // second hit would be the full-packument fallback this flag
        // exists to avoid.
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(60 * 24); // 24h
    opts.registry_supports_time_field = true;
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("{server_url}/aged-pkg/-/aged-pkg-1.0.0.tgz"),
        integrity: Some(fake_integrity()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let name: PkgName = "aged-pkg".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    assert_eq!(result, ResolutionVerification::Ok);
    meta_mock.assert_async().await;
}
