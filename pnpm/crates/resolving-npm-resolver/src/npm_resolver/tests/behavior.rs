use super::{
    Arc, CurrentPkg, HashMap, InvalidRevisionSpecifierError, InvalidTarballIntegrityError,
    JSR_PACKAGE_BODY, LockfileResolution, MalformedRevisionHistoryError, NoMatchingRevisionError,
    PACKAGE_BODY, PkgResolutionId, ResolveOptions, TarballRevision, UpdateBehavior,
    WantedDependency, assert_eq, build_resolver, build_resolver_with_registries, json,
    reject_versions, revision_history_package_body, revision_package_body,
    shasum_only_package_body,
};
use pnpm_config::TrustPolicy;
use pnpm_resolving_resolver_base::Resolver;

#[tokio::test]
async fn calculated_specifier_keeps_the_operator_the_previous_specifier_declared() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.1.0".to_string()),
        prev_specifier: Some("~1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let opts = ResolveOptions {
        specifier: pnpm_resolving_resolver_base::ResolverSpecifierOptions {
            calc_specifier: true,
            ..Default::default()
        },
        ..ResolveOptions::default()
    };
    let result = resolver
        .resolve(&wanted, &opts)
        .await
        .unwrap()
        .unwrap();
    // An exact requested version is honored over the previous range operator (pnpm/pnpm#6040).
    assert_eq!(result.normalized_bare_specifier.as_deref(), Some("1.1.0"));

    let wanted_latest = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("latest".to_string()),
        prev_specifier: Some("~1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result_latest = resolver
        .resolve(&wanted_latest, &opts)
        .await
        .unwrap()
        .unwrap();
    // A request without a range style (like latest) keeps the previous operator.
    assert_eq!(result_latest.normalized_bare_specifier.as_deref(), Some("~1.1.0"));
}

