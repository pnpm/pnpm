use super::{Deserialize, PackageRules, TeamDirectory};

/// A resolved hosted registry: the `org` namespace it serves and its
/// `packages:` rules — the namespace it claims plus the per-package
/// `access` / `publish` / `unpublish` policies, with the registry-level
/// `access:` as the default an entry's omitted fields fall back to.
#[derive(Debug, Clone)]
pub struct HostedConfig {
    /// The storage/serving namespace, so two hosted registries holding the same
    /// `name@version` never collide. Empty (`""`) ⇒ the flat `storage` root.
    pub org: String,
    /// The registry's `packages:` map: namespace and per-package rules in one
    /// declaration, selected by specificity. The effective `access` gates
    /// reads *and* the write routing (publish, dist-tag, unpublish), with a
    /// denied caller masked as not-found either way.
    pub rules: PackageRules,
    /// The registry's live team roster, shared with the `team:` tokens in
    /// [`Self::rules`] and served by the npm team API
    /// (`GET /-/org/{scope}/team`, `GET /-/team/{scope}/{team}/user`).
    pub teams: TeamDirectory,
    pub teams_managed_by: Management,
    pub rules_managed_by: Management,
}

impl HostedConfig {
    /// A registry without teams.
    #[must_use]
    pub fn new(org: impl Into<String>, rules: PackageRules) -> Self {
        Self {
            org: org.into(),
            rules,
            teams: TeamDirectory::default(),
            teams_managed_by: Management::Config,
            rules_managed_by: Management::Config,
        }
    }
}

/// Who owns a part of a hosted registry's configuration: its team roster
/// (`teamsManagedBy`) or its package rules (`rulesManagedBy`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Management {
    /// The YAML is the only source, and the admin API rejects writes.
    #[default]
    Config,
    /// The hosted store holds what admins change through the admin API. The
    /// YAML is the starting point until the first change is stored.
    Api,
}
