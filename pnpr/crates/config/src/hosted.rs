use super::{PackageRules, TeamDirectory};

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

/// Where a server takes its registry graph from.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum GraphSource {
    /// The metadata database: the server seeds it on first start and follows
    /// every stored revision. Every config file selects this.
    #[default]
    Database,
    /// [`crate::Config::routing`] as an embedder, such as a test, built it in
    /// code. The server serves it without reading a graph from the database,
    /// under the same policies. No config file selects this.
    Supplied,
}
