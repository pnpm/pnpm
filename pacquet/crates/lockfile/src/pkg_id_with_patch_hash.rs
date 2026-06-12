use derive_more::{From, Into};
use serde::{Deserialize, Serialize};

/// The patch-aware package ident used by upstream's side-effects
/// cache and dep-graph hashing. Mirrors upstream's
/// [`PkgIdWithPatchHash`](https://github.com/pnpm/pnpm/blob/94240bc046/core/types/src/misc.ts)
/// (`type PkgIdWithPatchHash = string & { __brand: 'PkgIdWithPatchHash' }`).
///
/// The on-disk shape is `<pkg_id>` or `<pkg_id>(patch_hash=<hash>)` —
/// see [`createFullPkgId`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L248-L274)
/// and [`createPatchHash`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/index.ts).
/// The two pacquet consumers ([`crate`]'s downstream
/// `virtual_store_layout` and `hoisted_dep_graph` in
/// `pacquet-package-manager`) build the value by `to_string()`-ing a
/// [`crate::PackageKey`] today; the format is fixed by upstream so
/// no validating constructor is appropriate here.
///
/// Per `CLAUDE.md`'s "Porting branded string types" section rule 3:
/// non-validating brand → infallible `From<String>` / `From<&str>`
/// via [`derive_more::From`] / [`derive_more::Into`], plus
/// `#[serde(transparent)]` so the wire format is identical to
/// `String` (the value crosses JSON / YAML boundaries when it lands
/// inside `.modules.yaml` or a side-effects-cache key).
///
/// Modelled on `pacquet_modules_yaml::DepPath` — the closest
/// existing peer in pacquet, which ports the sibling brand from the
/// same upstream `misc.ts` file under the same rules. Bare-text link
/// rather than an intra-doc link because `pacquet-lockfile` doesn't
/// depend on `pacquet-modules-yaml` and adding the dep just for a
/// rustdoc reference would invert the natural crate ordering.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, From, Into,
)]
#[serde(transparent)]
pub struct PkgIdWithPatchHash(String);

impl PkgIdWithPatchHash {
    /// Borrow the underlying string. Mirrors `pacquet_modules_yaml::DepPath::as_str`.
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
