use super::{
    Arc, AuthHeaders, HashMap, InMemoryPackageMetaCache, LockfileResolution,
    MalformedRevisionHistoryError, NpmResolver, ResolveOptions, RetryOpts, TarballRevision,
    TempDir, ThrottledClient, WantedDependency, assert_eq, build_resolver, json,
    revision_package_body, shared_packument_fetch_locker, shared_picked_manifest_cache,
};
use pnpm_resolving_resolver_base::Resolver;

/// Two `NpmResolvers` pointing at different registries, sharing the
/// same `picked_manifest_cache`, must not hand each other the
/// other's manifest when both happen to pick `acme@1.0.0`. Two
/// registries can serve different artifacts under the same
/// `name@version` (a public + private package collision, or a
/// fork), and collapsing the cache key to `name@version` alone
/// would propagate one registry's manifest into the other
/// resolver's `ResolveResult`, breaking the downstream dependency
/// graph / peer extraction / lockfile metadata.
#[tokio::test]
async fn shared_manifest_cache_does_not_leak_across_registries() {
    fn body_with_dep(dep_name: &str, dep_range: &str) -> String {
        format!(
            r#"{{
                "name": "acme",
                "dist-tags": {{ "latest": "1.0.0" }},
                "modified": "2025-01-15T12:00:00.000Z",
                "time": {{ "1.0.0": "2024-01-10T08:30:00.000Z" }},
                "versions": {{
                    "1.0.0": {{
                        "name": "acme",
                        "version": "1.0.0",
                        "dependencies": {{ "{dep_name}": "{dep_range}" }},
                        "dist": {{
                            "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                            "shasum": "0000000000000000000000000000000000000000",
                            "tarball": "https://registry/acme-1.0.0.tgz"
                        }}
                    }}
                }}
            }}"#,
        )
    }

    let mut server_a = mockito::Server::new_async().await;
    let _mock_a = server_a
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(body_with_dep("left-pad", "^1.0.0"))
        .create_async()
        .await;
    let mut server_b = mockito::Server::new_async().await;
    let _mock_b = server_b
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(body_with_dep("right-pad", "^2.0.0"))
        .create_async()
        .await;

    // Shared cache — the leak path. The fix is the cache key
    // including the registry; without it, whichever resolver runs
    // second would return the other's manifest.
    let shared_picked_cache = shared_picked_manifest_cache();
    let shared_fetch_locker = shared_packument_fetch_locker();

    let make_resolver = |registry: String| -> (NpmResolver<InMemoryPackageMetaCache>, TempDir) {
        let mut registries = HashMap::new();
        registries.insert("default".to_string(), registry);
        let cache_dir = TempDir::new().expect("tempdir");
        let resolver = NpmResolver {
            registries,
            registries_by_prefix: HashMap::new(),
            http_client: Arc::new(ThrottledClient::default()),
            auth_headers: Arc::new(AuthHeaders::default()),
            meta_cache: Arc::new(InMemoryPackageMetaCache::default()),
            fetch_locker: Arc::clone(&shared_fetch_locker),
            picked_manifest_cache: Arc::clone(&shared_picked_cache),
            cache_dir: Some(cache_dir.path().to_path_buf()),
            offline: false,
            prefer_offline: false,
            ignore_missing_time_field: false,
            full_metadata: false,
            needs_full_metadata_for: None,
            filter_metadata: false,
            retry_opts: RetryOpts::default(),
        };
        (resolver, cache_dir)
    };

    let (resolver_a, _cache_dir_a) = make_resolver(format!("{}/", server_a.url()));
    let (resolver_b, _cache_dir_b) = make_resolver(format!("{}/", server_b.url()));

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0".to_string()),
        ..WantedDependency::default()
    };

    let result_a = resolver_a
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect("resolver A")
        .expect("resolver A picks");
    let result_b = resolver_b
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect("resolver B")
        .expect("resolver B picks");

    let deps_a = result_a
        .manifest
        .as_ref()
        .and_then(|m| m.get("dependencies"))
        .and_then(|d| d.as_object())
        .expect("resolver A manifest carries dependencies");
    let deps_b = result_b
        .manifest
        .as_ref()
        .and_then(|m| m.get("dependencies"))
        .and_then(|d| d.as_object())
        .expect("resolver B manifest carries dependencies");

    assert!(deps_a.contains_key("left-pad"), "resolver A keeps its own manifest: {deps_a:?}");
    assert!(
        deps_b.contains_key("right-pad"),
        "resolver B got its own manifest, not resolver A's: {deps_b:?}",
    );
    assert!(
        !deps_b.contains_key("left-pad"),
        "resolver B must not see resolver A's `left-pad`: {deps_b:?}",
    );
}

#[tokio::test]
async fn revision_metadata_is_validated_and_preserved() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let tarball = format!("{}-/tarballs/sha512/{}", registry, "A".repeat(86));
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(revision_package_body(&tarball, &json!(2)))
        .create_async()
        .await;
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();

    mock.assert_async().await;
    let LockfileResolution::Tarball(resolution) = result.resolution else {
        panic!("expected a tarball resolution");
    };
    assert_eq!(resolution.tarball, tarball);
    assert_eq!(resolution.revision.map(TarballRevision::get), Some(2));
}

#[tokio::test]
async fn malformed_revision_metadata_has_the_malformed_metadata_error() {
    for revision in
        [json!(0), json!(-1), json!(1.5), json!(9_007_199_254_740_992_u64), json!("1"), json!("01")]
    {
        let mut server = mockito::Server::new_async().await;
        let registry = format!("{}/", server.url());
        let tarball = format!("{}-/tarballs/sha512/{}", registry, "A".repeat(86));
        server
            .mock("GET", "/acme")
            .with_status(200)
            .with_body(revision_package_body(&tarball, &revision))
            .create_async()
            .await;
        let (resolver, _tempdir) = build_resolver(&registry);

        let wanted =
            WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };
        let error = match resolver.resolve(&wanted, &ResolveOptions::default()).await {
            Ok(result) => panic!("revision {revision} must fail the resolve; got {result:?}"),
            Err(error) => error,
        };

        let error =
            error.downcast_ref::<MalformedRevisionHistoryError>().expect("revision history error");
        assert_eq!(error.name, "acme");
        assert_eq!(error.version, "1.0.0");
    }
}

#[tokio::test]
async fn invalid_shasum_error_redacts_registry_metadata() {
    let mut server = mockito::Server::new_async().await;
    let body = json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "modified": "2025-01-15T12:00:00.000Z",
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "shasum": "not\u{7}-a-hex-digest",
                    "tarball": "https://user:hunter2@registry/acme-1.0.0.tgz",
                },
            },
        },
    })
    .to_string();
    let _mock = server.mock("GET", "/acme").with_status(200).with_body(body).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };
    let error = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect_err("an unusable shasum must fail the resolve")
        .to_string();

    assert!(!error.contains("hunter2"), "inline credentials must not reach the message: {error}");
    assert!(
        !error.chars().any(char::is_control),
        "control characters must not reach the message: {error:?}",
    );
}
