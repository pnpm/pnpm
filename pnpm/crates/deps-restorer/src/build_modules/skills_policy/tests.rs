use super::{ConservativeUpsert, HashMap, SkillsPermission, SkillsPolicy};
use pretty_assertions::assert_eq;

fn chosen(skills: &[&str]) -> SkillsPermission {
    SkillsPermission::Chosen(
        skills
            .iter()
            .map(ToString::to_string)
            .collect(),
    )
}

fn policy(approvals: &[(&str, SkillsPermission)]) -> SkillsPolicy {
    SkillsPolicy::from_approvals(
        &approvals
            .iter()
            .map(|(key, decision)| ((*key).to_string(), decision.clone()))
            .collect::<HashMap<_, _>>(),
    )
    .expect("expand the approval keys")
}

#[test]
fn default_policy_decides_nothing() {
    let policy = SkillsPolicy::default();
    assert_eq!(policy.check("foo@1.0.0"), None);
}

#[test]
fn deny_by_name_outranks_every_allowance() {
    let denied = policy(&[
        ("foo@https://example.com/foo.tgz", chosen(&["guide"])),
        ("foo", SkillsPermission::All(false)),
    ]);
    assert_eq!(denied.check("foo@https://example.com/foo.tgz"), Some(SkillsPermission::All(false)));
    let denied = policy(&[
        ("foo@2.0.0", SkillsPermission::All(false)),
        ("foo", SkillsPermission::All(true)),
    ]);
    assert_eq!(denied.check("foo@2.0.0"), Some(SkillsPermission::All(false)));
}

#[test]
fn version_rule_outranks_bare_name() {
    let policy = policy(&[("foo", chosen(&["guide"])), ("foo@2.0.0", SkillsPermission::All(true))]);
    assert_eq!(
        policy.check("foo@2.0.0"),
        Some(SkillsPermission::All(true)),
        "the more specific name@version key decides",
    );
    assert_eq!(
        policy.check("foo@1.0.0"),
        Some(chosen(&["guide"])),
        "the bare-name rule still covers the other versions",
    );
}

#[test]
fn bare_name_rule_matches_registry_and_local_directories_only() {
    let policy = policy(&[("@example/my-skills", SkillsPermission::All(true))]);
    assert_eq!(policy.check("@example/my-skills@2.0.0"), Some(SkillsPermission::All(true)));
    assert_eq!(
        policy.check("@example/my-skills@link:packages/my-skills"),
        Some(SkillsPermission::All(true)),
    );
    assert_eq!(
        policy.check("@example/my-skills@file:../my-skills"),
        Some(SkillsPermission::All(true)),
    );
    assert_eq!(
        policy.check("@example/my-skills@file:packages/my-skills(peer@1.0.0)"),
        Some(SkillsPermission::All(true)),
    );
    assert_eq!(policy.check("@example/my-skills@https://example.com/foo.tgz"), None);
    assert_eq!(policy.check("@example/my-skills@file:./foo.tgz"), None);
    assert_eq!(policy.check("@example/my-skills@file:./foo.tar.gz"), None);
    assert_eq!(policy.check("@example/my-skills@file:./foo.tar"), None);
    assert_eq!(policy.check("@example/my-skills@file:./foo.TGZ"), None);
    assert_eq!(policy.check("@example/my-skills@git+https://github.com/org/foo.git#abc"), None);
}

#[test]
fn skill_approval_key_uses_bare_name_for_local_directories_and_dep_path_for_tarballs() {
    use super::skill_approval_key;

    assert_eq!(skill_approval_key("@example/my-skills@1.2.3"), "@example/my-skills");
    assert_eq!(
        skill_approval_key("@example/my-skills@link:packages/my-skills"),
        "@example/my-skills",
    );
    assert_eq!(
        skill_approval_key("@example/my-skills@file:../my-skills(peer@1.0.0)"),
        "@example/my-skills",
    );
    assert_eq!(
        skill_approval_key("@example/my-skills@file:./my-skills.tgz"),
        "@example/my-skills@file:./my-skills.tgz",
    );
    assert_eq!(
        skill_approval_key("@example/my-skills@file:./my-skills.tar.gz"),
        "@example/my-skills@file:./my-skills.tar.gz",
    );
    assert_eq!(
        skill_approval_key("@example/my-skills@file:./my-skills.tar"),
        "@example/my-skills@file:./my-skills.tar",
    );
    assert_eq!(
        skill_approval_key("foo@git+https://github.com/org/foo.git#abc123"),
        "foo@git+https://github.com/org/foo.git#abc123",
    );
}

#[test]
fn explicit_local_dep_path_rule_outranks_bare_name_rule() {
    let policy = policy(&[
        ("foo", SkillsPermission::All(true)),
        ("foo@link:packages/foo", chosen(&["guide"])),
        ("foo@file:../vendor/foo@1.0.0", chosen(&["migrate"])),
    ]);
    assert_eq!(policy.check("foo@link:packages/foo"), Some(chosen(&["guide"])));
    assert_eq!(policy.check("foo@file:../vendor/foo@1.0.0"), Some(chosen(&["migrate"])));
    assert_eq!(policy.check("foo@link:packages/other"), Some(SkillsPermission::All(true)));
}

