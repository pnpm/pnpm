use super::{
    CanonicalPackageName, abbreviate_packument, extract_version_manifest, json, now,
    rewrite_tarball_urls, rewrite_upstream_tarball_urls, tarball_basename,
};

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
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:4873");
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["tarball"],
        "http://127.0.0.1:4873/foo/-/foo-1.0.0.tgz",
    );
    assert_eq!(doc["versions"]["1.0.0"]["dist"]["shasum"], "abc");
}

#[test]
fn preserves_valid_upstream_revision_route_on_the_public_registry() {
    let current_digest = "A".repeat(86);
    let original_digest = "Q".repeat(86);
    let mut doc = json!({
        "versions": {
            "1.0.0": {
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("https://upstream.test/npm/-/tarballs/sha512/{current_digest}"),
                    "integrity": format!("sha512-{current_digest}=="),
                    "revision": 2,
                    "revisions": [{
                        "revision": 0,
                        "integrity": format!("sha512-{original_digest}=="),
                        "tarball": format!("https://upstream.test/npm/-/tarballs/sha512/{original_digest}"),
                        "manifest": { "dependencies": { "bar": "1" } },
                    }, {
                        "revision": 2,
                        "integrity": format!("sha512-{current_digest}=="),
                        "tarball": format!("https://upstream.test/npm/-/tarballs/sha512/{current_digest}"),
                        "manifest": {},
                    }],
                }
            }
        }
    });
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();

    rewrite_upstream_tarball_urls(
        &mut doc,
        &name,
        "https://upstream.test/npm/",
        "http://pnpr.test/~corp/",
    );

    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["tarball"],
        format!("http://pnpr.test/~corp/-/tarballs/sha512/{current_digest}"),
    );
    assert_eq!(doc["versions"]["1.0.0"]["dist"]["revision"], 2);
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["revisions"][0]["tarball"],
        format!("http://pnpr.test/~corp/-/tarballs/sha512/{original_digest}"),
    );
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["revisions"][1]["tarball"],
        format!("http://pnpr.test/~corp/-/tarballs/sha512/{current_digest}"),
    );
}

#[test]
fn does_not_preserve_an_invalid_upstream_revision_route() {
    let digest = "A".repeat(86);
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    for (revision, tarball) in [
        (json!(0), format!("https://upstream.test/-/tarballs/sha512/{digest}")),
        (json!(2), format!("https://other.test/-/tarballs/sha512/{digest}")),
    ] {
        let mut doc = json!({
            "version": "1.0.0",
            "dist": {
                "tarball": tarball,
                "integrity": format!("sha512-{digest}=="),
                "revision": revision,
            }
        });

        rewrite_upstream_tarball_urls(
            &mut doc,
            &name,
            "https://upstream.test/",
            "http://pnpr.test/~corp/",
        );

        assert_eq!(doc["dist"]["tarball"], format!("http://pnpr.test/~corp/foo/-/{digest}"));
        assert!(doc["dist"].get("revision").is_none());
    }
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
    let name =
        CanonicalPackageName::parse("@foo/no-deps", pnpr_package_name::Ecosystem::Npm).unwrap();
    rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:9999");
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["tarball"],
        "http://127.0.0.1:9999/@foo/no-deps/-/no-deps-1.0.0.tgz",
    );
}

#[test]
fn preserves_non_canonical_tarball_basename() {
    // A version whose tarball filename does not follow the
    // `<name>-<version>.tgz` convention (here esprima-fb's zero-padded
    // name for version `3001.1.0-dev-harmony-fb`) keeps its basename, so
    // the client records and fetches back the path the upstream hosts.
    let mut doc = json!({
        "versions": {
            "3001.1.0-dev-harmony-fb": {
                "dist": {
                    "tarball": "https://registry.npmjs.org/esprima-fb/-/esprima-fb-3001.0001.0000-dev-harmony-fb.tgz"
                }
            }
        }
    });
    let name =
        CanonicalPackageName::parse("esprima-fb", pnpr_package_name::Ecosystem::Npm).unwrap();
    rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:9999");
    assert_eq!(
        doc["versions"]["3001.1.0-dev-harmony-fb"]["dist"]["tarball"],
        "http://127.0.0.1:9999/esprima-fb/-/esprima-fb-3001.0001.0000-dev-harmony-fb.tgz",
    );
}

#[test]
fn rewrites_query_bearing_tarball_to_its_path_basename() {
    // A signed/query-bearing upstream URL must rewrite to the path basename
    // alone: the query is not part of the route a client later requests, so
    // keeping it would desync the public URL from the serve-time match.
    let mut doc = json!({
        "versions": {
            "1.0.0": {
                "dist": { "tarball": "https://cdn.example.com/foo/-/foo-1.0.0.tgz?sig=abc&exp=123" }
            }
        }
    });
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:9999");
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["tarball"],
        "http://127.0.0.1:9999/foo/-/foo-1.0.0.tgz",
    );
}

#[test]
fn rewrites_basenameless_tarball_url_to_pnpr_route() {
    // A dist.tarball with no usable basename (here a trailing slash) must
    // never be passed through to the client; it falls back to the
    // version-derived pnpr route so integrity/OSV stay enforced here and the
    // client is never directed at the upstream host.
    let mut doc = json!({
        "versions": {
            "1.0.0": {
                "version": "1.0.0",
                "dist": { "tarball": "https://evil.example.com/somewhere/" }
            }
        }
    });
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:9999");
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["tarball"],
        "http://127.0.0.1:9999/foo/-/foo-1.0.0.tgz",
    );
}