#[tokio::test]
async fn jsr_specifier_routes_through_jsr_registry() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/@jsr%2Ffoo__bar")
        .with_status(200)
        .with_body(JSR_PACKAGE_BODY)
        .create_async()
        .await;
    let jsr_registry = format!("{}/", server.url());
    let mut registries = HashMap::new();
    registries.insert("default".to_string(), "https://registry.npmjs.org/".to_string());
    registries.insert("@jsr".to_string(), jsr_registry);
    let (resolver, _tempdir) = build_resolver_with_registries(registries);

    let wanted = WantedDependency {
        alias: Some("@foo/bar".to_string()),
        bare_specifier: Some("jsr:@foo/bar@^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .unwrap()
        .unwrap();
    let name_ver = result.package.name_ver.as_ref().expect("npm resolver fills name_ver");
    assert_eq!(name_ver.name.to_string(), "@jsr/foo__bar");
    assert_eq!(name_ver.suffix.to_string(), "1.1.0");
    assert_eq!(result.resolved_via, "jsr-registry");
    assert_eq!(result.alias.as_deref(), Some("@foo/bar"));
    assert_eq!(result.package.latest.as_deref(), Some("1.1.0"));
    assert!(matches!(result.resolution, LockfileResolution::Tarball(_)));
}

#[tokio::test]
async fn jsr_calculated_specifier_keeps_the_operator_the_previous_specifier_declared() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/@jsr%2Ffoo__bar")
        .with_status(200)
        .with_body(JSR_PACKAGE_BODY)
        .create_async()
        .await;
    let jsr_registry = format!("{}/", server.url());
    let mut registries = HashMap::new();
    registries.insert("default".to_string(), "https://registry.npmjs.org/".to_string());
    registries.insert("@jsr".to_string(), jsr_registry);
    let (resolver, _tempdir) = build_resolver_with_registries(registries);

    let wanted = WantedDependency {
        alias: Some("@foo/bar".to_string()),
        bare_specifier: Some("jsr:@foo/bar@1.1.0".to_string()),
        prev_specifier: Some("jsr:@foo/bar@~1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let opts = ResolveOptions {
        specifier: pnpm_resolving_resolver_base::ResolverSpecifierOptions {
            calc_specifier: true,
            ..Default::default()
        },
        ..ResolveOptions::default()
    };
    let result = resolver
        .resolve(&wanted, &opts)
        .await
        .unwrap()
        .unwrap();
    // An exact requested version is honored over the previous range operator (pnpm/pnpm#6040).
    assert_eq!(result.normalized_bare_specifier.as_deref(), Some("jsr:1.1.0"));

    let wanted_latest = WantedDependency {
        alias: Some("@foo/bar".to_string()),
        bare_specifier: Some("jsr:@foo/bar@latest".to_string()),
        prev_specifier: Some("jsr:@foo/bar@~1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result_latest = resolver
        .resolve(&wanted_latest, &opts)
        .await
        .unwrap()
        .unwrap();
    // A request without a range style (like latest) keeps the previous operator.
    assert_eq!(result_latest.normalized_bare_specifier.as_deref(), Some("jsr:~1.1.0"));
}

/// `optionalDependencies` and `peerDependenciesMeta` round-trip from the
/// registry's per-version manifest into [`ResolvedPackageInfo::manifest`]
/// (a [`serde_json::Value`]). Downstream
/// `extract_children` reads the optional-dep edges and
/// `extract_peer_dependencies` reads the per-peer `optional` flag;
/// dropping either field silently treats optional peers as required
/// (so `autoInstallPeers` cascades them in) and skips
/// `optionalDependencies` entirely. See pnpm/pnpm#11934.
#[tokio::test]
async fn resolved_manifest_carries_optional_dependencies_and_peer_dependencies_meta() {
    const BODY: &str = r#"{
        "name": "consumer",
        "dist-tags": { "latest": "1.0.0" },
        "modified": "2025-01-15T12:00:00.000Z",
        "versions": {
            "1.0.0": {
                "name": "consumer",
                "version": "1.0.0",
                "peerDependencies": {
                    "@vercel/kv": "^1 || ^2 || ^3",
                    "ioredis": "^5.4.2"
                },
                "peerDependenciesMeta": {
                    "@vercel/kv": { "optional": true },
                    "ioredis": { "optional": true }
                },
                "optionalDependencies": {
                    "sharp": "^0.34.0"
                },
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/consumer-1.0.0.tgz"
                }
            }
        }
    }"#;

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/consumer")
        .with_status(200)
        .with_body(BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("consumer".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .unwrap()
        .unwrap();
    let manifest = result.package.manifest.as_ref().expect("npm resolver populates manifest");

    let optional = manifest
        .get("optionalDependencies")
        .and_then(serde_json::Value::as_object)
        .expect("optionalDependencies present");
    assert_eq!(optional.get("sharp").and_then(serde_json::Value::as_str), Some("^0.34.0"));

    let peer_meta = manifest
        .get("peerDependenciesMeta")
        .and_then(serde_json::Value::as_object)
        .expect("peerDependenciesMeta present");
    assert_eq!(
        peer_meta
            .get("@vercel/kv")
            .and_then(|v| v.get("optional"))
            .and_then(serde_json::Value::as_bool),
        Some(true),
    );
    assert_eq!(
        peer_meta
            .get("ioredis")
            .and_then(|v| v.get("optional"))
            .and_then(serde_json::Value::as_bool),
        Some(true),
    );
}

#[tokio::test]
async fn jsr_specifier_with_invalid_scope_propagates_parser_error() {
    let server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("foo".to_string()),
        bare_specifier: Some("jsr:foo@^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .unwrap_err();
    let msg = err.to_string();
    // Asserting the error message ties the test to the public
    // `ERR_PNPM_MISSING_JSR_PACKAGE_SCOPE` contract; the resolver seam
    // returns the parser error as a boxed `dyn Error` so we can't
    // downcast to the variant directly.
    assert_eq!(msg, "Package names from JSR must have a scope", "unexpected error message: {msg}");
}

#[tokio::test]
async fn explicit_current_revision_accepts_its_matching_history_record() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(revision_history_package_body(&registry))
        .create_async()
        .await;
    let (resolver, _tempdir) = build_resolver(&registry);
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0+r2".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .unwrap()
        .unwrap();
    let LockfileResolution::Tarball(resolution) = &result.resolution else {
        panic!("expected tarball resolution");
    };
    assert_eq!(resolution.revision.map(TarballRevision::get), Some(2));
    assert_eq!(
        result.package.manifest.as_ref().expect("manifest")["dependencies"],
        json!({
            "selected-current": "1.0.0",
        }),
    );
}

#[tokio::test]
async fn explicit_original_revision_omits_the_lockfile_revision() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(revision_history_package_body(&registry))
        .create_async()
        .await;
    let (resolver, _tempdir) = build_resolver(&registry);
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0+r0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .unwrap()
        .unwrap();
    let LockfileResolution::Tarball(resolution) = &result.resolution else {
        panic!("expected tarball resolution");
    };
    assert_eq!(resolution.revision, None);
    assert_eq!(
        result.package.manifest.as_ref().expect("manifest")["dependencies"],
        json!({
            "original": "1.0.0",
        }),
    );
}

#[tokio::test]
async fn unknown_and_invalid_explicit_revisions_are_hard_errors() {
    for (specifier, expected_kind) in [("1.0.0+r9", "missing"), ("1.0.0+r01", "invalid")] {
        let mut server = mockito::Server::new_async().await;
        let registry = format!("{}/", server.url());
        server
            .mock("GET", "/acme")
            .with_status(200)
            .with_body(revision_history_package_body(&registry))
            .create_async()
            .await;
        let (resolver, _tempdir) = build_resolver(&registry);
        let wanted = WantedDependency {
            alias: Some("acme".to_string()),
            bare_specifier: Some(specifier.to_string()),
            ..WantedDependency::default()
        };
        let error = resolver
            .resolve(&wanted, &ResolveOptions::default())
            .await
            .unwrap_err();
        match expected_kind {
            "missing" => assert!(error.downcast_ref::<NoMatchingRevisionError>().is_some()),
            "invalid" => assert!(error.downcast_ref::<InvalidRevisionSpecifierError>().is_some()),
            _ => unreachable!(),
        }
    }
}

#[tokio::test]
async fn current_revision_requires_a_matching_history_entry() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let tarball = format!("{}-/tarballs/sha512/{}", registry, "A".repeat(86));
    let mut body: serde_json::Value =
        serde_json::from_str(&revision_package_body(&tarball, &json!(1))).unwrap();
    body["versions"]["1.0.0"]["dist"]["revisions"] = json!([]);
    server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(body.to_string())
        .create_async()
        .await;
    let (resolver, _tempdir) = build_resolver(&registry);
    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };

    let error = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .unwrap_err();

    assert!(error.downcast_ref::<MalformedRevisionHistoryError>().is_some());
}

