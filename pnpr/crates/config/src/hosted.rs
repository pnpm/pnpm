use super::{Deserialize, PackageRules, Serialize, TeamDirectory};

/// A resolved hosted registry: the storage `namespace` it serves and its
/// `packages:` rules — the package patterns it claims plus the per-package
/// `access` / `publish` / `unpublish` policies, with the registry-level
/// `access:` as the default an entry's omitted fields fall back to.
#[derive(Debug, Clone)]
pub struct HostedConfig {
    /// The storage/serving namespace, so two hosted registries holding the same
    /// `name@version` never collide. Empty (`""`) ⇒ the default namespace.
    pub namespace: String,
    /// The registry's `packages:` map: claimed patterns and per-package rules in one
    /// declaration, selected by specificity. The effective `access` gates
    /// reads *and* the write routing (publish, dist-tag, unpublish), with a
    /// denied caller masked as not-found either way.
    pub rules: PackageRules,
    /// The registry's live team roster, shared with the `team:` tokens in
    /// [`Self::rules`] and served by the npm team API
    /// (`GET /-/org/{scope}/team`, `GET /-/team/{scope}/{team}/user`).
    pub teams: TeamDirectory,
}

impl HostedConfig {
    /// A registry without teams.
    #[must_use]
    pub fn new(namespace: impl Into<String>, rules: PackageRules) -> Self {
        Self { namespace: namespace.into(), rules, teams: TeamDirectory::default() }
    }
}

/// Who owns the registry graph (`registriesManagedBy`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Management {
    /// The config file declares the graph, and the admin API rejects writes.
    #[default]
    Config,
    /// The metadata database stores the graph, and the admin API changes it.
    Api,
}

impl Management {
    /// The value as pnpr.yaml spells it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Management::Config => "config",
            Management::Api => "api",
        }
    }
}
