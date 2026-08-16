use std::sync::Arc;

use pnpm_network::ThrottledClient;
use pnpm_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};
use pretty_assertions::assert_eq;

use super::{
    NodeResolver, NodeResolverError, bin_spec_for_platform, exact_release_version,
    normalize_node_runtime_version_specifier, parse_node_file_name, parse_node_specifier,
    read_node_assets_from_mirror,
};

fn resolver() -> NodeResolver {
    NodeResolver::new(Arc::new(ThrottledClient::new_for_installs()))
}

#[tokio::test]
async fn declines_non_node_alias() {
    let wanted = WantedDependency {
        alias: Some("foo".to_string()),
        bare_specifier: Some("runtime:22.0.0".to_string()),
        ..WantedDependency::default()
    };
    let outcome = resolver().resolve(&wanted, &ResolveOptions::default()).await.unwrap();
    assert!(outcome.is_none());
}

/// `node` alias without a `runtime:` prefix is declined — that shape
/// is owned by the npm resolver (`node` could be a package name too).
#[tokio::test]
async fn declines_node_without_runtime_prefix() {
    let wanted = WantedDependency {
        alias: Some("node".to_string()),
        bare_specifier: Some("^22".to_string()),
        ..WantedDependency::default()
    };
    let outcome = resolver().resolve(&wanted, &ResolveOptions::default()).await.unwrap();
    assert!(outcome.is_none());
}

