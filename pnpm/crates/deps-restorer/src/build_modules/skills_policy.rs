//! Deciding which agent skills a dependency may link.

use super::allow_build_policy::{
    git_repo_allow_build_key_from_dep_path, is_dep_path_allow_build_key,
    is_git_repo_allow_build_key, normalize_build_dep_path,
};
use super::{
    HashMap, VersionPolicyError, expand_package_version_spec, parse_name_version_from_key,
};
use pnpm_config::SkillsPermission;

/// Skill-linking policy derived from the `skills` decisions of
/// `permissions` in `pnpm-workspace.yaml`, keyed like `allowBuilds` rules:
/// by config dep path, git repo, or expanded package spec.
///
/// The per-skill answers follow the same trust-gated ladder as
/// the `allowBuilds` policy — a `permissions.<pkg>.skills: false`
/// decision is a denial outranking any allowance, and a bare-name or
/// `name@version` decision only reaches a dep path whose version is a semver
/// range, the same trust signal `allowBuilds` requires.
#[derive(Debug, Default)]
pub struct SkillsPolicy {
    expanded: HashMap<String, SkillsPermission>,
    dep_paths: HashMap<String, SkillsPermission>,
    git_repos: HashMap<String, SkillsPermission>,
}

/// One side of `permissions`, split by the shape of its keys.
#[derive(Default)]
struct SkillsKeys {
    specs: Vec<(String, SkillsPermission)>,
    dep_paths: HashMap<String, SkillsPermission>,
    git_repos: HashMap<String, SkillsPermission>,
}

impl SkillsKeys {
    fn add(&mut self, spec: &str, decision: &SkillsPermission) {
        if is_git_repo_allow_build_key(spec) {
            self.git_repos.upsert_conservative(spec.to_owned(), decision);
        } else if is_dep_path_allow_build_key(spec) {
            self.dep_paths.upsert_conservative(normalize_build_dep_path(spec), decision);
        } else {
            self.specs.push((spec.to_owned(), decision.clone()));
        }
    }
}

/// The one decision a collapsed key records. Two `pnpm-workspace.yaml`
/// entries that normalize onto the same key keep their distinct spec keys in
/// the file; the map can hold only one, so a denial is kept over a decision
/// that links and the first-seen decision is kept over another decision that
/// links.
trait ConservativeUpsert {
    fn upsert_conservative(&mut self, key: String, decision: &SkillsPermission);
}

impl ConservativeUpsert for HashMap<String, SkillsPermission> {
    fn upsert_conservative(&mut self, key: String, decision: &SkillsPermission) {
        let entry = self.entry(key).or_insert_with(|| decision.clone());
        if matches!(decision, SkillsPermission::All(false))
            && *entry != SkillsPermission::All(false)
        {
            *entry = decision.clone();
        }
    }
}

impl SkillsPolicy {
    /// Build the policy from a map of `permissions.<pkg>.skills` keys to
    /// decisions. Reads `Config::allow_skills`.
    pub fn from_approvals(
        approvals: &HashMap<String, SkillsPermission>,
    ) -> Result<Self, VersionPolicyError> {
        let mut keys = SkillsKeys::default();
        for (spec, decision) in approvals {
            keys.add(spec, decision);
        }
        let mut expanded: HashMap<String, SkillsPermission> = HashMap::new();
        for (spec, decision) in keys.specs {
            for key in expand_package_version_spec(&spec)? {
                expanded.upsert_conservative(key, &decision);
            }
        }
        Ok(Self { expanded, dep_paths: keys.dep_paths, git_repos: keys.git_repos })
    }

