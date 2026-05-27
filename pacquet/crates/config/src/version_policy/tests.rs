use crate::version_policy::{
    PolicyMatch, VersionPolicyError, create_package_version_policy, expand_package_version_specs,
};
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
/// [`config/version-policy/src/index.ts:73-75`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/version-policy/src/index.ts#L73-L75).
#[test]
fn wildcard_name_with_version_errors() {
    let err = expand_package_version_specs(["foo*@1.0.0"]).expect_err("must reject");
    assert!(matches!(err, VersionPolicyError::NamePatternInVersionUnion { .. }), "got: {err:?}");
}

/// Mirrors upstream
/// [`config/version-policy/src/index.ts:67-69`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/version-policy/src/index.ts#L67-L69)
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

// ─── create_package_version_policy ────────────────────────────────────
//
// Ports the upstream test cases at
// <https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/version-policy/test/index.ts#L8-L55>.

/// Exact-version rule: matching name returns the version list,
/// non-matching name returns `No`. Upstream:
/// `expect(match('axios')).toStrictEqual(['1.12.2'])`.
#[test]
fn create_policy_exact_version_match_returns_versions() {
    let policy = create_package_version_policy(["axios@1.12.2"]).unwrap();
    assert_eq!(policy.matches("axios"), PolicyMatch::ExactVersions(vec!["1.12.2".to_string()]));
    assert_eq!(policy.matches("is-odd"), PolicyMatch::No);
}

/// Wildcard name rule (no version): every matching name returns
/// `AnyVersion`; non-matching names return `No`. Upstream:
/// `expect(match('is-odd')).toBe(true)`.
#[test]
fn create_policy_wildcard_name_matches_via_matcher() {
    let policy = create_package_version_policy(["is-*"]).unwrap();
    assert_eq!(policy.matches("is-odd"), PolicyMatch::AnyVersion);
    assert_eq!(policy.matches("is-even"), PolicyMatch::AnyVersion);
    assert_eq!(policy.matches("lodash"), PolicyMatch::No);
}

/// Scoped name with exact version. Upstream:
/// `expect(match('@babel/core')).toStrictEqual(['7.20.0'])`.
#[test]
fn create_policy_scoped_name_at_exact_version() {
    let policy = create_package_version_policy(["@babel/core@7.20.0"]).unwrap();
    assert_eq!(
        policy.matches("@babel/core"),
        PolicyMatch::ExactVersions(vec!["7.20.0".to_string()]),
    );
}

/// Scoped bare name returns `AnyVersion`. Upstream:
/// `expect(match('@babel/core')).toBe(true)`.
#[test]
fn create_policy_scoped_bare_name_returns_any_version() {
    let policy = create_package_version_policy(["@babel/core"]).unwrap();
    assert_eq!(policy.matches("@babel/core"), PolicyMatch::AnyVersion);
}

/// Multiple rules: the first matching rule wins. Upstream:
/// `expect(match('axios')).toStrictEqual(['1.12.2'])` for a list
/// containing `axios@1.12.2`, `lodash@4.17.21`, `is-*`.
#[test]
fn create_policy_first_matching_rule_wins() {
    let policy = create_package_version_policy(["axios@1.12.2", "lodash@4.17.21", "is-*"]).unwrap();
    assert_eq!(policy.matches("axios"), PolicyMatch::ExactVersions(vec!["1.12.2".to_string()]));
    assert_eq!(policy.matches("lodash"), PolicyMatch::ExactVersions(vec!["4.17.21".to_string()]));
    assert_eq!(policy.matches("is-odd"), PolicyMatch::AnyVersion);
}

/// Non-exact semver in a name@version rule errors. Upstream:
/// `expect(() => createPackageVersionPolicy(['lodash@^4.17.0']))
///    .toThrow(/Invalid versions union/)`.
#[test]
fn create_policy_range_specifier_in_version_errors() {
    let err = create_package_version_policy(["lodash@^4.17.0"]).expect_err("must reject");
    assert!(matches!(err, VersionPolicyError::InvalidVersionUnion { .. }), "got: {err:?}");

    let err = create_package_version_policy(["lodash@~4.17.0"]).expect_err("must reject");
    assert!(matches!(err, VersionPolicyError::InvalidVersionUnion { .. }), "got: {err:?}");

    let err = create_package_version_policy(["react@>=18.0.0"]).expect_err("must reject");
    assert!(matches!(err, VersionPolicyError::InvalidVersionUnion { .. }), "got: {err:?}");
}

/// Wildcard + version combo errors. Upstream:
/// `expect(() => createPackageVersionPolicy(['is-*@1.0.0']))
///    .toThrow(/Name patterns are not allowed/)`.
#[test]
fn create_policy_wildcard_with_version_errors() {
    let err = create_package_version_policy(["is-*@1.0.0"]).expect_err("must reject");
    assert!(matches!(err, VersionPolicyError::NamePatternInVersionUnion { .. }), "got: {err:?}");
}

/// Version unions on the unscoped path. Upstream:
/// `expect(match('axios')).toStrictEqual(['1.12.0', '1.12.1'])` for
/// `axios@1.12.0 || 1.12.1`.
#[test]
fn create_policy_version_union_unscoped() {
    let policy = create_package_version_policy(["axios@1.12.0 || 1.12.1"]).unwrap();
    assert_eq!(
        policy.matches("axios"),
        PolicyMatch::ExactVersions(vec!["1.12.0".to_string(), "1.12.1".to_string()]),
    );
}

/// Version unions on the scoped path. Upstream:
/// `expect(match('@scope/pkg')).toStrictEqual(['1.0.0', '1.0.1'])`.
#[test]
fn create_policy_version_union_scoped() {
    let policy = create_package_version_policy(["@scope/pkg@1.0.0 || 1.0.1"]).unwrap();
    assert_eq!(
        policy.matches("@scope/pkg"),
        PolicyMatch::ExactVersions(vec!["1.0.0".to_string(), "1.0.1".to_string()]),
    );
}

/// Three-version union with non-standard whitespace. Upstream:
/// `expect(match('pkg')).toStrictEqual(['1.0.0', '1.0.1', '1.0.2'])`.
#[test]
fn create_policy_version_union_handles_whitespace() {
    let policy = create_package_version_policy(["pkg@1.0.0||1.0.1  ||  1.0.2"]).unwrap();
    assert_eq!(
        policy.matches("pkg"),
        PolicyMatch::ExactVersions(vec![
            "1.0.0".to_string(),
            "1.0.1".to_string(),
            "1.0.2".to_string(),
        ]),
    );
}
