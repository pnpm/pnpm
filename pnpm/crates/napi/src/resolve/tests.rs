//! Offline coverage for the [`resolveDependency`](super::resolve_dependency)
//! resolver chain. Every case here resolves through the local-filesystem
//! branch of the chain (or exhausts it), so no registry / git / network
//! access is required — the runtime resolvers never run for a `file:` /
//! `link:` spec because the local-scheme resolver claims it first.

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
