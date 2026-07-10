use std::sync::Arc;

use pacquet_network::ThrottledClient;
use pacquet_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};
use pretty_assertions::assert_eq;

use super::{
    NodeResolver, NodeResolverError, bin_spec_for_platform,
    normalize_node_runtime_version_specifier, parse_node_file_name, read_node_assets_from_mirror,
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
        Some("NO_OFFLINE_NODEJS_RESOLUTION"),
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
    use pacquet_lockfile::BinarySpec;
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
    )
    .await
    .expect_err("stable release assets must require a SHASUMS signature");
    let err = err.downcast_ref::<NodeResolverError>().expect("NodeResolverError");

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
    )
    .await
    .expect("unsigned channels use the raw SHASUMS file");

    assert_eq!(assets.len(), 1);
}

const SHASUMS_WITH_ONE_NODE_ASSET: &str = "\
ed52239294ad517fbe91a268146d5d2aa8a17d2d62d64873e43219078ba71c4e  node-v22.11.0-linux-x64.tar.gz
";