#[tokio::test]
async fn unparsable_shasum_fails_the_resolve() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(shasum_only_package_body("not-a-hex-digest"))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };
    let error = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect_err("an unusable shasum must fail the resolve");

    let error = error.downcast_ref::<InvalidTarballIntegrityError>().expect("integrity error");
    assert_eq!(error.shasum, "not-a-hex-digest");
    assert_eq!(error.tarball, "https://registry/acme-1.0.0.tgz");
}

#[tokio::test]
async fn peek_manifest_from_store_bypasses_network_when_package_in_store() {
    let server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let (mut resolver, _tempdir) = build_resolver(&registry);

    let store_dir = tempfile::TempDir::new().unwrap();
    let index = pnpm_store_dir::StoreIndex::open(store_dir.path()).unwrap();
    let integrity_str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
    let pkg_id = "acme@1.0.0";
    let key = pnpm_store_dir::store_index_key(integrity_str, pkg_id);
    index
        .set(
            &key,
            &pnpm_store_dir::PackageFilesIndex {
                manifest: Some(json!({
                    "name": "acme",
                    "version": "1.0.0",
                })),
                ..Default::default()
            },
        )
        .unwrap();

    resolver.store_index = Some(Arc::new(std::sync::Mutex::new(index)));

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let opts = ResolveOptions {
        refresh: pnpm_resolving_resolver_base::ResolutionRefreshOptions {
            current_pkg: Some(CurrentPkg {
                id: PkgResolutionId::from(pkg_id),
                name: Some("acme".to_string()),
                version: Some("1.0.0".to_string()),
                resolution: LockfileResolution::Tarball(pnpm_lockfile::TarballResolution {
                    tarball: "https://registry/acme-1.0.0.tgz".to_string(),
                    integrity: Some(integrity_str.parse().unwrap()),
                    revision: None,
                    git_hosted: None,
                    path: None,
                }),
                published_at: None,
                manifest: None,
            }),
            update: UpdateBehavior::Off,
            ..Default::default()
        },
        ..ResolveOptions::default()
    };

    let result = resolver
        .resolve(&wanted, &opts)
        .await
        .unwrap()
        .expect("should resolve from store peek");

    assert_eq!(result.id.as_str(), "acme@1.0.0");
    assert_eq!(result.resolved_via, "npm-registry");
    let manifest = result.package.manifest.expect("manifest should be present");
    assert_eq!(manifest["name"], "acme");
    assert_eq!(manifest["version"], "1.0.0");
}

