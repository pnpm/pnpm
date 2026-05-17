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
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, From, Into,
)]
#[serde(transparent)]
pub struct PkgIdWithPatchHash(String);

impl PkgIdWithPatchHash {
    /// Borrow the underlying string. Mirrors `pacquet_modules_yaml::DepPath::as_str`.
    #[inline]
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
mod tests {
    use super::PkgIdWithPatchHash;
    use pretty_assertions::assert_eq;

    /// `#[serde(transparent)]` guarantees the on-disk shape is the
    /// raw string — no `{ "0": "..." }` wrapping, no struct
    /// indirection. Pins both sides so a future refactor that
    /// drops the attribute would surface here rather than in some
    /// downstream `.modules.yaml` consumer that suddenly can't
    /// parse pacquet's output.
    #[test]
    fn serde_round_trip_matches_plain_string() {
        let original = PkgIdWithPatchHash::from("foo@1.0.0(patch_hash=abc)");
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(json, r#""foo@1.0.0(patch_hash=abc)""#);
        let round_tripped: PkgIdWithPatchHash = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, original);
    }

    /// Per upstream rule (and `CLAUDE.md` rule 3 for non-validating
    /// brands), construction must be infallible. Pin both
    /// `From<String>` (via `derive_more`) and `From<&str>` (manual
    /// impl) so call sites can choose without intermediate
    /// allocations.
    #[test]
    fn constructs_from_string_and_str_without_validation() {
        let from_string = PkgIdWithPatchHash::from(String::from("bar@2.0.0"));
        let from_str = PkgIdWithPatchHash::from("bar@2.0.0");
        assert_eq!(from_string, from_str);
        // The "obviously not a real ident" case must still go
        // through — non-validating means non-validating.
        let nonsense = PkgIdWithPatchHash::from("");
        assert_eq!(nonsense.as_str(), "");
    }
}
