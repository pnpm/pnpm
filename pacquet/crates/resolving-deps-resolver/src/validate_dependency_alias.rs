//! Reject dependency aliases that aren't valid npm package names —
//! anything else, joined with a `node_modules` path, can either escape
//! the intended directory (`@x/../../../../../.git/hooks`) or collide
//! with pnpm's own layout (`.bin`, `.pnpm`, `node_modules`). Mirrors
//! pnpm's
//! [`isValidDependencyAlias`](https://github.com/pnpm/pnpm/blob/main/installing/deps-resolver/src/validateDependencyAlias.ts),
//! which routes through the same `validate-npm-package-name`
//! `validForOldPackages` check that `parse_wanted_dependency` applies
//! to CLI-given names.

use pacquet_resolving_parse_wanted_dependency::is_valid_old_npm_package_name;

/// `true` when `alias` is a valid npm package name that pnpm can safely
/// use as a `node_modules/<alias>` directory.
#[must_use]
pub fn is_valid_dependency_alias(alias: &str) -> bool {
    is_valid_old_npm_package_name(alias)
}

#[cfg(test)]
mod tests;