#[tokio::test]
async fn store_peek_bypassed_under_trust_policy_no_downgrade() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (mut resolver, _tempdir) = build_resolver(&registry);

    let store_dir = tempfile::TempDir::new().unwrap();
    let index = pnpm_store_dir::StoreIndex::open(store_dir.path()).unwrap();
    let integrity_str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
    let pkg_id = "acme@1.0.0";
    let key = pnpm_store_dir::store_index_key(integrity_str, pkg_id);
    index
        .set(
            &key,
            &pnpm_store_dir::PackageFilesIndex {
                manifest: Some(json!({
                    "name": "acme",
                    "version": "1.0.0",
                })),
                ..Default::default()
            },
        )
        .unwrap();

    resolver.store_index = Some(Arc::new(std::sync::Mutex::new(index)));

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let opts = ResolveOptions {
        refresh: pnpm_resolving_resolver_base::ResolutionRefreshOptions {
            current_pkg: Some(CurrentPkg {
                id: PkgResolutionId::from(pkg_id),
                name: Some("acme".to_string()),
                version: Some("1.0.0".to_string()),
                resolution: LockfileResolution::Tarball(pnpm_lockfile::TarballResolution {
                    tarball: "https://registry/acme-1.0.0.tgz".to_string(),
                    integrity: Some(integrity_str.parse().unwrap()),
                    revision: None,
                    git_hosted: None,
                    path: None,
                }),
                published_at: None,
                manifest: None,
            }),
            update: UpdateBehavior::Off,
            ..Default::default()
        },
        policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
            trust_policy: Some(TrustPolicy::NoDowngrade),
            ..Default::default()
        },
        ..ResolveOptions::default()
    };

    let _ = resolver.resolve(&wanted, &opts).await;
    mock.assert_async().await;
}

#[tokio::test]
async fn store_peek_bypassed_when_version_guard_configured() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (mut resolver, _tempdir) = build_resolver(&registry);

    let store_dir = tempfile::TempDir::new().unwrap();
    let index = pnpm_store_dir::StoreIndex::open(store_dir.path()).unwrap();
    let integrity_str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
    let pkg_id = "acme@1.0.0";
    let key = pnpm_store_dir::store_index_key(integrity_str, pkg_id);
    index
        .set(
            &key,
            &pnpm_store_dir::PackageFilesIndex {
                manifest: Some(json!({
                    "name": "acme",
                    "version": "1.0.0",
                })),
                ..Default::default()
            },
        )
        .unwrap();

    resolver.store_index = Some(Arc::new(std::sync::Mutex::new(index)));

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let opts = ResolveOptions {
        refresh: pnpm_resolving_resolver_base::ResolutionRefreshOptions {
            current_pkg: Some(CurrentPkg {
                id: PkgResolutionId::from(pkg_id),
                name: Some("acme".to_string()),
                version: Some("1.0.0".to_string()),
                resolution: LockfileResolution::Tarball(pnpm_lockfile::TarballResolution {
                    tarball: "https://registry/acme-1.0.0.tgz".to_string(),
                    integrity: Some(integrity_str.parse().unwrap()),
                    revision: None,
                    git_hosted: None,
                    path: None,
                }),
                published_at: None,
                manifest: None,
            }),
            update: UpdateBehavior::Off,
            ..Default::default()
        },
        policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
            package_version_guard: Some(reject_versions(&["1.0.0"])),
            ..Default::default()
        },
        ..ResolveOptions::default()
    };

    let result = resolver
        .resolve(&wanted, &opts)
        .await
        .unwrap()
        .expect("should resolve allowed version");
    let name_ver = result.package.name_ver.as_ref().expect("name_ver");
    assert_eq!(name_ver.suffix.to_string(), "1.1.0");
    mock.assert_async().await;
}