    /// The skills decision for the package installed at `dep_path`, or
    /// `None` when nothing is decided and the skills await approval.
    ///
    /// A `permissions` dep-path or git-repo rule matches any dep path of the
    /// same shape. A bare-name or `name@version` rule needs a trusted
    /// package identity, whose signal is a registry-shaped dep path
    /// (`name@semver`) — see the `allowBuilds` policy. A `name@version`
    /// rule outranks a bare-name one, the more specific key wins.
    #[must_use]
    pub fn check(&self, dep_path: &str) -> Option<SkillsPermission> {
        let normalized_dep_path = normalize_build_dep_path(dep_path);
        let git_repo_key = git_repo_allow_build_key_from_dep_path(&normalized_dep_path);
        let git_repo_key = git_repo_key.as_deref();
        let (name, version) = parse_name_version_from_key(&normalized_dep_path);
        let name_at_version = format!("{name}@{version}");

        let matches = [
            self.dep_paths.get(&normalized_dep_path),
            git_repo_key.and_then(|key| self.git_repos.get(key)),
            self.expanded.get(&name),
            self.expanded.get(&name_at_version),
        ];
        if matches
            .into_iter()
            .flatten()
            .any(|decision| matches!(decision, SkillsPermission::All(false)))
        {
            return Some(SkillsPermission::All(false));
        }
        if let Some(decision) = self.dep_paths.get(&normalized_dep_path) {
            return Some(decision.clone());
        }
        if let Some(decision) = git_repo_key.and_then(|key| self.git_repos.get(key)) {
            return Some(decision.clone());
        }
        // Package-name rules require a trusted package identity, as they do
        // for `allowBuilds`: a non-registry dep path stays pending rather
        // than matching by name.
        if node_semver::Version::parse(&version).is_err() {
            return None;
        }
        if let Some(decision) = self.expanded
            .get(&name_at_version)
            .or_else(|| self.expanded.get(&name))
        {
            return Some(decision.clone());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::ConservativeUpsert;
    use super::{HashMap, SkillsPermission, SkillsPolicy};
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
        assert_eq!(
            denied.check("foo@https://example.com/foo.tgz"),
            Some(SkillsPermission::All(false)),
        );
        let denied = policy(&[
            ("foo@2.0.0", SkillsPermission::All(false)),
            ("foo", SkillsPermission::All(true)),
        ]);
        assert_eq!(denied.check("foo@2.0.0"), Some(SkillsPermission::All(false)));
    }

    #[test]
    fn version_rule_outranks_bare_name() {
        let policy =
            policy(&[("foo", chosen(&["guide"])), ("foo@2.0.0", SkillsPermission::All(true))]);
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
            Some(chosen(&["guide"]))
        );
        assert_eq!(
            policy.check("foo@git+ssh://git@example.com/org/foo.git"),
            Some(chosen(&["guide"]))
        );
        assert_eq!(policy.check("foo@git+ssh://git@example.com/other/foo.git#abc123"), None);
        assert_eq!(policy.check("foo@1.0.0"), None);
    }

    #[test]
    fn github_tarball_dep_path_matches_the_repo_rule() {
        let policy =
            policy(&[("foo@git+https://github.com/org/foo.git", SkillsPermission::All(true))]);
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
                ("foo@2.0.0", SkillsPermission::All(false))
            ])
            .check("foo@2.0.0"),
            Some(SkillsPermission::All(false)),
        );
        assert_eq!(
            policy(&[
                ("foo@2.0.0", SkillsPermission::All(false)),
                ("foo@1.0.0 || 2.0.0", SkillsPermission::All(true))
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
            "deny beats a decision that links"
        );

        let mut reversed = HashMap::new();
        reversed.upsert_conservative("foo@1.0.0".to_string(), &SkillsPermission::All(false));
        reversed.upsert_conservative("foo@1.0.0".to_string(), &chosen(&["guide"]));
        assert_eq!(reversed["foo@1.0.0"], SkillsPermission::All(false));

        let mut first = HashMap::new();
        first.upsert_conservative("foo@1.0.0".to_string(), &chosen(&["guide"]));
        first.upsert_conservative("foo@1.0.0".to_string(), &SkillsPermission::All(true));
        assert_eq!(
            first["foo@1.0.0"],
            chosen(&["guide"]),
            "the first seen decision that links wins"
        );
    }
}
