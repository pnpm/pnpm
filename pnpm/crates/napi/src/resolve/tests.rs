//! Coverage for the [`resolveDependency`](super::resolve_dependency)
//! resolver chain that needs no outside network. Most cases resolve
//! through the local-filesystem branch of the chain (or exhaust it) — the
//! runtime resolvers never run for a `file:` / `link:` spec because the
//! local-scheme resolver claims it first; the registry cases point the
//! default registry at a `mockito` server on localhost.

use std::collections::HashMap;

use super::{ResolveDependencyOptions, WantedDependencyInput, run_resolve_blocking};

/// Options anchored at `dir`, pinned `offline` so a stray registry-shaped
/// spec can't reach the network from a unit test.
fn options_for(dir: &std::path::Path) -> ResolveDependencyOptions {
    ResolveDependencyOptions {
        dir: dir.display().to_string(),
        store_dir: None,
        cache_dir: None,
        registries: Some(HashMap::from([(
            "default".to_string(),
            "https://registry.npmjs.org/".to_string(),
        )])),
        full_metadata: None,
        offline: Some(true),
        prefer_offline: None,
        auth_header_by_uri: None,
    }
}

/// Write a package directory with the given `name` / `version` under
/// `parent` and return its path.
fn write_package(parent: &std::path::Path, name: &str, version: &str) -> std::path::PathBuf {
    let pkg = parent.join(name);
    std::fs::create_dir(&pkg).expect("create package dir");
    std::fs::write(
        pkg.join("package.json"),
        format!(r#"{{"name":"{name}","version":"{version}"}}"#),
    )
    .expect("write package.json");
    pkg
}

#[test]
fn resolves_a_local_directory_via_the_file_scheme() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(dir.path(), "local-pkg", "1.2.3");

    let wanted = WantedDependencyInput {
        alias: None,
        bare_specifier: Some(format!("file:{}", pkg.display())),
    };
    let result = run_resolve_blocking(wanted, &options_for(dir.path())).expect("resolve file: dep");

    let manifest = result.manifest.expect("file: resolution carries a manifest");
    assert_eq!(manifest["name"], "local-pkg");
    assert_eq!(manifest["version"], "1.2.3");
    assert_eq!(result.resolved_via, "local-filesystem");
    dbg!(&result.normalized_bare_specifier);
    assert!(result.normalized_bare_specifier.is_some());
}

#[test]
fn resolves_a_local_directory_via_the_link_scheme() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(dir.path(), "linked-pkg", "4.5.6");

    let wanted = WantedDependencyInput {
        alias: None,
        bare_specifier: Some(format!("link:{}", pkg.display())),
    };
    let result = run_resolve_blocking(wanted, &options_for(dir.path())).expect("resolve link: dep");

    let manifest = result.manifest.expect("link: resolution carries a manifest");
    assert_eq!(manifest["name"], "linked-pkg");
    assert_eq!(manifest["version"], "4.5.6");
    assert_eq!(result.resolved_via, "local-filesystem");
}

#[test]
fn errors_when_no_resolver_in_the_chain_claims_the_spec() {
    let dir = tempfile::tempdir().expect("tempdir");

    // No alias and no bare specifier: every resolver in the chain declines,
    // so the dispatcher raises `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER` rather
    // than the old npm-only "not claimed" string.
    let wanted = WantedDependencyInput { alias: None, bare_specifier: None };
    let Err(error) = run_resolve_blocking(wanted, &options_for(dir.path())) else {
        panic!("an unclaimed spec should error rather than resolve");
    };

    eprintln!("resolve error: {}", error.reason);
    assert!(
        error.reason.contains("isn't supported by any available resolver"),
        "unexpected error message: {}",
        error.reason,
    );
}

/// A packument whose version object carries a registry-custom field
/// (`componentId` is Bit's) outside the abbreviated field set.
const CUSTOM_FIELD_PACKAGE_BODY: &str = r#"{
    "name": "custom-field-pkg",
    "dist-tags": { "latest": "1.0.0" },
    "modified": "2024-01-15T12:00:00.000Z",
    "time": { "1.0.0": "2024-01-10T08:30:00.000Z" },
    "versions": {
        "1.0.0": {
            "name": "custom-field-pkg",
            "version": "1.0.0",
            "componentId": { "scope": "acme.ui", "name": "button", "version": "1.0.0" },
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/custom-field-pkg-1.0.0.tgz"
            }
        }
    }
}"#;

#[test]
fn full_metadata_keeps_registry_custom_version_fields() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/custom-field-pkg")
        .with_status(200)
        .with_body(CUSTOM_FIELD_PACKAGE_BODY)
        .create();

    let dir = tempfile::tempdir().expect("tempdir");
    let cache_dir = tempfile::tempdir().expect("cache tempdir");
    let options = ResolveDependencyOptions {
        dir: dir.path().display().to_string(),
        store_dir: None,
        cache_dir: Some(cache_dir.path().display().to_string()),
        registries: Some(HashMap::from([("default".to_string(), format!("{}/", server.url()))])),
        full_metadata: Some(true),
        offline: None,
        prefer_offline: None,
        auth_header_by_uri: None,
    };
    let wanted = WantedDependencyInput {
        alias: Some("custom-field-pkg".to_string()),
        bare_specifier: Some("1.0.0".to_string()),
    };

    let result = run_resolve_blocking(wanted, &options).expect("resolve registry dep");

    let manifest = result.manifest.expect("registry resolution carries a manifest");
    dbg!(&manifest);
    assert_eq!(manifest["componentId"]["name"], "button");
    mock.assert();
}
