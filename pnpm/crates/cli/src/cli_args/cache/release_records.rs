//! The cached release records of packages read from pnpr's own package
//! document, kept under [`RELEASE_RECORDS_DIR`] as
//! `<registry>/<package>/<version>+r<revision>.json`.

use super::{CacheCommand, walk_metadata_files};
use miette::IntoDiagnostic;
use pnpm_config::Config;
use pnpm_resolving_npm_resolver::mirror::RELEASE_RECORDS_DIR;
use std::fs;

/// Delete the cached release records of `packages` (every package when
/// empty) under the registry directory `registry_prefix`, and list them.
pub(super) fn delete(
    config: &Config,
    registry_prefix: &str,
    packages: &[String],
) -> miette::Result<Vec<String>> {
    let dir = config.cache_dir.join(RELEASE_RECORDS_DIR);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let patterns = if packages.is_empty() {
        vec![format!("{registry_prefix}/**/*.json")]
    } else {
        packages
            .iter()
            .map(|pkg| {
                CacheCommand::reject_path_traversal(pkg)?;
                Ok(format!("{registry_prefix}/{pkg}/*.json"))
            })
            .collect::<miette::Result<Vec<_>>>()?
    };
    let mut deleted = Vec::new();
    for pattern in patterns {
        for (relative, full_path) in walk_metadata_files(&dir, &pattern)? {
            fs::remove_file(full_path).into_diagnostic()?;
            deleted.push(relative);
        }
    }
    Ok(deleted)
}
