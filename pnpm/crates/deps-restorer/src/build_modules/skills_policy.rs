//! Deciding which agent skills a dependency may link.

use super::{
    HashMap, VersionPolicyError,
    allow_build_policy::{
        git_repo_allow_build_key_from_dep_path, is_dep_path_allow_build_key,
        is_git_repo_allow_build_key, normalize_build_dep_path,
    },
    expand_package_version_spec, parse_name_version_from_key,
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
        let mut ordered: Vec<(&String, &SkillsPermission)> = approvals.iter().collect();
        ordered.sort_by(|left, right| left.0.cmp(right.0));
        for (spec, decision) in ordered {
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
mod tests;