#[tokio::test]
async fn offline_raises_no_offline_nodejs_resolution() {
    let mut resolver = resolver();
    resolver.offline = true;
    let wanted = WantedDependency {
        alias: Some("node".to_string()),
        bare_specifier: Some("runtime:22.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap_err();
    let code: &dyn miette::Diagnostic =
        err.downcast_ref::<super::NodeResolverError>().expect("error is a NodeResolverError");
    assert_eq!(
        code.code().map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_NO_OFFLINE_NODEJS_RESOLUTION"),
    );
}

#[test]
fn parses_node_file_names() {
    let version = "22.0.0";
    let linux =
        parse_node_file_name("node-v22.0.0-linux-x64.tar.gz", version).expect("linux glibc");
    assert_eq!(linux.platform, "linux");
    assert_eq!(linux.arch, "x64");
    assert!(!linux.is_musl);

    let musl =
        parse_node_file_name("node-v22.0.0-linux-x64-musl.tar.gz", version).expect("linux musl");
    assert_eq!(musl.platform, "linux");
    assert_eq!(musl.arch, "x64");
    assert!(musl.is_musl);

    let windows = parse_node_file_name("node-v22.0.0-win-x64.zip", version).expect("windows");
    assert_eq!(windows.platform, "win");
    assert_eq!(windows.arch, "x64");
    assert!(!windows.is_musl);

    assert!(parse_node_file_name("node-v22.0.0.pkg", version).is_none());
    assert!(parse_node_file_name("node-v22.0.0-headers.tar.gz", version).is_none());
}

#[test]
fn bin_spec_is_a_named_map() {
    use pnpm_lockfile::BinarySpec;
    use std::collections::BTreeMap;

    assert_eq!(
        bin_spec_for_platform("linux"),
        BinarySpec::Map(BTreeMap::from([("node".to_string(), "bin/node".to_string())])),
    );
    assert_eq!(
        bin_spec_for_platform("win32"),
        BinarySpec::Map(BTreeMap::from([("node".to_string(), "node.exe".to_string())])),
    );
}

#[test]
fn normalized_runtime_spec_preserves_version_prefix() {
    let cases = [
        ("22", None, "22.11.0"),
        ("^22", None, "^22.11.0"),
        ("22", Some("runtime:~22.0.0"), "~22.11.0"),
        ("^22", Some("runtime:22.0.0"), "22.11.0"),
        ("rc/^22", None, "^22.11.0"),
        ("22", Some("runtime:^22.0.0-rc.0"), "^22.11.0"),
    ];
    for (version_spec, prev_specifier, expected) in cases {
        assert_eq!(
            normalize_node_runtime_version_specifier(version_spec, "22.11.0", prev_specifier),
            expected,
            "version_spec={version_spec:?}, prev_specifier={prev_specifier:?}",
        );
    }

    assert_eq!(normalize_node_runtime_version_specifier("^22", "22.0.0-rc.0", None), "22.0.0-rc.0");
}

/// `add node@runtime:<spec>` saves the picked version, not the requested
/// range — `runtime:26` must reach the manifest as `runtime:26.5.0`.
#[tokio::test]
async fn resolve_save_specifier_pins_the_picked_version() {
    let mut server = mockito::Server::new_async().await;
    let index = server
        .mock("GET", "/download/release/index.json")
        .with_status(200)
        .with_body(
            r#"[
                {"version": "v26.5.0", "lts": false},
                {"version": "v26.4.0", "lts": false},
                {"version": "v24.13.0", "lts": "Krypton"}
            ]"#,
        )
        .expect_at_least(1)
        .create_async()
        .await;
    let mut resolver = resolver();
    resolver
        .node_download_mirrors
        .insert("release".to_string(), format!("{}/download/release/", server.url()));

    let cases = [
        ("26", None, "runtime:26.5.0"),
        ("^26", None, "runtime:^26.5.0"),
        ("26", Some("runtime:~26.4.0"), "runtime:~26.5.0"),
        ("", None, "runtime:26.5.0"),
        ("lts", None, "runtime:24.13.0"),
    ];
    for (version_spec, prev_specifier, expected) in cases {
        assert_eq!(
            resolver.resolve_save_specifier(version_spec, prev_specifier).await.unwrap(),
            expected,
            "version_spec={version_spec:?}, prev_specifier={prev_specifier:?}",
        );
    }
    index.assert_async().await;
}

#[tokio::test]
async fn resolve_save_specifier_errors_when_no_version_satisfies() {
    let mut server = mockito::Server::new_async().await;
    let _index = server
        .mock("GET", "/download/release/index.json")
        .with_status(200)
        .with_body(r#"[{"version": "v26.5.0", "lts": false}]"#)
        .create_async()
        .await;
    let mut resolver = resolver();
    resolver
        .node_download_mirrors
        .insert("release".to_string(), format!("{}/download/release/", server.url()));

    let err = resolver.resolve_save_specifier("99", None).await.unwrap_err();
    let code: &dyn miette::Diagnostic = &err;
    assert_eq!(
        code.code().map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_NODEJS_VERSION_NOT_FOUND"),
    );
}

#[tokio::test]
async fn release_asset_reader_requires_signature_when_requested() {
    let mut server = mockito::Server::new_async().await;
    let _shasums = server
        .mock("GET", "/download/release/v22.11.0/SHASUMS256.txt")
        .with_status(200)
        .with_body(SHASUMS_WITH_ONE_NODE_ASSET)
        .create_async()
        .await;
    let _signature = server
        .mock("GET", "/download/release/v22.11.0/SHASUMS256.txt.sig")
        .with_status(404)
        .create_async()
        .await;
    let err = read_node_assets_from_mirror(
        &ThrottledClient::new_for_installs(),
        &format!("{}/download/release/", server.url()),
        "22.11.0",
        false,
        true,
        None,
    )
    .await
    .expect_err("stable release assets must require a SHASUMS signature");

    assert!(matches!(err, NodeResolverError::FetchVerifiedNodeShasums(_)));
}

#[tokio::test]
async fn prerelease_asset_reader_does_not_require_signature() {
    let mut server = mockito::Server::new_async().await;
    let _shasums = server
        .mock("GET", "/download/rc/v22.11.0/SHASUMS256.txt")
        .with_status(200)
        .with_body(SHASUMS_WITH_ONE_NODE_ASSET)
        .create_async()
        .await;
    let assets = read_node_assets_from_mirror(
        &ThrottledClient::new_for_installs(),
        &format!("{}/download/rc/", server.url()),
        "22.11.0",
        false,
        false,
        None,
    )
    .await
    .expect("unsigned channels use the raw SHASUMS file");

    assert_eq!(assets.len(), 1);
}

#[test]
fn exact_release_versions_are_their_own_resolution() {
    let exact = |specifier: &str| {
        exact_release_version(&parse_node_specifier(specifier).expect("valid specifier"))
    };
    assert_eq!(exact("22.11.0").as_deref(), Some("22.11.0"));
    assert_eq!(exact("release/22.11.0").as_deref(), Some("22.11.0"));
    assert_eq!(exact("^22.11.0"), None);
    assert_eq!(exact("22.11"), None);
    assert_eq!(exact("v22.11.0"), None);
    assert_eq!(exact("22.11.0-rc.1"), None);
    assert_eq!(exact("rc/22.11.0"), None);
    assert_eq!(exact("latest"), None);
    assert_eq!(exact("lts"), None);
}

/// An exact stable version is saved verbatim without consulting the
/// release index: the mirror here is unroutable, so any network access
/// would fail the call.
#[tokio::test]
async fn resolve_save_specifier_saves_an_exact_version_without_network() {
    let mut resolver = resolver();
    resolver
        .node_download_mirrors
        .insert("release".to_string(), "http://127.0.0.1:9/download/release/".to_string());

    assert_eq!(resolver.resolve_save_specifier("22.11.0", None).await.unwrap(), "runtime:22.11.0");
}

/// An exact-version resolve skips the release index, so a nonexistent
/// version first fails its asset fetch; the resolver must then consult
/// the index and raise the canonical not-found error rather than the
/// raw fetch failure.
#[tokio::test]
async fn exact_resolve_of_a_nonexistent_version_raises_version_not_found() {
    let mut server = mockito::Server::new_async().await;
    let _shasums = server
        .mock("GET", "/download/release/v22.99.0/SHASUMS256.txt")
        .with_status(404)
        .create_async()
        .await;
    let index = server
        .mock("GET", "/download/release/index.json")
        .with_status(200)
        .with_body(r#"[{"version": "v22.11.0", "lts": false}]"#)
        .expect(1)
        .create_async()
        .await;
    let mut resolver = resolver();
    resolver
        .node_download_mirrors
        .insert("release".to_string(), format!("{}/download/release/", server.url()));
    let wanted = WantedDependency {
        alias: Some("node".to_string()),
        bare_specifier: Some("runtime:22.99.0".to_string()),
        ..WantedDependency::default()
    };

    let err = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap_err();
    let code: &dyn miette::Diagnostic =
        err.downcast_ref::<NodeResolverError>().expect("error is a NodeResolverError");
    assert_eq!(
        code.code().map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_NODEJS_VERSION_NOT_FOUND"),
    );
    index.assert_async().await;
}

/// When the index confirms the exact version exists, the asset-fetch
/// failure is the real error and must surface unchanged.
#[tokio::test]
async fn exact_resolve_keeps_the_asset_error_when_the_version_exists() {
    let mut server = mockito::Server::new_async().await;
    let _shasums = server
        .mock("GET", "/download/release/v22.11.0/SHASUMS256.txt")
        .with_status(500)
        .create_async()
        .await;
    let _index = server
        .mock("GET", "/download/release/index.json")
        .with_status(200)
        .with_body(r#"[{"version": "v22.11.0", "lts": false}]"#)
        .create_async()
        .await;
    let mut resolver = resolver();
    resolver
        .node_download_mirrors
        .insert("release".to_string(), format!("{}/download/release/", server.url()));
    let wanted = WantedDependency {
        alias: Some("node".to_string()),
        bare_specifier: Some("runtime:22.11.0".to_string()),
        ..WantedDependency::default()
    };

    let err = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap_err();
    let err = err.downcast_ref::<NodeResolverError>().expect("error is a NodeResolverError");
    assert!(matches!(err, NodeResolverError::FetchVerifiedNodeShasums(_)));
}

/// A SHASUMS body cached by an earlier resolve serves the next one
/// without any network access: the mock expects exactly one hit.
#[tokio::test]
async fn asset_reader_serves_repeat_reads_from_the_cache() {
    let mut server = mockito::Server::new_async().await;
    let shasums = server
        .mock("GET", "/download/rc/v22.11.0/SHASUMS256.txt")
        .with_status(200)
        .with_body(SHASUMS_WITH_ONE_NODE_ASSET)
        .expect(1)
        .create_async()
        .await;
    let cache_dir = tempfile::tempdir().expect("create temp cache dir");
    let client = ThrottledClient::new_for_installs();
    let mirror = format!("{}/download/rc/", server.url());

    let fetched = read_node_assets_from_mirror(
        &client,
        &mirror,
        "22.11.0",
        false,
        false,
        Some(cache_dir.path()),
    )
    .await
    .expect("fetch the asset list");
    let cached = read_node_assets_from_mirror(
        &client,
        &mirror,
        "22.11.0",
        false,
        false,
        Some(cache_dir.path()),
    )
    .await
    .expect("serve the asset list from the cache");

    assert_eq!(fetched.len(), 1);
    assert_eq!(cached.len(), 1);
    shasums.assert_async().await;
}

const SHASUMS_WITH_ONE_NODE_ASSET: &str = "\
ed52239294ad517fbe91a268146d5d2aa8a17d2d62d64873e43219078ba71c4e  node-v22.11.0-linux-x64.tar.gz
";
