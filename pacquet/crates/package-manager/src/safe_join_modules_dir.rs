//! Join a `node_modules` directory with a dependency alias and reject
//! aliases that aren't valid npm package names before the join, because
//! the alias becomes a directory name inside `node_modules`. Mirrors
//! pnpm's
//! [`safeJoinModulesDir`](https://github.com/pnpm/pnpm/blob/main/fs/symlink-dependency/src/safeJoinModulesDir.ts)
//! and routes through the same [`is_valid_dependency_alias`]
//! check pacquet applies to direct-dependency aliases at resolution
//! time, so the hoisted restore path enforces the boundary the
//! resolution path already enforces.

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_resolving_deps_resolver::is_valid_dependency_alias;
use std::path::{Path, PathBuf};

/// A dependency alias that would escape `modules` or collide with
/// pnpm's own `node_modules` layout. Surfaces pnpm's
/// `ERR_PNPM_INVALID_DEPENDENCY_NAME`.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("Refusing to place a dependency under {} with the invalid alias {alias:?}", modules.display())]
#[diagnostic(code(INVALID_DEPENDENCY_NAME))]
pub struct InvalidDependencyAliasError {
    pub modules: PathBuf,
    #[error(not(source))]
    pub alias: String,
}

/// `modules.join(alias)` guarded by a package-name validity check.
/// Returns [`InvalidDependencyAliasError`] when `alias` is not a valid
/// npm package name.
pub fn safe_join_modules_dir(
    modules: &Path,
    alias: &str,
) -> Result<PathBuf, InvalidDependencyAliasError> {
    if !is_valid_dependency_alias(alias) {
        return Err(InvalidDependencyAliasError {
            modules: modules.to_path_buf(),
            alias: alias.to_owned(),
        });
    }
    Ok(modules.join(alias))
}

#[cfg(test)]
mod tests;
