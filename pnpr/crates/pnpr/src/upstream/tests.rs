use super::{abbreviate_packument, extract_version_manifest, rewrite_tarball_urls};
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

#[test]
fn abbreviation_drops_fields_the_resolver_ignores() {
    let doc = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "modified": "2020-01-01T00:00:00.000Z", "1.0.0": "2019-01-01T00:00:00.000Z" },
        "_id": "foo",
        "_rev": "1-abc",
        "readme": "# foo\nlots of prose",
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dependencies": { "bar": "^1.0.0" },
                "devDependencies": { "jest": "^29.0.0" },
                "peerDependencies": { "react": "*" },
                "os": ["linux"],
                "cpu": ["x64"],
                "libc": ["glibc"],
                "funding": { "url": "https://example.com" },
                "acceptDependencies": { "bar": "^1.0.0" },
                "_hasShrinkwrap": false,
                "hasInstallScript": true,
                "dist": {
                    "tarball": "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz",
                    "integrity": "sha512-abc",
                    "shasum": "deadbeef"
                }
            }
        }
    });

    let out = abbreviate_packument(&doc);

    // Top-level: prose and registry bookkeeping gone, `modified`
    // synthesized from `time.modified`, `time` retained.
    assert!(out.get("readme").is_none());
    assert!(out.get("readmeFilename").is_none());
    assert!(out.get("_id").is_none());
    assert!(out.get("_rev").is_none());
    assert_eq!(out["modified"], "2020-01-01T00:00:00.000Z");
    assert!(out.get("time").is_some());

    let version = &out["versions"]["1.0.0"];
    // Resolver-relevant fields kept.
    assert_eq!(version["name"], "foo");
    assert_eq!(version["dependencies"]["bar"], "^1.0.0");
    assert_eq!(version["peerDependencies"]["react"], "*");
    assert_eq!(version["hasInstallScript"], true);
    // Platform-filtering fields kept for optional-dep selection (`#9950`).
    assert_eq!(version["os"][0], "linux");
    assert_eq!(version["cpu"][0], "x64");
    assert_eq!(version["libc"][0], "glibc");
    // Ignored fields dropped.
    assert!(version.get("devDependencies").is_none());
    assert!(version.get("funding").is_none());
    assert!(version.get("acceptDependencies").is_none());
    assert!(version.get("_hasShrinkwrap").is_none());
    // `shasum` dropped because `integrity` is present.
    assert_eq!(version["dist"]["integrity"], "sha512-abc");
    assert!(version["dist"].get("shasum").is_none());
}

#[test]
fn abbreviation_keeps_shasum_when_integrity_absent() {
    let doc = json!({
        "name": "legacy",
        "versions": {
            "0.0.1": {
                "name": "legacy",
                "version": "0.0.1",
                "dist": {
                    "tarball": "https://registry.npmjs.org/legacy/-/legacy-0.0.1.tgz",
                    "shasum": "deadbeef"
                }
            }
        }
    });

    let out = abbreviate_packument(&doc);

    let dist = &out["versions"]["0.0.1"]["dist"];
    assert!(dist.get("integrity").is_none());
    assert_eq!(dist["shasum"], "deadbeef");
}
