use crate::{
    group::{PatchInput, group_patched_dependencies},
    verify::{UnusedPatchError, UnusedPatches, all_patch_keys, verify_patches},
};
use pretty_assertions::assert_eq;
use std::collections::HashSet;

const ZERO_HASH: &str = "00000000000000000000000000000000";

fn input(hash: &str) -> PatchInput {
    PatchInput { hash: hash.to_string(), patch_file_path: None }
}

fn entries(keys: &[&str]) -> Vec<(String, PatchInput)> {
    keys.iter().map(|key| (key.to_string(), input(ZERO_HASH))).collect()
}

/// `all_patch_keys` iteration order is part of the contract —
/// `verify_patches` uses it to build the unused-patch list that
/// surfaces in `ERR_PNPM_UNUSED_PATCH`. The order is:
///
/// 1. Outer: alphabetical by package name (`PatchGroupRecord` is a
///    `BTreeMap`).
/// 2. Within a group: exact versions (alphabetical by version
///    string, also `BTreeMap`), then ranges (insertion order, `Vec`),
///    then the wildcard.
///
/// Assert the order directly rather than sorting — sorting would
/// hide regressions in [`all_patch_keys`].
#[test]
fn all_keys_yields_every_configured_key() {
    let groups = group_patched_dependencies(entries(&[
        "foo@1.0.0",
        "foo@^2.0.0",
        "foo",
        "bar@3.0.0",
        "baz",
    ]))
    .unwrap();

    let keys: Vec<&str> = all_patch_keys(&groups).collect();
    assert_eq!(keys, vec!["bar@3.0.0", "baz", "foo@1.0.0", "foo@^2.0.0", "foo"]);
}

#[test]
fn no_unused_patches_returns_ok_none() {
    let groups = group_patched_dependencies(entries(&["foo@1.0.0", "bar"])).unwrap();
    let applied: HashSet<String> =
        ["foo@1.0.0", "bar"].iter().map(std::string::ToString::to_string).collect();
    assert_eq!(verify_patches(&groups, &applied, false).unwrap(), None);
}

/// `allow_unused_patches: true` surfaces the list to the caller so
/// the caller can warn via `pacquet-diagnostics`. Mirrors upstream's
/// [`globalWarn` branch](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/src/verifyPatches.ts#L26-L28).
#[test]
fn unused_patches_with_allow_returns_warning_payload() {
    let groups = group_patched_dependencies(entries(&["foo@1.0.0", "bar"])).unwrap();
    let applied: HashSet<String> =
        std::iter::once(&"foo@1.0.0").map(std::string::ToString::to_string).collect();
    let result = verify_patches(&groups, &applied, true).unwrap();
    assert_eq!(result, Some(UnusedPatches { unused_patches: vec!["bar".to_string()] }));
}

#[test]
fn unused_patches_without_allow_returns_err() {
    let groups = group_patched_dependencies(entries(&["foo@1.0.0", "bar"])).unwrap();
    let applied: HashSet<String> =
        std::iter::once(&"foo@1.0.0").map(std::string::ToString::to_string).collect();
    let err: UnusedPatchError = verify_patches(&groups, &applied, false).unwrap_err();
    assert_eq!(err.unused_patches, vec!["bar".to_string()]);
}
