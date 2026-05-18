use crate::group::{PatchInput, group_patched_dependencies};
use crate::types::{ExtendedPatchInfo, PatchGroup, PatchGroupRangeItem};
use pretty_assertions::assert_eq;

const ZERO_HASH: &str = "00000000000000000000000000000000";

fn input(hash: &str) -> PatchInput {
    PatchInput { hash: hash.to_string(), patch_file_path: None }
}

fn info(key: &str, hash: &str) -> ExtendedPatchInfo {
    ExtendedPatchInfo { hash: hash.to_string(), patch_file_path: None, key: key.to_string() }
}

/// Mirrors upstream's
/// [`'groups patchedDependencies according to names, match types, and versions'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/test/groupPatchedDependencies.test.ts#L17-L74).
#[test]
fn groups_by_name_match_type_and_version() {
    let entries: Vec<(String, PatchInput)> = vec![
        ("exact-version-only@0.0.0", ZERO_HASH),
        ("exact-version-only@1.2.3", ZERO_HASH),
        ("exact-version-only@2.1.0", ZERO_HASH),
        ("version-range-only@~1.2.0", ZERO_HASH),
        ("version-range-only@4", ZERO_HASH),
        ("star-version-range@*", ZERO_HASH),
        ("without-versions", ZERO_HASH),
        ("mixed-style@0.1.2", ZERO_HASH),
        ("mixed-style@1.x.x", ZERO_HASH),
        ("mixed-style", ZERO_HASH),
    ]
    .into_iter()
    .map(|(k, h)| (k.to_string(), input(h)))
    .collect();

    let result = group_patched_dependencies(entries).expect("valid input");

    let exact_version_only =
        result.get("exact-version-only").expect("exact-version-only group present");
    assert_eq!(
        exact_version_only.exact.get("0.0.0"),
        Some(&info("exact-version-only@0.0.0", ZERO_HASH)),
    );
    assert_eq!(
        exact_version_only.exact.get("1.2.3"),
        Some(&info("exact-version-only@1.2.3", ZERO_HASH)),
    );
    assert_eq!(
        exact_version_only.exact.get("2.1.0"),
        Some(&info("exact-version-only@2.1.0", ZERO_HASH)),
    );
    assert!(exact_version_only.range.is_empty());
    assert_eq!(exact_version_only.all, None);

    let version_range_only =
        result.get("version-range-only").expect("version-range-only group present");
    assert!(version_range_only.exact.is_empty());
    // Insertion order matches input order (input listed `~1.2.0` then
    // `4`). Upstream's test sorts before asserting because JS object
    // iteration order is implementation-defined; our deterministic
    // iteration order means the sort would be a no-op.
    assert_eq!(
        version_range_only.range,
        vec![
            PatchGroupRangeItem {
                version: "~1.2.0".to_string(),
                patch: info("version-range-only@~1.2.0", ZERO_HASH),
            },
            PatchGroupRangeItem {
                version: "4".to_string(),
                patch: info("version-range-only@4", ZERO_HASH),
            },
        ],
    );
    assert_eq!(version_range_only.all, None);

    let star = result.get("star-version-range").expect("star-version-range group present");
    assert!(star.exact.is_empty());
    assert!(star.range.is_empty());
    assert_eq!(star.all, Some(info("star-version-range@*", ZERO_HASH)));

    let without_versions = result.get("without-versions").expect("without-versions group present");
    assert!(without_versions.exact.is_empty());
    assert!(without_versions.range.is_empty());
    assert_eq!(without_versions.all, Some(info("without-versions", ZERO_HASH)));

    let mixed = result.get("mixed-style").expect("mixed-style group present");
    assert_eq!(mixed.exact.get("0.1.2"), Some(&info("mixed-style@0.1.2", ZERO_HASH)));
    assert_eq!(
        mixed.range,
        vec![PatchGroupRangeItem {
            version: "1.x.x".to_string(),
            patch: info("mixed-style@1.x.x", ZERO_HASH),
        }],
    );
    assert_eq!(mixed.all, Some(info("mixed-style", ZERO_HASH)));
}

/// Mirrors upstream's
/// [`'errors on invalid version range'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/test/groupPatchedDependencies.test.ts#L76-L81).
#[test]
fn errors_on_invalid_version_range() {
    let entries = vec![("foo@link:packages/foo".to_string(), input(ZERO_HASH))];
    let err = group_patched_dependencies(entries).expect_err("non-semver range must error");
    assert_eq!(err.non_semver_version, "link:packages/foo");
}

#[test]
fn star_wildcard_lands_in_all() {
    let entries = vec![("foo@*".to_string(), input(ZERO_HASH))];
    let result = group_patched_dependencies(entries).expect("valid");
    let group = result.get("foo").expect("foo group");
    assert_eq!(group.all, Some(info("foo@*", ZERO_HASH)));
    assert!(group.range.is_empty());
}

/// Empty `PatchGroup` value type round-trips through default. Guard
/// against accidentally requiring fields at construction.
#[test]
fn patch_group_default_is_empty() {
    let g = PatchGroup::default();
    assert!(g.exact.is_empty());
    assert!(g.range.is_empty());
    assert_eq!(g.all, None);
}
