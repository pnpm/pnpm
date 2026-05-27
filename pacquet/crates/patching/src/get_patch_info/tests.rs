use crate::get_patch_info::{PatchKeyConflictError, get_patch_info};
use crate::types::{ExtendedPatchInfo, PatchGroup, PatchGroupRangeItem, PatchGroupRecord};
use pretty_assertions::assert_eq;

const ZERO_HASH: &str = "00000000000000000000000000000000";

fn info(key: &str) -> ExtendedPatchInfo {
    ExtendedPatchInfo { hash: ZERO_HASH.to_string(), patch_file_path: None, key: key.to_string() }
}

fn record_with_foo(group: PatchGroup) -> PatchGroupRecord {
    let mut record = PatchGroupRecord::new();
    record.insert("foo".to_string(), group);
    record
}

/// Mirrors upstream's
/// [`'getPatchInfo(undefined, ...) returns undefined'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/test/getPatchInfo.test.ts#L6-L8).
#[test]
fn missing_record_returns_none() {
    assert_eq!(get_patch_info(None, "foo", "1.0.0").unwrap(), None);
}

/// Mirrors upstream's
/// [`'getPatchInfo() returns an exact version patch if the name and version match'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/test/getPatchInfo.test.ts#L10-L24).
#[test]
fn exact_version_match() {
    let mut exact = std::collections::BTreeMap::new();
    let patch = info("foo@1.0.0");
    exact.insert("1.0.0".to_string(), patch.clone());
    let record = record_with_foo(PatchGroup { exact, ..PatchGroup::default() });

    assert_eq!(get_patch_info(Some(&record), "foo", "1.0.0").unwrap(), Some(&patch));
    assert_eq!(get_patch_info(Some(&record), "foo", "1.1.0").unwrap(), None);
    assert_eq!(get_patch_info(Some(&record), "foo", "2.0.0").unwrap(), None);
    assert_eq!(get_patch_info(Some(&record), "bar", "1.0.0").unwrap(), None);
}

/// Mirrors upstream's
/// [`'getPatchInfo() returns a range version patch if the name matches and the version satisfied'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/test/getPatchInfo.test.ts#L26-L43).
#[test]
fn range_version_match() {
    let patch = info("foo@1");
    let record = record_with_foo(PatchGroup {
        range: vec![PatchGroupRangeItem { version: "1".to_string(), patch: patch.clone() }],
        ..PatchGroup::default()
    });

    assert_eq!(get_patch_info(Some(&record), "foo", "1.0.0").unwrap(), Some(&patch));
    assert_eq!(get_patch_info(Some(&record), "foo", "1.1.0").unwrap(), Some(&patch));
    assert_eq!(get_patch_info(Some(&record), "foo", "2.0.0").unwrap(), None);
    assert_eq!(get_patch_info(Some(&record), "bar", "1.0.0").unwrap(), None);
}

/// Mirrors upstream's
/// [`'getPatchInfo() returns name-only patch if the name matches'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/test/getPatchInfo.test.ts#L45-L58).
#[test]
fn name_only_match() {
    let patch = info("foo");
    let record = record_with_foo(PatchGroup { all: Some(patch.clone()), ..PatchGroup::default() });

    assert_eq!(get_patch_info(Some(&record), "foo", "1.0.0").unwrap(), Some(&patch));
    assert_eq!(get_patch_info(Some(&record), "foo", "1.1.0").unwrap(), Some(&patch));
    assert_eq!(get_patch_info(Some(&record), "foo", "2.0.0").unwrap(), Some(&patch));
    assert_eq!(get_patch_info(Some(&record), "bar", "1.0.0").unwrap(), None);
}

/// Mirrors upstream's
/// [`'exact version patches override version range patches, version range patches override name-only patches'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/test/getPatchInfo.test.ts#L60-L106).
#[test]
fn precedence_exact_over_range_over_all() {
    let mut exact = std::collections::BTreeMap::new();
    let p100 = info("foo@1.0.0");
    let p110 = info("foo@1.1.0");
    exact.insert("1.0.0".to_string(), p100.clone());
    exact.insert("1.1.0".to_string(), p110.clone());

    let p_range_1 = info("foo@1");
    let p_range_2 = info("foo@2");
    let p_all = info("foo");

    let record = record_with_foo(PatchGroup {
        exact,
        range: vec![
            PatchGroupRangeItem { version: "1".to_string(), patch: p_range_1.clone() },
            PatchGroupRangeItem { version: "2".to_string(), patch: p_range_2.clone() },
        ],
        all: Some(p_all.clone()),
    });

    assert_eq!(get_patch_info(Some(&record), "foo", "1.0.0").unwrap(), Some(&p100));
    assert_eq!(get_patch_info(Some(&record), "foo", "1.1.0").unwrap(), Some(&p110));
    assert_eq!(get_patch_info(Some(&record), "foo", "1.1.1").unwrap(), Some(&p_range_1));
    assert_eq!(get_patch_info(Some(&record), "foo", "2.0.0").unwrap(), Some(&p_range_2));
    assert_eq!(get_patch_info(Some(&record), "foo", "2.1.0").unwrap(), Some(&p_range_2));
    assert_eq!(get_patch_info(Some(&record), "foo", "3.0.0").unwrap(), Some(&p_all));
    assert_eq!(get_patch_info(Some(&record), "bar", "1.0.0").unwrap(), None);
}

/// Mirrors upstream's
/// [`'getPatchInfo(_, name, version) throws an error when name@version matches more than one version range patches'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/test/getPatchInfo.test.ts#L108-L136).
#[test]
fn ambiguous_ranges_error() {
    let record = record_with_foo(PatchGroup {
        range: vec![
            PatchGroupRangeItem {
                version: ">=1.0.0 <3.0.0".to_string(),
                patch: info("foo@>=1.0.0 <3.0.0"),
            },
            PatchGroupRangeItem { version: ">=2.0.0".to_string(), patch: info("foo@>=2.0.0") },
        ],
        ..PatchGroup::default()
    });

    let err = get_patch_info(Some(&record), "foo", "2.1.0").expect_err("must conflict");
    let PatchKeyConflictError { pkg_name, pkg_version, satisfied_versions } = err;
    assert_eq!(pkg_name, "foo");
    assert_eq!(pkg_version, "2.1.0");
    assert_eq!(satisfied_versions, vec![">=1.0.0 <3.0.0".to_string(), ">=2.0.0".to_string()]);
}

/// Mirrors upstream's
/// [`'getPatchInfo(_, name, version) does not throw an error when name@version matches an exact version patch and more than one version range patches'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/test/getPatchInfo.test.ts#L138-L167).
#[test]
fn exact_match_short_circuits_ambiguity() {
    let mut exact = std::collections::BTreeMap::new();
    let p210 = info("foo@>=1.0.0 <3.0.0");
    exact.insert("2.1.0".to_string(), p210.clone());

    let record = record_with_foo(PatchGroup {
        exact,
        range: vec![
            PatchGroupRangeItem {
                version: ">=1.0.0 <3.0.0".to_string(),
                patch: info("foo@>=1.0.0 <3.0.0"),
            },
            PatchGroupRangeItem { version: ">=2.0.0".to_string(), patch: info("foo@>=2.0.0") },
        ],
        ..PatchGroup::default()
    });

    assert_eq!(get_patch_info(Some(&record), "foo", "2.1.0").unwrap(), Some(&p210));
}
