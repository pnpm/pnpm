use super::{Deserialize, Serialize};

/// How the resolver picks a version for a direct dependency when more
/// than one satisfies the wanted range.
///
/// The setting is `'highest' | 'time-based' | 'lowest-direct'`.
///
/// Only direct dependencies are affected by the lowest-version pick;
/// subdependencies are always picked highest. Under
/// [`ResolutionMode::TimeBased`] the resolver additionally constrains
/// subdependencies to versions published no later than the newest
/// resolved direct dependency (plus a one-hour delta).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionMode {
    /// Pick the highest version that satisfies the range, everywhere.
    #[default]
    Highest,

    /// Resolve direct dependencies to their lowest satisfying version,
    /// then resolve subdependencies from versions published before the
    /// last direct dependency was published.
    TimeBased,

    /// Resolve direct dependencies to their lowest satisfying version;
    /// subdependencies are unconstrained (picked highest).
    LowestDirect,
}

impl ResolutionMode {
    /// Whether direct dependencies are resolved to their lowest
    /// satisfying version. True for both [`Self::TimeBased`] and
    /// [`Self::LowestDirect`].
    #[must_use]
    pub fn picks_lowest_direct(self) -> bool {
        matches!(self, ResolutionMode::TimeBased | ResolutionMode::LowestDirect)
    }
}

/// How `pnpm add` / `pnpm update` reconcile a directly-specified version
/// against a `catalog:` entry for the same package.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogMode {
    /// The catalog is consulted only for explicit `catalog:` specifiers;
    /// `add` / `update` never reconcile a direct version against it.
    #[default]
    Manual,

    /// A direct version that disagrees with the matching catalog entry is
    /// an error (`ERR_PNPM_CATALOG_VERSION_MISMATCH`).
    Strict,

    /// A direct version that disagrees with the matching catalog entry is
    /// kept, with a warning; a version that agrees is used from the
    /// catalog.
    Prefer,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageImportMethod {
    /// Try the platform's cheap link tiers in order — hardlink first on
    /// Linux and Windows, clone first elsewhere — and fall back to copying
    /// when none is possible. `deps-restorer::link_file::next_auto_tier`
    /// implements the ladder and carries the rationale.
    #[default]
    Auto,

    /// hard link packages from the store
    Hardlink,

    /// copy packages from the store
    Copy,

    /// clone (AKA copy-on-write or reference link) packages from the store
    Clone,

    /// try to clone packages from the store. If cloning is not supported then fall back to copying
    CloneOrCopy,
}