#[tokio::test]
async fn store_peek_bypassed_when_cached_version_does_not_satisfy_spec() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (mut resolver, _tempdir) = build_resolver(&registry);

    let store_dir = tempfile::TempDir::new().unwrap();
    let index = pnpm_store_dir::StoreIndex::open(store_dir.path()).unwrap();
    let integrity_str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
    let pkg_id = "acme@1.0.0";
    let key = pnpm_store_dir::store_index_key(integrity_str, pkg_id);
    index
        .set(
            &key,
            &pnpm_store_dir::PackageFilesIndex {
                manifest: Some(json!({
                    "name": "acme",
                    "version": "1.0.0",
                })),
                ..Default::default()
            },
        )
        .unwrap();

    resolver.store_index = Some(Arc::new(std::sync::Mutex::new(index)));

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.1.0".to_string()),
        ..WantedDependency::default()
    };
    let opts = ResolveOptions {
        refresh: pnpm_resolving_resolver_base::ResolutionRefreshOptions {
            current_pkg: Some(CurrentPkg {
                id: PkgResolutionId::from(pkg_id),
                name: Some("acme".to_string()),
                version: Some("1.0.0".to_string()),
                resolution: LockfileResolution::Tarball(pnpm_lockfile::TarballResolution {
                    tarball: "https://registry/acme-1.0.0.tgz".to_string(),
                    integrity: Some(integrity_str.parse().unwrap()),
                    revision: None,
                    git_hosted: None,
                    path: None,
                }),
                published_at: None,
                manifest: None,
            }),
            update: UpdateBehavior::Off,
            ..Default::default()
        },
        ..ResolveOptions::default()
    };

    let result = resolver
        .resolve(&wanted, &opts)
        .await
        .unwrap()
        .expect("should resolve satisfying version from registry");
    let name_ver = result.package.name_ver.as_ref().expect("name_ver");
    assert_eq!(name_ver.suffix.to_string(), "1.1.0");
    mock.assert_async().await;
}

#[tokio::test]
async fn store_peek_bypassed_when_update_checksums_is_true() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (mut resolver, _tempdir) = build_resolver(&registry);

    let store_dir = tempfile::TempDir::new().unwrap();
    let index = pnpm_store_dir::StoreIndex::open(store_dir.path()).unwrap();
    let integrity_str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
    let pkg_id = "acme@1.0.0";
    let key = pnpm_store_dir::store_index_key(integrity_str, pkg_id);
    index
        .set(
            &key,
            &pnpm_store_dir::PackageFilesIndex {
                manifest: Some(json!({
                    "name": "acme",
                    "version": "1.0.0",
                })),
                ..Default::default()
            },
        )
        .unwrap();

    resolver.store_index = Some(Arc::new(std::sync::Mutex::new(index)));

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let opts = ResolveOptions {
        refresh: pnpm_resolving_resolver_base::ResolutionRefreshOptions {
            current_pkg: Some(CurrentPkg {
                id: PkgResolutionId::from(pkg_id),
                name: Some("acme".to_string()),
                version: Some("1.0.0".to_string()),
                resolution: LockfileResolution::Tarball(pnpm_lockfile::TarballResolution {
                    tarball: "https://registry/acme-1.0.0.tgz".to_string(),
                    integrity: Some(integrity_str.parse().unwrap()),
                    revision: None,
                    git_hosted: None,
                    path: None,
                }),
                published_at: None,
                manifest: None,
            }),
            update: UpdateBehavior::Off,
            update_checksums: true,
            ..Default::default()
        },
        ..ResolveOptions::default()
    };

    let result = resolver
        .resolve(&wanted, &opts)
        .await
        .unwrap()
        .expect("should resolve from registry when update_checksums is true");
    let name_ver = result.package.name_ver.as_ref().expect("name_ver");
    assert_eq!(name_ver.suffix.to_string(), "1.0.0");
    let registry_manifest: serde_json::Value = serde_json::from_str(PACKAGE_BODY).unwrap();
    let registry_integrity = registry_manifest["versions"]["1.0.0"]["dist"]["integrity"]
        .as_str()
        .expect("registry integrity");
    assert_ne!(registry_integrity, integrity_str);
    let LockfileResolution::Tarball(tarball) = &result.resolution else {
        panic!("expected tarball resolution");
    };
    assert_eq!(
        tarball.integrity
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some(registry_integrity),
    );
    assert_eq!(
        result.package.manifest
            .as_ref()
            .and_then(|manifest| manifest["dist"]["integrity"].as_str()),
        Some(registry_integrity),
    );
    mock.assert_async().await;
}

