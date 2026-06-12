//! Pacquet port of pnpm's
//! [`@pnpm/resolving.parse-wanted-dependency`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/parse-wanted-dependency/src/index.ts).
//!
//! Splits a raw dependency string from the manifest (or the CLI's `add`
//! argument) into its `(alias, bareSpecifier)` halves so the downstream
//! resolvers can decide which protocol is at play.
//!
//! Examples (mirrors upstream's behavior):
//!
//! - `foo@1.2.3` → `alias = "foo"`, `bare_specifier = "1.2.3"`.
//! - `@scope/foo@1.2.3` → `alias = "@scope/foo"`, `bare_specifier = "1.2.3"`.
//! - `foo@npm:lodash@^4` (npm-alias form) → `alias = "foo"`,
//!   `bare_specifier = "npm:lodash@^4"`.
//! - `git+ssh://git@github.com/owner/repo` → no alias, the whole string
//!   stays in `bare_specifier` (the `@` after `git` doesn't split the
//!   prefix as a valid package name).
//! - `foo` → `alias = "foo"`, no `bare_specifier`.
//! - `^1.2.3` → no alias, the whole string stays in `bare_specifier`.

pub mod validate_npm_package_name;

pub use validate_npm_package_name::is_valid_old_npm_package_name;

/// The `(alias, bareSpecifier)` split for a raw dependency string. At
/// least one of the two fields is always populated; mirrors upstream's
/// `ParseWantedDependencyResult`
/// ([source](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/parse-wanted-dependency/src/index.ts#L8-L13)),
/// which is a union over the three populated shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedWantedDependency {
    /// The local alias the dep should be installed as in `node_modules`,
    /// when present. For `foo@1.2.3` this is `"foo"`; for the npm-alias
    /// form `foo@npm:lodash@^4` it is also `"foo"`.
    pub alias: Option<String>,
    /// The version spec / protocol-prefixed selector the resolver chain
    /// will dispatch on, when present. For `foo@1.2.3` this is
    /// `"1.2.3"`; for `git+ssh://…` it is the whole input.
    pub bare_specifier: Option<String>,
}

/// Port of pnpm's
/// [`parseWantedDependency`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/parse-wanted-dependency/src/index.ts#L15-L37).
///
/// Searches for the first `@` from index 1 onwards (so the scope-marker
/// `@` of `@scope/foo` is not treated as a version separator). When the
/// substring before that `@` parses as a valid (old-style) npm package
/// name, the split is taken; otherwise the input passes through as a
/// bare specifier.
#[must_use]
pub fn parse_wanted_dependency(raw_wanted_dependency: &str) -> ParsedWantedDependency {
    let version_delimiter = find_version_delimiter(raw_wanted_dependency);
    if let Some(idx) = version_delimiter {
        let alias = &raw_wanted_dependency[..idx];
        if is_valid_old_npm_package_name(alias) {
            return ParsedWantedDependency {
                alias: Some(alias.to_string()),
                bare_specifier: Some(raw_wanted_dependency[idx + 1..].to_string()),
            };
        }
        return ParsedWantedDependency {
            alias: None,
            bare_specifier: Some(raw_wanted_dependency.to_string()),
        };
    }
    if is_valid_old_npm_package_name(raw_wanted_dependency) {
        return ParsedWantedDependency {
            alias: Some(raw_wanted_dependency.to_string()),
            bare_specifier: None,
        };
    }
    ParsedWantedDependency { alias: None, bare_specifier: Some(raw_wanted_dependency.to_string()) }
}

/// Find the first `@` byte index strictly after index 0, mirroring
/// upstream's
/// [`indexOf('@', 1)`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/parse-wanted-dependency/src/index.ts#L16).
///
/// Index 0 is skipped so the scope-prefix `@` of `@scope/foo` does not
/// split the input.
fn find_version_delimiter(input: &str) -> Option<usize> {
    input.bytes().enumerate().skip(1).find_map(|(i, b)| (b == b'@').then_some(i))
}

#[cfg(test)]
mod tests;
