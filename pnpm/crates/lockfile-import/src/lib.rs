//! Reads another package manager's lockfile and reports the versions it
//! pins, so `pnpm import` can hand them to the resolver as preferences
//! and reproduce the source lockfile's picks instead of resolving every
//! range afresh.
//!
//! Three source files are supported, searched in this order:
//! `yarn.lock`, `package-lock.json`, `npm-shrinkwrap.json`.
//!
//! The extracted versions are advisory, not authoritative. They become
//! plain `version` selectors, so a version the source lockfile pinned
//! wins a tie among the versions a range allows, and a version no longer
//! published is ignored rather than fatal.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_resolving_resolver_base::{PreferredVersions, VersionSelectorEntry, VersionSelectorType};

mod npm;
mod yarn;

pub use npm::collect_npm_lockfile_versions;
pub use yarn::{YarnSyntaxError, collect_yarn_lockfile_versions};

/// Yarn's lockfile name, covering both yarn classic and yarn berry.
pub const YARN_LOCKFILE_NAME: &str = "yarn.lock";

/// npm's lockfile name.
pub const NPM_LOCKFILE_NAME: &str = "package-lock.json";

/// npm's publishable lockfile name, read when [`NPM_LOCKFILE_NAME`] is
/// absent.
pub const NPM_SHRINKWRAP_NAME: &str = "npm-shrinkwrap.json";

/// Every version string a foreign lockfile associates with a package
/// name. A value is usually a concrete version, but npm's flat format
/// stores ranges in the same slots, so ranges land here too.
pub type VersionsByPackageName = BTreeMap<String, BTreeSet<String>>;

/// Failures reading a foreign lockfile. The variants carrying a
/// `PnpmError` code match the codes the TypeScript CLI raises for the
/// same conditions.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ImportLockfileError {
    #[display("No lockfile found")]
    #[diagnostic(
        code(ERR_PNPM_LOCKFILE_NOT_FOUND),
        help(
            r#""pnpm import" reads a lockfile written by another package manager. Run it in a directory containing yarn.lock, package-lock.json, or npm-shrinkwrap.json."#
        )
    )]
    LockfileNotFound,

    #[display("Yarn.lock file was conflict")]
    #[diagnostic(
        code(ERR_PNPM_YARN_LOCKFILE_PARSE_FAILED),
        help(r#"Resolve the merge conflict in yarn.lock, then run "pnpm import" again."#)
    )]
    YarnLockfileConflict,

    #[display("Failed to parse {path:?}")]
    #[diagnostic(code(ERR_PNPM_YARN_LOCKFILE_PARSE_FAILED))]
    YarnParse {
        #[error(not(source))]
        path: PathBuf,
        source: yarn::YarnSyntaxError,
    },

    #[display("Failed to read {path:?}")]
    Read {
        #[error(not(source))]
        path: PathBuf,
        source: std::io::Error,
    },

    #[display("Failed to parse {path:?}")]
    Parse {
        #[error(not(source))]
        path: PathBuf,
        source: serde_json::Error,
    },
}

/// Collect the versions pinned by the first foreign lockfile found in
/// `dir`.
pub fn read_foreign_lockfile_versions(
    dir: &Path,
) -> Result<VersionsByPackageName, ImportLockfileError> {
    let mut versions = VersionsByPackageName::new();

    let yarn_lockfile_path = dir.join(YARN_LOCKFILE_NAME);
    if let Some(contents) = read_if_exists(&yarn_lockfile_path)? {
        if contents.lines().any(|line| line.starts_with("<<<<<<<")) {
            return Err(ImportLockfileError::YarnLockfileConflict);
        }
        collect_yarn_lockfile_versions(&contents, &mut versions).map_err(|source| {
            ImportLockfileError::YarnParse { path: yarn_lockfile_path, source }
        })?;
        return Ok(versions);
    }

    for lockfile_name in [NPM_LOCKFILE_NAME, NPM_SHRINKWRAP_NAME] {
        let path = dir.join(lockfile_name);
        if let Some(contents) = read_if_exists(&path)? {
            let lockfile = serde_json::from_str(&contents)
                .map_err(|source| ImportLockfileError::Parse { path, source })?;
            collect_npm_lockfile_versions(&lockfile, &mut versions);
            return Ok(versions);
        }
    }

    Err(ImportLockfileError::LockfileNotFound)
}

/// Turn collected versions into resolver preferences.
///
/// Every version becomes a plain `version` selector, matching the
/// TypeScript CLI. A range collected from npm's flat format therefore
/// only takes effect when it happens to name a published version; the
/// resolver drops the rest.
#[must_use]
pub fn to_preferred_versions(versions: &VersionsByPackageName) -> PreferredVersions {
    versions
        .iter()
        .map(|(name, versions)| {
            let selectors = versions
                .iter()
                .map(|version| {
                    (version.clone(), VersionSelectorEntry::Plain(VersionSelectorType::Version))
                })
                .collect();
            (name.clone(), selectors)
        })
        .collect()
}

fn read_if_exists(path: &Path) -> Result<Option<String>, ImportLockfileError> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ImportLockfileError::Read { path: path.to_path_buf(), source }),
    }
}

fn add_version(versions: &mut VersionsByPackageName, name: &str, version: &str) {
    if name.is_empty() || version.is_empty() {
        return;
    }
    versions.entry(name.to_string()).or_default().insert(version.to_string());
}

#[cfg(test)]
mod tests;
