/// Branded depPath string. Mirrors pnpm's
/// [`DepPath`](https://github.com/pnpm/pnpm/blob/097983fbca/packages/types/src/misc.ts).
///
/// Upstream's `DepPath` is a `string` brand — it's never validated at
/// the boundary, just used to keep depPath strings from getting mixed
/// up with arbitrary strings in the type system. Pacquet's port
/// therefore exposes infallible `From<String>` / `From<&str>`
/// constructors and skips a validating `TryFrom`.
///
/// The newtype lives in `pacquet-deps-path` (not in the higher-level
/// resolver crate) so that lower-level helpers (peer-id construction,
/// suffix scanning, filename escaping) can speak in depPath terms
/// without forcing a back-dependency from `deps-path` to the resolver.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DepPath(String);

impl DepPath {
    /// Borrow the underlying depPath string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DepPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for DepPath {
    fn from(value: String) -> DepPath {
        DepPath(value)
    }
}

impl From<&str> for DepPath {
    fn from(value: &str) -> DepPath {
        DepPath(value.to_string())
    }
}