#[test]
fn tarball_basename_strips_query_and_fragment() {
    let base = "foo-1.0.0.tgz";
    assert_eq!(tarball_basename("https://r/foo/-/foo-1.0.0.tgz"), Some(base));
    assert_eq!(tarball_basename("https://r/foo/-/foo-1.0.0.tgz?sig=x"), Some(base));
    assert_eq!(tarball_basename("https://r/foo/-/foo-1.0.0.tgz#frag"), Some(base));
    assert_eq!(tarball_basename("foo-1.0.0.tgz"), Some(base));
    assert_eq!(tarball_basename("https://r/foo/"), None);
}

#[test]
fn handles_packument_without_versions() {
    let mut doc = json!({ "name": "foo" });
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
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
    let name =
        CanonicalPackageName::parse("@foo/no-deps", pnpr_package_name::Ecosystem::Npm).unwrap();
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
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let manifest = extract_version_manifest(&doc, &name, "2.0.0", "http://reg").unwrap();
    assert_eq!(manifest["version"], "2.0.0");
    assert_eq!(manifest["dist"]["tarball"], "http://reg/foo/-/foo-2.0.0.tgz");
}

#[test]
fn extract_returns_none_for_unknown_version() {
    let doc = json!({
        "versions": { "1.0.0": { "dist": { "tarball": "x/foo-1.0.0.tgz" } } }
    });
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
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
                    "shasum": "deadbeef",
                    "fileCount": 12,
                    "unpackedSize": 34567,
                    "signatures": [{ "keyid": "SHA256:xyz", "sig": "base64sig" }],
                    "npm-signature": "-----BEGIN PGP SIGNATURE-----"
                }
            }
        }
    });

    let out = abbreviate_packument(&doc, now());

    // Top-level: prose and registry bookkeeping gone, `modified`
    // synthesized from `time.modified`, `time` retained. Both `time`
    // entries predate the week-old horizon, so they coarsen to bare
    // dates.
    assert!(out.get("readme").is_none());
    assert!(out.get("readmeFilename").is_none());
    assert!(out.get("_id").is_none());
    assert!(out.get("_rev").is_none());
    assert_eq!(out["modified"], "2020-01-01");
    assert_eq!(out["time"]["modified"], "2020-01-01");
    assert_eq!(out["time"]["1.0.0"], "2019-01-01");

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
    // Legacy PGP signature dropped; ECDSA registry signatures kept.
    assert!(version["dist"].get("npm-signature").is_none());
    assert_eq!(version["dist"]["signatures"][0]["keyid"], "SHA256:xyz");
    // Size hints kept: pacquet reads both for decompression
    // preallocation and download scheduling.
    assert_eq!(version["dist"]["fileCount"], 12);
    assert_eq!(version["dist"]["unpackedSize"], 34567);
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

    let out = abbreviate_packument(&doc, now());

    let dist = &out["versions"]["0.0.1"]["dist"];
    assert!(dist.get("integrity").is_none());
    assert_eq!(dist["shasum"], "deadbeef");
}

#[test]
fn abbreviation_keeps_shasum_when_integrity_is_empty_or_non_string() {
    // pnpm's `getIntegrity` falls back to `shasum` unless `integrity`
    // is a truthy (non-empty) string, so an empty or malformed
    // `integrity` must not strip the sha1 fallback.
    let doc = json!({
        "name": "weird",
        "versions": {
            "1.0.0": {
                "version": "1.0.0",
                "dist": { "tarball": "x/weird-1.0.0.tgz", "integrity": "", "shasum": "deadbeef" }
            },
            "2.0.0": {
                "version": "2.0.0",
                "dist": { "tarball": "x/weird-2.0.0.tgz", "integrity": false, "shasum": "cafe" }
            }
        }
    });

    let out = abbreviate_packument(&doc, now());

    assert_eq!(out["versions"]["1.0.0"]["dist"]["shasum"], "deadbeef");
    assert_eq!(out["versions"]["2.0.0"]["dist"]["shasum"], "cafe");
}

#[test]
fn coarsens_time_entries_by_age() {
    // `now()` is 2024-03-20T12:00Z; the horizon is one week earlier
    // (2024-03-13T12:00Z). Values are rounded *up* so the coarsened
    // timestamp never predates the real publish time.
    let doc = json!({
        "name": "foo",
        "time": {
            // Older than a week: rounded up to the next day...
            "modified": "2024-03-01T08:15:30.500Z",
            "1.0.0": "2023-12-25T23:59:59.000Z",
            // ...unless already exactly midnight, which stays put.
            "2.0.0": "2024-01-10T00:00:00.000Z",
            // Within the last week: rounded up to the next minute...
            "1.1.0": "2024-03-19T08:30:45.123Z",
            // ...unless already on a minute boundary.
            "1.2.0": "2024-03-18T06:15:00.000Z",
            // The reserved `unpublished` object passes through verbatim.
            "unpublished": { "time": "2024-03-18T00:00:00.000Z", "versions": ["0.9.0"] },
            // An unparsable value is left untouched.
            "0.0.1": "not a date"
        },
        "versions": {
            "1.0.0": { "version": "1.0.0", "dist": { "tarball": "x/foo-1.0.0.tgz" } }
        }
    });

    let out = abbreviate_packument(&doc, now());
    let time = &out["time"];

    assert_eq!(time["modified"], "2024-03-02");
    assert_eq!(time["1.0.0"], "2023-12-26");
    assert_eq!(time["2.0.0"], "2024-01-10");
    assert_eq!(time["1.1.0"], "2024-03-19T08:31Z");
    assert_eq!(time["1.2.0"], "2024-03-18T06:15Z");
    assert_eq!(time["unpublished"]["versions"][0], "0.9.0");
    assert_eq!(time["0.0.1"], "not a date");
    // Synthesized top-level `modified` mirrors the coarsened entry.
    assert_eq!(out["modified"], "2024-03-02");
}
