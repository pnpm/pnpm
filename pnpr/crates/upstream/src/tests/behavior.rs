use super::{CanonicalPackageName, json, rewrite_upstream_tarball_urls};

#[test]
fn drops_invalid_upstream_revision_history_entries() {
    let digest = "A".repeat(86);
    let mut doc = json!({
        "version": "1.0.0",
        "dist": {
            "tarball": format!("https://upstream.test/-/tarballs/sha512/{digest}"),
            "integrity": format!("sha512-{digest}=="),
            "revision": 2,
            "revisions": [{
                "revision": 1,
                "integrity": format!("sha512-{digest}=="),
                "tarball": format!("https://other.test/-/tarballs/sha512/{digest}"),
                "manifest": {},
            }],
        },
    });
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();

    rewrite_upstream_tarball_urls(&mut doc, &name, "https://upstream.test/", "http://pnpr.test/");

    assert_eq!(doc["dist"]["revisions"], json!([]));
}
