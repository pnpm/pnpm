use super::{extract_version_manifest, rewrite_tarball_urls};
use crate::package_name::PackageName;
use serde_json::json;

#[test]
fn rewrites_npm_form_tarball() {
    let mut doc = json!({
        "name": "foo",
        "versions": {
            "1.0.0": {
                "dist": {
                    "tarball": "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz",
                    "shasum": "abc"
                }
            }
        }
    });
    let name = PackageName::parse("foo").unwrap();
    rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:4873");
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["tarball"],
        "http://127.0.0.1:4873/foo/-/foo-1.0.0.tgz",
    );
    assert_eq!(doc["versions"]["1.0.0"]["dist"]["shasum"], "abc");
}

#[test]
fn rewrites_verdaccio_form_tarball_for_scoped() {
    // Verdaccio publishes scoped tarball URLs like
    // `/@scope/name/-/@scope/name-1.0.0.tgz` — the scope is
    // present twice. We only care about the basename.
    let mut doc = json!({
        "versions": {
            "1.0.0": {
                "dist": {
                    "tarball": "http://localhost:4873/@foo/no-deps/-/@foo/no-deps-1.0.0.tgz"
                }
            }
        }
    });
    let name = PackageName::parse("@foo/no-deps").unwrap();
    rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:9999");
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["tarball"],
        "http://127.0.0.1:9999/@foo/no-deps/-/no-deps-1.0.0.tgz",
    );
}

#[test]
fn handles_packument_without_versions() {
    let mut doc = json!({ "name": "foo" });
    let name = PackageName::parse("foo").unwrap();
    rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:4873");
    assert_eq!(doc, json!({ "name": "foo" }));
}

#[test]
fn extracts_version_by_dist_tag() {
    let doc = json!({
        "name": "@foo/no-deps",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "@foo/no-deps",
                "version": "1.0.0",
                "dist": {
                    "tarball": "http://localhost:4873/@foo/no-deps/-/@foo/no-deps-1.0.0.tgz",
                    "shasum": "abc"
                }
            }
        }
    });
    let name = PackageName::parse("@foo/no-deps").unwrap();
    let manifest = extract_version_manifest(&doc, &name, "latest", "http://reg").unwrap();
    assert_eq!(manifest["version"], "1.0.0");
    assert_eq!(manifest["dist"]["tarball"], "http://reg/@foo/no-deps/-/no-deps-1.0.0.tgz");
    assert_eq!(manifest["dist"]["shasum"], "abc");
}

#[test]
fn extracts_version_by_literal_version() {
    let doc = json!({
        "name": "foo",
        "versions": { "2.0.0": { "version": "2.0.0", "dist": { "tarball": "x/foo-2.0.0.tgz" } } }
    });
    let name = PackageName::parse("foo").unwrap();
    let manifest = extract_version_manifest(&doc, &name, "2.0.0", "http://reg").unwrap();
    assert_eq!(manifest["version"], "2.0.0");
    assert_eq!(manifest["dist"]["tarball"], "http://reg/foo/-/foo-2.0.0.tgz");
}

#[test]
fn extract_returns_none_for_unknown_version() {
    let doc = json!({
        "versions": { "1.0.0": { "dist": { "tarball": "x/foo-1.0.0.tgz" } } }
    });
    let name = PackageName::parse("foo").unwrap();
    assert!(extract_version_manifest(&doc, &name, "9.9.9", "http://reg").is_none());
    assert!(extract_version_manifest(&doc, &name, "latest", "http://reg").is_none());
}
