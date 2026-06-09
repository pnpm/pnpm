use std::sync::Arc;

use pacquet_network::ThrottledClient;
use pacquet_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};
use pretty_assertions::assert_eq;

use super::{NodeResolver, bin_spec_for_platform, parse_node_file_name};

fn resolver() -> NodeResolver {
    NodeResolver::new(Arc::new(ThrottledClient::new_for_installs()))
}

/// A `WantedDependency` whose alias is not `node` is declined (the
/// dispatcher chain falls through to the next resolver).
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

/// `offline=true` raises `NO_OFFLINE_NODEJS_RESOLUTION` so the install
/// stops with the same code pnpm emits.
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

/// Tarball filename pattern parsing — exercises every branch the
/// regex covers upstream (Linux glibc, Linux musl, Windows zip, the
/// unrecognised `.pkg` reject case).
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

/// A variant's `bin` is a named map keyed by the executable name — pnpm
/// writes `bin: { node: bin/node }` on unix and `bin: { node: node.exe }`
/// on win32, never a bare string. Mirrors the `variants[].resolution.bin`
/// block in pnpm's runtime lockfile entry.
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
