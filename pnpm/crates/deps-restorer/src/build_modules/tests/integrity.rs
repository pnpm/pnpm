use super::{super::allow_build_policy::AllowBuildPolicy, policy_from_specs};
use pretty_assertions::assert_eq;

// Policy-logic tests below drive `AllowBuildPolicy::new` directly with
// in-memory rule maps so the policy logic stays decoupled from the
// manifest reader and `Config` parsing.

#[test]
fn default_policy_denies_all() {
    let policy = AllowBuildPolicy::default();
    assert_eq!(policy.check("any-package@1.0.0"), None);
}
#[test]
fn explicit_allow_by_dep_path_allows_untrusted_package_identity() {
    let policy = policy_from_specs(
        [("foo@git+https://github.com/org/foo.git#abc123", true), ("foo", true)],
        false,
    );
    assert_eq!(
        policy.check("foo@git+https://github.com/org/foo.git#abc123(react@19.0.0)"),
        Some(true),
    );
    assert_eq!(policy.check("foo@git+https://github.com/attacker/foo.git#abc123"), None);
}
#[test]
fn explicit_allow_by_git_repo_allows_untrusted_package_identity() {
    let policy = policy_from_specs(
        [
            ("foo@git+ssh://git@example.com/org/foo.git", true),
            ("bar@git+ssh://git@example.com/org/bar.git", false),
        ],
        false,
    );

    assert_eq!(policy.check("foo@git+ssh://git@example.com/org/foo.git#abc123"), Some(true));
    assert_eq!(policy.check("foo@git+ssh://git@example.com/org/foo.git"), Some(true));
    assert_eq!(
        policy.check("foo@git+ssh://git@example.com/org/foo.git#def456(react@19.0.0)"),
        Some(true),
    );
    assert_eq!(policy.check("foo@git+ssh://git@example.com/other/foo.git#abc123"), None);
    assert_eq!(policy.check("foo@1.0.0"), None);
    assert_eq!(policy.check("bar@git+ssh://git@example.com/org/bar.git#abc123"), Some(false));
}
#[test]
fn explicit_allow_by_tarball_dep_path_allows_untrusted_package_identity() {
    let policy = policy_from_specs([("foo@https://example.com/foo.tgz", true)], false);

    assert_eq!(policy.check("foo@https://example.com/foo.tgz"), Some(true));
}
