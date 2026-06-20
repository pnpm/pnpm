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

#[test]
fn name_with_wildcard_alone_is_kept_verbatim() {
    assert_eq!(expand(&["is-*"]), vec!["is-*".to_string()]);
}

#[test]
fn wildcard_name_with_version_errors() {
    let err = expand_package_version_specs(["foo*@1.0.0"]).expect_err("must reject");
    assert!(matches!(err, VersionPolicyError::NamePatternInVersionUnion { .. }), "got: {err:?}");
}

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

#[test]
fn create_policy_exact_version_match_returns_versions() {
    let policy = create_package_version_policy(["axios@1.12.2"]).unwrap();
    assert_eq!(policy.matches("axios"), PolicyMatch::ExactVersions(vec!["1.12.2".to_string()]));
    assert_eq!(policy.matches("is-odd"), PolicyMatch::No);
}

#[test]
fn create_policy_wildcard_name_matches_via_matcher() {
    let policy = create_package_version_policy(["is-*"]).unwrap();
    assert_eq!(policy.matches("is-odd"), PolicyMatch::AnyVersion);
    assert_eq!(policy.matches("is-even"), PolicyMatch::AnyVersion);
    assert_eq!(policy.matches("lodash"), PolicyMatch::No);
}

#[test]
fn create_policy_scoped_name_at_exact_version() {
    let policy = create_package_version_policy(["@babel/core@7.20.0"]).unwrap();
    assert_eq!(
        policy.matches("@babel/core"),
        PolicyMatch::ExactVersions(vec!["7.20.0".to_string()]),
    );
}

#[test]
fn create_policy_scoped_bare_name_returns_any_version() {
    let policy = create_package_version_policy(["@babel/core"]).unwrap();
    assert_eq!(policy.matches("@babel/core"), PolicyMatch::AnyVersion);
}

#[test]
fn create_policy_distinct_name_rules() {
    let policy = create_package_version_policy(["axios@1.12.2", "lodash@4.17.21", "is-*"]).unwrap();
    assert_eq!(policy.matches("axios"), PolicyMatch::ExactVersions(vec!["1.12.2".to_string()]));
    assert_eq!(policy.matches("lodash"), PolicyMatch::ExactVersions(vec!["4.17.21".to_string()]));
    assert_eq!(policy.matches("is-odd"), PolicyMatch::AnyVersion);
}

#[test]
fn create_policy_multiple_exact_version_rules_for_same_name_merge() {
    let policy = create_package_version_policy(["form-data@4.0.6", "form-data@2.5.6"]).unwrap();
    assert_eq!(
        policy.matches("form-data"),
        PolicyMatch::ExactVersions(vec!["4.0.6".to_string(), "2.5.6".to_string()]),
    );
}

#[test]
fn create_policy_merges_exact_versions_and_unions_for_same_name() {
    let policy =
        create_package_version_policy(["form-data@4.0.6", "form-data@2.5.6 || 2.5.7"]).unwrap();
    assert_eq!(
        policy.matches("form-data"),
        PolicyMatch::ExactVersions(vec![
            "4.0.6".to_string(),
            "2.5.6".to_string(),
            "2.5.7".to_string(),
        ]),
    );
}

#[test]
fn create_policy_deduplicates_repeated_versions_across_rules() {
    let policy = create_package_version_policy(["form-data@4.0.6", "form-data@4.0.6"]).unwrap();
    assert_eq!(policy.matches("form-data"), PolicyMatch::ExactVersions(vec!["4.0.6".to_string()]));
}

#[test]
fn create_policy_bare_rule_after_exact_keeps_exact_versions() {
    let policy = create_package_version_policy(["axios@1.12.2", "axios"]).unwrap();
    assert_eq!(policy.matches("axios"), PolicyMatch::ExactVersions(vec!["1.12.2".to_string()]));
}

#[test]
fn create_policy_bare_rule_listed_first_wins_over_later_exact() {
    let policy = create_package_version_policy(["axios", "axios@1.12.2"]).unwrap();
    assert_eq!(policy.matches("axios"), PolicyMatch::AnyVersion);
}

#[test]
fn create_policy_wildcard_after_exact_keeps_exact_versions() {
    let policy = create_package_version_policy(["axios@1.12.2", "ax*"]).unwrap();
    assert_eq!(policy.matches("axios"), PolicyMatch::ExactVersions(vec!["1.12.2".to_string()]));
}

#[test]
fn create_policy_wildcard_listed_first_wins_over_later_exact() {
    let policy = create_package_version_policy(["ax*", "axios@1.12.2"]).unwrap();
    assert_eq!(policy.matches("axios"), PolicyMatch::AnyVersion);
}

#[test]
fn create_policy_range_specifier_in_version_errors() {
    let err = create_package_version_policy(["lodash@^4.17.0"]).expect_err("must reject");
    assert!(matches!(err, VersionPolicyError::InvalidVersionUnion { .. }), "got: {err:?}");

    let err = create_package_version_policy(["lodash@~4.17.0"]).expect_err("must reject");
    assert!(matches!(err, VersionPolicyError::InvalidVersionUnion { .. }), "got: {err:?}");

    let err = create_package_version_policy(["react@>=18.0.0"]).expect_err("must reject");
    assert!(matches!(err, VersionPolicyError::InvalidVersionUnion { .. }), "got: {err:?}");
}

#[test]
fn create_policy_wildcard_with_version_errors() {
    let err = create_package_version_policy(["is-*@1.0.0"]).expect_err("must reject");
    assert!(matches!(err, VersionPolicyError::NamePatternInVersionUnion { .. }), "got: {err:?}");
}

#[test]
fn create_policy_version_union_unscoped() {
    let policy = create_package_version_policy(["axios@1.12.0 || 1.12.1"]).unwrap();
    assert_eq!(
        policy.matches("axios"),
        PolicyMatch::ExactVersions(vec!["1.12.0".to_string(), "1.12.1".to_string()]),
    );
}

#[test]
fn create_policy_version_union_scoped() {
    let policy = create_package_version_policy(["@scope/pkg@1.0.0 || 1.0.1"]).unwrap();
    assert_eq!(
        policy.matches("@scope/pkg"),
        PolicyMatch::ExactVersions(vec!["1.0.0".to_string(), "1.0.1".to_string()]),
    );
}

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
