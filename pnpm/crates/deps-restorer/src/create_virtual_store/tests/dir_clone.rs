use super::super::dir_clone_cacheable;
use pnpm_lockfile::{PackageKey, PackageMetadata};
use std::collections::HashMap;

const INTEGRITY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

#[test]
fn only_immutable_integrity_addressed_resolutions_qualify() {
    for (label, resolution, cacheable) in [
        ("registry", serde_json::json!({ "integrity": INTEGRITY }), true),
        (
            "remote tarball",
            serde_json::json!({ "tarball": "https://example.com/foo.tgz", "integrity": INTEGRITY }),
            true,
        ),
        (
            "local tarball",
            serde_json::json!({ "tarball": "file:../foo.tgz", "integrity": INTEGRITY }),
            false,
        ),
        (
            "flagged git-hosted tarball",
            serde_json::json!({
                "tarball": "https://example.com/foo.tgz",
                "gitHosted": true,
                "integrity": INTEGRITY,
            }),
            false,
        ),
        (
            "git-host URL contradicting the flag",
            serde_json::json!({
                "tarball": "https://codeload.github.com/foo/bar/tar.gz/0123456789abcdef0123456789abcdef01234567",
                "gitHosted": false,
                "integrity": INTEGRITY,
            }),
            false,
        ),
        ("directory", serde_json::json!({ "type": "directory", "directory": "../foo" }), false),
        ("git", serde_json::json!({ "type": "git", "repo": "x", "commit": "abc" }), false),
    ] {
        let (key, packages) = packages_with(&resolution);

        let qualified = dir_clone_cacheable(&packages, &key, false, false, false);

        assert_eq!(qualified, cacheable, "{label}");
    }
}

#[test]
fn build_mutability_and_force_each_disqualify_a_registry_slot() {
    let (key, packages) = packages_with(&serde_json::json!({ "integrity": INTEGRITY }));

    assert!(!dir_clone_cacheable(&packages, &key, true, false, false), "needs a build marker");
    assert!(!dir_clone_cacheable(&packages, &key, false, true, false), "mutable source");
    assert!(!dir_clone_cacheable(&packages, &key, false, false, true), "forced re-import");
}

#[test]
fn a_snapshot_without_a_packages_entry_does_not_qualify() {
    let key: PackageKey = "foo@1.0.0".parse().expect("parse package key");

    assert!(!dir_clone_cacheable(&HashMap::new(), &key, false, false, false));
}

fn packages_with(
    resolution: &serde_json::Value,
) -> (PackageKey, HashMap<PackageKey, PackageMetadata>) {
    let key: PackageKey = "foo@1.0.0".parse().expect("parse package key");
    let metadata = serde_json::from_value(serde_json::json!({ "resolution": resolution }))
        .expect("parse package metadata");
    (key.clone(), HashMap::from([(key, metadata)]))
}
