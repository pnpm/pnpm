//! Parses dependency strings before resolver dispatch. This crate splits a
//! raw manifest or `add` argument into its `(alias, bareSpecifier)` halves and
//! compares the supported equivalent forms of Git specifiers.

pub use pnpm_package_name as validate_npm_package_name;

pub use git_specifier::git_specifiers_are_equivalent;
pub use validate_npm_package_name::is_valid_old_npm_package_name;

use pnpm_pkg_name_split::split_name_suffix;

mod git_specifier;

/// The `(alias, bareSpecifier)` split for a raw dependency string. At
/// least one of the two fields is always populated.
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

/// Searches for the first `@` from index 1 onwards (so the scope-marker
/// `@` of `@scope/foo` is not treated as a version separator). When the
/// substring before that `@` parses as a valid (old-style) npm package
/// name, the split is taken; otherwise the input passes through as a
/// bare specifier.
#[must_use]
pub fn parse_wanted_dependency(raw_wanted_dependency: &str) -> ParsedWantedDependency {
    if let Ok((alias, bare_specifier)) = split_name_suffix(raw_wanted_dependency) {
        let alias = alias.as_str();
        if is_valid_old_npm_package_name(alias) {
            return ParsedWantedDependency {
                alias: Some(alias.to_string()),
                bare_specifier: Some(bare_specifier.to_string()),
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

#[cfg(test)]
mod tests;
