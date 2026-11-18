use super::PackageRules;

/// A resolved hosted registry: the storage `namespace` it serves and the
/// package patterns it claims.
#[derive(Debug, Clone)]
pub struct HostedConfig {
    /// The storage/serving namespace, so two hosted registries holding the same
    /// `name@version` never collide. Empty (`""`) ⇒ the default namespace.
    pub namespace: String,
    /// The patterns the registry claims and the grants that cover it. Whether
    /// a grant lets a caller read a package gates reads and the write routing
    /// (publish, dist-tag, unpublish) alike, with a denied caller masked as
    /// not-found either way.
    pub rules: PackageRules,
}

impl HostedConfig {
    #[must_use]
    pub fn new(namespace: impl Into<String>, rules: PackageRules) -> Self {
        Self { namespace: namespace.into(), rules }
    }
}

/// Where a server takes its registry graph from.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum GraphSource {
    /// The database: the server seeds it on first start and follows
    /// every stored revision. Every config file selects this.
    #[default]
    Database,
    /// [`crate::Config::routing`] as an embedder, such as a test, built it in
    /// code. The server serves it without reading a graph from the database,
    /// under the same policies. No config file selects this.
    Supplied,
}
