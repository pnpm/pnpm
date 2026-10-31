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
fn bare_name_rule_needs_a_registry_dep_path() {
    let policy = policy(&[("foo", SkillsPermission::All(true))]);
    assert_eq!(policy.check("foo@https://example.com/foo.tgz"), None);
    assert_eq!(policy.check("foo@2.0.0"), Some(SkillsPermission::All(true)));
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
