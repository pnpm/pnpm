use super::{
    ABBREVIATED_META_DIR, Arc, AuthHeaders, FAKE_INTEGRITY, LockfileResolution, MetadataCacheScope,
    Package, PkgName, ResolutionVerification, ScopeHook, TarballResolution, TempDir,
    UpstreamRouteHook, assert_eq, create_npm_resolution_verifier, ctx, default_opts,
    fake_integrity, get_pkg_mirror_path, load_meta, now_at, persist_meta_to_mirror,
    registry_resolution,
};
use pnpm_resolving_resolver_base::ResolutionVerifier;

#[tokio::test]
async fn private_scope_verifier_ignores_public_mirror_and_writes_private_mirror() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let public_tarball = format!("{server_url}/acme/-/acme-1.0.0.tgz");
    let private_tarball = format!("{server_url}/acme/-/acme-private-1.0.0.tgz");
    let public_packument = serde_json::json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": public_tarball,
                }
            }
        }
    });
    let private_packument = serde_json::json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": private_tarball,
                }
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(private_packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let cache = TempDir::new().expect("tempdir");
    let public_meta: Package = serde_json::from_value(public_packument).expect("package parses");
    persist_meta_to_mirror(cache.path(), ABBREVIATED_META_DIR, &registry, &public_meta)
        .expect("warm public mirror");

    let mut opts = default_opts(&registry);
    opts.cache_dir = Some(cache.path().to_path_buf());
    opts.auth_headers =
        Arc::new(AuthHeaders::default().with_route_hook(Arc::new(ScopeHook {
            scope: MetadataCacheScope::Private { descriptor_id: "private-scope".to_string() },
        }) as Arc<dyn UpstreamRouteHook>));
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: public_tarball.clone(),
        integrity: Some(fake_integrity()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let name: PkgName = "acme".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;

    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected private metadata mismatch, got {result:?}");
    };
    assert_eq!(code, "TARBALL_URL_MISMATCH");

    let private_path = get_pkg_mirror_path(
        cache.path(),
        "v11/metadata-private/private-scope/metadata",
        &registry,
        "acme",
    )
    .expect("private mirror path");
    let private_meta = load_meta(&private_path).expect("private mirror written");
    let private_version = private_meta.versions.get("1.0.0").expect("private version");
    assert_eq!(private_version.dist.tarball, private_tarball);

    let public_path =
        get_pkg_mirror_path(cache.path(), ABBREVIATED_META_DIR, &registry, "acme").expect("path");
    let public_meta = load_meta(&public_path).expect("public mirror remains readable");
    let public_version = public_meta.versions.get("1.0.0").expect("public version");
    assert_eq!(public_version.dist.tarball, public_tarball);
}

#[tokio::test]
async fn registry_resolution_with_no_active_policy_skips_metadata_lookup() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _meta_mock = server.mock("GET", "/acme").expect(0).create_async().await;

    let opts = default_opts(&registry);
    let verifier = create_npm_resolution_verifier(opts);
    let name: PkgName = "acme".parse().expect("parse");
    assert!(!verifier.might_verify(&registry_resolution(), ctx(&name, "1.0.0")));
    let result = verifier.verify(&registry_resolution(), ctx(&name, "1.0.0")).await;

    assert_eq!(result, ResolutionVerification::Ok);
}

#[tokio::test]
async fn planned_fetch_head_shortcut_skips_the_metadata_body() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _head_mock = server
        .mock("HEAD", "/acme")
        .with_status(200)
        .with_header("last-modified", "Mon, 01 Jan 2024 00:00:00 GMT")
        .expect(1)
        .create_async()
        .await;
    // The whole point: no metadata body is fetched for a planned entry
    // whose package-level Last-Modified is older than the cutoff.
    let _meta_mock = server.mock("GET", "/acme").expect(0).create_async().await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(60 * 24);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let planned = pnpm_resolving_resolver_base::PlannedCanonicalFetches::default();
    planned
        .set(std::collections::HashSet::from([("acme".to_string(), "1.0.0".to_string(), None)]))
        .expect("first fill");
    opts.planned_canonical_fetches = Some(std::sync::Arc::clone(&planned));
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// A 403 on the metadata fetch (e.g. a CI token that is authenticated but not
/// authorized to read a private package) must not be reported as a lockfile
/// tarball-URL mismatch: the lockfile is correct, the fetch is the problem. The
/// verifier propagates the registry's own fetch error so the install aborts.
#[tokio::test]
async fn propagates_metadata_fetch_failure_instead_of_a_tampering_mismatch() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let _meta_mock = server
        .mock("GET", "/private-pkg")
        .with_status(403)
        .with_body(r#"{"error":"Forbidden"}"#)
        .create_async()
        .await;

    let opts = default_opts(&registry);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("{server_url}/private-pkg/-/private-pkg-1.0.0.tgz"),
        integrity: Some(fake_integrity()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let name: PkgName = "private-pkg".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;

    // A transport failure aborts via FetchFailed, never a tampering-style
    // TARBALL_URL_MISMATCH.
    let ResolutionVerification::FetchFailed { message } = result else {
        panic!("expected FetchFailed, got {result:?}");
    };
    assert!(message.contains("403"), "message: {message}");
}
