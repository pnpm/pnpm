use crate::version_policy::{VersionPolicyError, expand_package_version_specs};
use pretty_assertions::assert_eq;

fn expand(specs: &[&str]) -> Vec<String> {
    let mut out: Vec<String> =
        expand_package_version_specs(specs.iter().copied()).unwrap().into_iter().collect();
    out.sort();
    out
}

#[test]
fn bare_name_expands_verbatim() {
    assert_eq!(expand(&["foo"]), vec!["foo".to_string()]);
}

#[test]
fn scoped_bare_name_expands_verbatim() {
    assert_eq!(expand(&["@scope/foo"]), vec!["@scope/foo".to_string()]);
}

#[test]
fn name_at_exact_version_expands_to_one_literal() {
    assert_eq!(expand(&["foo@1.0.0"]), vec!["foo@1.0.0".to_string()]);
}

#[test]
fn scoped_name_at_exact_version_expands_to_one_literal() {
    assert_eq!(expand(&["@scope/foo@1.2.3"]), vec!["@scope/foo@1.2.3".to_string()]);
}

/// Mirrors the upstream test case
/// [`'should allowBuilds with true value'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/policy/test/index.ts#L5-L15)
/// — the spec `qar@1.0.0 || 2.0.0` expands into two literal
/// `qar@1.0.0` and `qar@2.0.0` entries.
#[test]
fn version_union_expands_into_separate_literals() {
    let result = expand(&["qar@1.0.0 || 2.0.0"]);
    assert_eq!(result, vec!["qar@1.0.0".to_string(), "qar@2.0.0".to_string()]);
}

#[test]
fn version_union_trims_whitespace_around_each_version() {
    // Extra whitespace around `||` and around each version. Mirrors
    // semver-js's `valid()` which trims internally before parsing.
    let result = expand(&["foo@  1.0.0   ||  2.0.0  "]);
    assert_eq!(result, vec!["foo@1.0.0".to_string(), "foo@2.0.0".to_string()]);
}

/// Mirrors upstream's
/// [`'should not allow patterns in allowBuilds'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/policy/test/index.ts#L28-L34)
/// — a pattern with `*` in the name lands in the expanded set as
/// the literal string `is-*`. Downstream callers use
/// `HashSet::contains` (mirroring upstream's `.has()`), so a real
/// package name like `is-odd` does NOT match `is-*`. The expansion
/// itself doesn't error.
#[test]
fn name_with_wildcard_alone_is_kept_verbatim() {
    assert_eq!(expand(&["is-*"]), vec!["is-*".to_string()]);
}

/// Combining a wildcard in the name with a version part is
/// explicitly an error (`ERR_PNPM_NAME_PATTERN_IN_VERSION_UNION`).
/// Mirrors upstream
/// [`config/version-policy/src/index.ts:73-75`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/version-policy/src/index.ts#L73-L75).
#[test]
fn wildcard_name_with_version_errors() {
    let err = expand_package_version_specs(["foo*@1.0.0"]).expect_err("must reject");
    assert!(matches!(err, VersionPolicyError::NamePatternInVersionUnion { .. }), "got: {err:?}");
}

/// Mirrors upstream
/// [`config/version-policy/src/index.ts:67-69`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/version-policy/src/index.ts#L67-L69)
/// — a `||` member that isn't valid semver triggers
/// `ERR_PNPM_INVALID_VERSION_UNION`.
#[test]
fn non_semver_version_in_union_errors() {
    let err = expand_package_version_specs(["foo@not-a-version"]).expect_err("must reject");
    assert!(matches!(err, VersionPolicyError::InvalidVersionUnion { .. }), "got: {err:?}");
}

#[test]
fn mixed_valid_invalid_union_errors() {
    let err =
        expand_package_version_specs(["foo@1.0.0 || not-a-version"]).expect_err("must reject");
    assert!(matches!(err, VersionPolicyError::InvalidVersionUnion { .. }), "got: {err:?}");
}

#[test]
fn empty_input_yields_empty_set() {
    let result = expand_package_version_specs::<_, &str>([]).unwrap();
    assert!(result.is_empty());
}

#[test]
fn duplicate_specs_collapse_in_set() {
    let result = expand(&["foo", "foo", "foo@1.0.0 || 1.0.0"]);
    assert_eq!(result, vec!["foo".to_string(), "foo@1.0.0".to_string()]);
}
