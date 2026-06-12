//! The `catalogs:` block of a v9 lockfile.
//!
//! Ports pnpm's
//! [`CatalogSnapshots`](https://github.com/pnpm/pnpm/blob/e7e99f04e4/lockfile/types/src/index.ts#L196-L210):
//! for every catalog-referenced direct dependency, the lockfile records both
//! the workspace-manifest specifier (`^1.2.3`) and the version it resolved to,
//! so a later install can verify the catalog without re-reading
//! `pnpm-workspace.yaml`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `catalogName → { dependencyName → entry }`. A [`BTreeMap`] so the entries
/// serialize in pnpm's sorted key order (`sortDirectKeys` / `sortDeepKeys`).
pub type CatalogSnapshots = BTreeMap<String, BTreeMap<String, ResolvedCatalogEntry>>;

/// One resolved catalog entry: the manifest specifier plus the version it
/// resolved to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedCatalogEntry {
    /// The specifier recorded under the catalog in `pnpm-workspace.yaml`
    /// (e.g. the `^1.2.3` of `catalog: { foo: ^1.2.3 }`).
    pub specifier: String,
    /// The concrete version the specifier resolved to.
    pub version: String,
}
