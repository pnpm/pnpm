use pretty_assertions::assert_eq;
use serde_json::json;

use crate::VersionsByPackageName;

use super::collect_npm_lockfile_versions;

fn collect(lockfile: &serde_json::Value) -> Vec<(String, Vec<String>)> {
    let mut versions = VersionsByPackageName::new();
    collect_npm_lockfile_versions(lockfile, &mut versions);
    versions
        .into_iter()
        .map(|(name, versions)| (name, versions.into_iter().collect()))
        .collect()
}

#[test]
fn nested_format_walks_the_whole_tree() {
    let versions = collect(&json!({
        "lockfileVersion": 1,
        "dependencies": {
            "@pnpm.e2e/dep-of-pkg-with-1-dep": { "version": "101.0.0" },
            "@pnpm.e2e/pkg-with-1-dep": {
                "version": "100.0.0",
                "dependencies": {
                    "@pnpm.e2e/dep-of-pkg-with-1-dep": { "version": "100.0.0" },
                },
            },
        },
    }));
    assert_eq!(
        versions,
        vec![
            (
                "@pnpm.e2e/dep-of-pkg-with-1-dep".to_string(),
                vec!["100.0.0".to_string(), "101.0.0".to_string()]
            ),
            ("@pnpm.e2e/pkg-with-1-dep".to_string(), vec!["100.0.0".to_string()]),
        ],
    );
}

#[test]
fn flat_format_reads_names_from_node_modules_paths() {
    let versions = collect(&json!({
        "lockfileVersion": 3,
        "packages": {
            "": { "name": "project", "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "*" } },
            "node_modules/@pnpm.e2e/pkg-with-1-dep": {
                "version": "100.0.0",
                "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0" },
            },
            "node_modules/@pnpm.e2e/pkg-with-1-dep/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep": {
                "version": "100.1.0",
            },
        },
    }));
    assert_eq!(
        versions,
        vec![
            (
                "@pnpm.e2e/dep-of-pkg-with-1-dep".to_string(),
                vec!["100.0.0".to_string(), "100.1.0".to_string()]
            ),
            ("@pnpm.e2e/pkg-with-1-dep".to_string(), vec!["*".to_string(), "100.0.0".to_string()]),
        ],
    );
}

#[test]
fn a_missing_lockfile_version_is_read_as_the_flat_format() {
    let versions = collect(&json!({
        "packages": {
            "node_modules/is-positive": { "version": "1.0.0" },
        },
        "dependencies": {
            "is-negative": { "version": "2.1.0" },
        },
    }));
    assert_eq!(
        versions,
        vec![
            ("is-negative".to_string(), vec!["2.1.0".to_string()]),
            ("is-positive".to_string(), vec!["1.0.0".to_string()]),
        ],
    );
}

#[test]
fn entries_without_a_version_are_skipped() {
    let versions = collect(&json!({
        "lockfileVersion": 3,
        "packages": {
            "": {},
            "node_modules/link-target": { "resolved": "../link-target", "link": true },
        },
    }));
    assert!(versions.is_empty(), "got {versions:?}");
}

#[test]
fn nested_format_unwraps_npm_alias_versions() {
    let versions = collect(&json!({
        "lockfileVersion": 1,
        "dependencies": {
            "my-alias": { "version": "npm:is-positive@1.0.0" },
            "parent": {
                "version": "1.0.0",
                "dependencies": {
                    "@my/alias": { "version": "npm:@pnpm.e2e/foo@1.0.0" },
                    "self-npm": { "version": "npm:2.0.0" },
                },
            },
        },
    }));
    assert_eq!(
        versions,
        vec![
            ("@pnpm.e2e/foo".to_string(), vec!["1.0.0".to_string()]),
            ("is-positive".to_string(), vec!["1.0.0".to_string()]),
            ("parent".to_string(), vec!["1.0.0".to_string()]),
            ("self-npm".to_string(), vec!["2.0.0".to_string()]),
        ],
    );
}

#[test]
fn flat_format_reads_npm_alias_target_names_and_skips_root_name() {
    let versions = collect(&json!({
        "lockfileVersion": 3,
        "packages": {
            "": {
                "name": "project",
                "version": "0.0.0",
                "dependencies": {
                    "my-alias": "npm:is-positive@^1.0.0",
                    "bare-alias": "npm:is-negative",
                },
            },
            "node_modules/my-alias": { "name": "is-positive", "version": "1.0.0" },
            "node_modules/bare-alias": { "name": "is-negative", "version": "2.1.0" },
            "node_modules/parent": {
                "version": "1.0.0",
                "dependencies": { "nested-alias": "npm:@pnpm.e2e/foo@1.0.0" },
            },
            "node_modules/parent/node_modules/@my/nested-alias": {
                "name": "@pnpm.e2e/foo",
                "version": "1.1.0",
            },
        },
    }));
    assert_eq!(
        versions,
        vec![
            ("@pnpm.e2e/foo".to_string(), vec!["1.0.0".to_string(), "1.1.0".to_string()]),
            ("is-negative".to_string(), vec!["2.1.0".to_string()]),
            ("is-positive".to_string(), vec!["1.0.0".to_string(), "^1.0.0".to_string()]),
            ("parent".to_string(), vec!["1.0.0".to_string()]),
        ],
    );
}