#[test]
fn local_directory_paths_containing_at_match_bare_name_and_key_helpers() {
    use super::skill_approval_key;

    let policy = policy(&[
        ("foo", SkillsPermission::All(true)),
        ("@scope/foo", SkillsPermission::All(true)),
    ]);
    for dep_path in [
        "foo@link:packages/@scope/foo",
        "foo@file:../vendor/foo@1.0.0",
        "@scope/foo@link:packages/@scope/foo",
        "@scope/foo@file:packages/@scope/foo@2.0.0(peer@1.0.0)",
    ] {
        assert_eq!(policy.check(dep_path), Some(SkillsPermission::All(true)), "{dep_path}");
    }
    assert_eq!(skill_approval_key("foo@link:packages/@scope/foo"), "foo");
    assert_eq!(skill_approval_key("foo@file:../vendor/foo@1.0.0"), "foo");
    assert_eq!(skill_approval_key("@scope/foo@link:packages/@scope/foo"), "@scope/foo");
    assert_eq!(policy.check("foo@https://example.com/foo@1.0.0"), None);
    assert_eq!(policy.check("foo@file:../vendor/foo@1.0.0/pkg.tgz"), None);
}

#[test]
fn link_directory_with_trailing_parentheses_does_not_approve_other_directory() {
    use super::skill_approval_key;

    let policy = policy(&[
        ("foo@link:packages/foo(alt)", SkillsPermission::All(true)),
        ("bar@link:(group)", SkillsPermission::All(true)),
    ]);
    assert_eq!(policy.check("foo@link:packages/foo(alt)"), Some(SkillsPermission::All(true)));
    assert_eq!(policy.check("foo@link:packages/foo"), None);
    assert_eq!(policy.check("bar@link:(group)"), Some(SkillsPermission::All(true)));
    assert_eq!(skill_approval_key("foo@link:packages/foo(alt)"), "foo");
    assert_eq!(skill_approval_key("bar@link:(group)"), "bar");
}

#[test]
fn dep_path_rule_matches_directly() {
    let policy = policy(&[("foo@https://example.com/foo.tgz", chosen(&["guide"]))]);
    assert_eq!(policy.check("foo@https://example.com/foo.tgz"), Some(chosen(&["guide"])));
    assert_eq!(policy.check("bar@https://example.com/foo.tgz"), None);
}

#[test]
fn git_repo_rule_matches_by_repository() {
    let policy = policy(&[("foo@git+ssh://git@example.com/org/foo.git", chosen(&["guide"]))]);
    assert_eq!(
        policy.check("foo@git+ssh://git@example.com/org/foo.git#abc123"),
        Some(chosen(&["guide"])),
    );
    assert_eq!(policy.check("foo@git+ssh://git@example.com/org/foo.git"), Some(chosen(&["guide"])));
    assert_eq!(policy.check("foo@git+ssh://git@example.com/other/foo.git#abc123"), None);
    assert_eq!(policy.check("foo@1.0.0"), None);
}

#[test]
fn github_tarball_dep_path_matches_the_repo_rule() {
    let policy = policy(&[("foo@git+https://github.com/org/foo.git", SkillsPermission::All(true))]);
    assert_eq!(
        policy.check("foo@https://codeload.github.com/org/foo/tar.gz/abc123"),
        Some(SkillsPermission::All(true)),
    );
}

#[test]
fn chosen_skills_pass_through_check() {
    let policy = policy(&[("drizzle-kit", chosen(&["guide", "migrate"]))]);
    assert_eq!(policy.check("drizzle-kit@7.0.0"), Some(chosen(&["guide", "migrate"])));
}

#[test]
fn a_denial_wins_a_collapsed_key() {
    assert_eq!(
        policy(&[
            ("foo@1.0.0 || 2.0.0", SkillsPermission::All(true)),
            ("foo@2.0.0", SkillsPermission::All(false)),
        ])
        .check("foo@2.0.0"),
        Some(SkillsPermission::All(false)),
    );
    assert_eq!(
        policy(&[
            ("foo@2.0.0", SkillsPermission::All(false)),
            ("foo@1.0.0 || 2.0.0", SkillsPermission::All(true)),
        ])
        .check("foo@1.0.0"),
        Some(SkillsPermission::All(true)),
    );
}

#[test]
fn conservative_merge_keeps_the_deny_then_the_first_decision() {
    let mut collapsed = HashMap::new();
    collapsed.upsert_conservative("foo@1.0.0".to_string(), &chosen(&["guide"]));
    collapsed.upsert_conservative("foo@1.0.0".to_string(), &SkillsPermission::All(false));
    assert_eq!(
        collapsed["foo@1.0.0"],
        SkillsPermission::All(false),
        "deny beats a decision that links",
    );

    let mut reversed = HashMap::new();
    reversed.upsert_conservative("foo@1.0.0".to_string(), &SkillsPermission::All(false));
    reversed.upsert_conservative("foo@1.0.0".to_string(), &chosen(&["guide"]));
    assert_eq!(reversed["foo@1.0.0"], SkillsPermission::All(false));

    let mut first = HashMap::new();
    first.upsert_conservative("foo@1.0.0".to_string(), &chosen(&["guide"]));
    first.upsert_conservative("foo@1.0.0".to_string(), &SkillsPermission::All(true));
    assert_eq!(first["foo@1.0.0"], chosen(&["guide"]), "the first seen decision that links wins");
}

#[test]
fn deterministic_collapsed_key_merge_regardless_of_input_order() {
    let policy_a = policy(&[
        ("foo@1.0.0 || 2.0.0", chosen(&["guide"])),
        ("foo@2.0.0", SkillsPermission::All(true)),
    ]);
    let policy_b = policy(&[
        ("foo@2.0.0", SkillsPermission::All(true)),
        ("foo@1.0.0 || 2.0.0", chosen(&["guide"])),
    ]);
    assert_eq!(policy_a.check("foo@2.0.0"), policy_b.check("foo@2.0.0"));
}
