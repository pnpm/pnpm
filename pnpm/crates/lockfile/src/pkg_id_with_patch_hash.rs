use derive_more::{From, Into};
use serde::{Deserialize, Serialize};

/// The patch-aware package ident used by pnpm's side-effects cache and
/// dep-graph hashing. A branded string
/// (`type PkgIdWithPatchHash = string & { __brand: 'PkgIdWithPatchHash' }`).
///
/// The on-disk shape is `<pkg_id>` or `<pkg_id>(patch_hash=<hash>)`. The
/// format is fixed by the on-disk contract, so no validating constructor is
/// appropriate here.
///
/// Modelled on `pnpm_modules_yaml::DepPath` — the closest existing
/// peer in pacquet, a sibling brand under the same rules. Bare-text
/// link rather than an intra-doc link because `pnpm-lockfile` doesn't
/// depend on `pnpm-modules-yaml` and adding the dep just for a
/// rustdoc reference would invert the natural crate ordering.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, From, Into,
)]
#[serde(transparent)]
pub struct PkgIdWithPatchHash(String);

impl PkgIdWithPatchHash {
    /// Borrow the underlying string. Mirrors `pnpm_modules_yaml::DepPath::as_str`.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for PkgIdWithPatchHash {
    fn from(value: &str) -> Self {
        PkgIdWithPatchHash(value.to_string())
    }
}

impl std::fmt::Display for PkgIdWithPatchHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests;
