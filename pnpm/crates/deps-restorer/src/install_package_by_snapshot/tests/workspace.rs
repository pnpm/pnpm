use super::super::local_file_tarball_install_url;
use crate::install_package_by_snapshot::runtime::{archive_filter_for, node_extras_filter};
use pnpm_lockfile::PackageKey;
use pretty_assertions::assert_eq;
use std::borrow::Cow;

#[test]
fn local_file_tarball_install_url_resolves_relative_specs_against_workspace_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace_root = tmp.path().join("deploy");
    let actual =
        local_file_tarball_install_url(Cow::Borrowed("file:../vendor/pkg.tgz"), &workspace_root);
    let expected = pnpm_fs::lexical_normalize(&workspace_root.join("../vendor/pkg.tgz"));

    assert_eq!(actual.as_ref(), format!("file:{}", expected.display()));
}
/// Pin each branch of the alternation, including the negative
/// cases the regex deliberately doesn't match — a regression
/// (e.g. matching `lib/node_modules/yarn/...` because someone
/// forgot the `npm|corepack` alternation) would slip past tests
/// that only checked positive matches.
#[test]
fn node_extras_filter_matches_upstream_regex_alternations() {
    // Branch 1: `^(?:lib/)?node_modules/(?:npm|corepack)(?:/|$)`
    for path in [
        "lib/node_modules/npm",
        "lib/node_modules/npm/",
        "lib/node_modules/npm/package.json",
        "lib/node_modules/corepack",
        "lib/node_modules/corepack/dist/manager.js",
        "node_modules/npm",
        "node_modules/npm/package.json",
        "node_modules/corepack/dist/manager.js",
    ] {
        assert!(node_extras_filter(path), "expected match: {path}");
    }
    for path in [
        "lib/node_modules/yarn",
        "lib/node_modules/yarn/package.json",
        "node_modules/yarn",
        "node_modules/typescript/lib/tsc.js",
        "src/node_modules/npm/foo",
    ] {
        assert!(!node_extras_filter(path), "expected no match: {path}");
    }

    // Branch 2: `^bin/(?:npm|npx|corepack)$`
    for path in ["bin/npm", "bin/npx", "bin/corepack"] {
        assert!(node_extras_filter(path), "expected match: {path}");
    }
    for path in ["bin/npm/foo", "bin/npm.cmd", "bin/yarn", "bin/", "binnpm"] {
        assert!(!node_extras_filter(path), "expected no match: {path}");
    }

    // Branch 3: `^(?:npm|npx|corepack)(?:\.(?:cmd|ps1))?$`
    for path in [
        "npm",
        "npx",
        "corepack",
        "npm.cmd",
        "npx.cmd",
        "corepack.cmd",
        "npm.ps1",
        "npx.ps1",
        "corepack.ps1",
    ] {
        assert!(node_extras_filter(path), "expected match: {path}");
    }
    for path in ["npm.bat", "npm.exe", "node", "yarn", "npmrc", "npm.cmd.bak"] {
        assert!(!node_extras_filter(path), "expected no match: {path}");
    }
}
#[test]
fn archive_filter_for_only_returns_filter_for_unscoped_node() {
    let key_node: PackageKey = "node@22.0.0".parse().expect("parse node key");
    assert!(archive_filter_for(&key_node).is_some(), "node must get the filter");

    let key_scoped_node: PackageKey = "@foo/node@22.0.0".parse().expect("parse @foo/node key");
    assert!(
        archive_filter_for(&key_scoped_node).is_none(),
        "scoped `@foo/node` must not get the filter; upstream `archiveFilters` is keyed by pkg.name and only matches the unscoped string `node`",
    );

    let key_react: PackageKey = "react@18.0.0".parse().expect("parse react key");
    assert!(archive_filter_for(&key_react).is_none());

    let key_bun: PackageKey = "bun@1.0.0".parse().expect("parse bun key");
    assert!(
        archive_filter_for(&key_bun).is_none(),
        "bun runtime has no bundled-tooling filter upstream (yet); leaving it `None` matches",
    );
}