#[test]
fn normalize_tarball_url_strips_default_ports() {
    use crate::normalize_tarball_url;
    let cases = [
        (
            "https://registry.example.com:443/package.tgz",
            "https://registry.example.com/package.tgz",
        ),
        ("http://registry.example.com:80/package.tgz", "http://registry.example.com/package.tgz"),
        (
            "https://registry.example.com:8443/package.tgz",
            "https://registry.example.com:8443/package.tgz",
        ),
        (
            "http://registry.example.com:8080/package.tgz",
            "http://registry.example.com:8080/package.tgz",
        ),
        ("https://registry.example.com/package.tgz", "https://registry.example.com/package.tgz"),
        ("http://registry.example.com/package.tgz", "http://registry.example.com/package.tgz"),
        (
            "https://artifactory:443/api/npm/npm-virtual/uuid/-/uuid-9.0.1.tgz",
            "https://artifactory/api/npm/npm-virtual/uuid/-/uuid-9.0.1.tgz",
        ),
        ("invalid-url", "invalid-url"),
    ];
    for (input, expected) in cases {
        assert_eq!(normalize_tarball_url(input), expected, "failed for input {input}");
    }
}

#[tokio::test]
async fn resolve_strips_default_ports_from_tarball_urls() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let body = json!({
        "name": "port-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "port-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry.npmjs.org:443/port-pkg/-/port-pkg-1.0.0.tgz"
                }
            }
        }
    });
    let mock = server
        .mock("GET", "/port-pkg")
        .with_status(200)
        .with_body(body.to_string())
        .create_async()
        .await;
    let (resolver, _tempdir) = build_resolver(&registry);
    let wanted = WantedDependency {
        alias: Some("port-pkg".to_string()),
        bare_specifier: Some("1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let opts = ResolveOptions::default();
    let result = resolver
        .resolve(&wanted, &opts)
        .await
        .unwrap()
        .unwrap();
    let LockfileResolution::Tarball(tarball) = &result.resolution else {
        panic!("expected tarball resolution");
    };
    assert_eq!(tarball.tarball, "https://registry.npmjs.org/port-pkg/-/port-pkg-1.0.0.tgz");
    mock.assert_async().await;
}

#[tokio::test]
async fn resolve_strips_port_80_from_http_tarball_urls() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let body = json!({
        "name": "http-port-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "http-port-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "http://registry.example.com:80/http-port-pkg/-/http-port-pkg-1.0.0.tgz"
                }
            }
        }
    });
    let mock = server
        .mock("GET", "/http-port-pkg")
        .with_status(200)
        .with_body(body.to_string())
        .create_async()
        .await;
    let (resolver, _tempdir) = build_resolver(&registry);
    let wanted = WantedDependency {
        alias: Some("http-port-pkg".to_string()),
        bare_specifier: Some("1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let opts = ResolveOptions::default();
    let result = resolver
        .resolve(&wanted, &opts)
        .await
        .unwrap()
        .unwrap();
    let LockfileResolution::Tarball(tarball) = &result.resolution else {
        panic!("expected tarball resolution");
    };
    assert_eq!(
        tarball.tarball,
        "http://registry.example.com/http-port-pkg/-/http-port-pkg-1.0.0.tgz",
    );
    mock.assert_async().await;
}
