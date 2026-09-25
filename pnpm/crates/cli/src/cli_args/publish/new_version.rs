//! `--new-version`: set the package version before publishing, mirroring
//! `pnpm version <version> --no-git-tag-version` followed by
//! `pnpm publish`. Only an exact semver version is accepted; the bump
//! keywords stay exclusive to `pnpm version`.

use super::PublishArgs;
use crate::cli_args::recursive::{
    AutoExcludeRoot, discover_workspace_projects, select_recursive_projects,
};
use derive_more::{Display, Error};
use miette::{Context, Diagnostic};
use node_semver::Version;
use pnpm_config::Config;
use pnpm_package_manifest::PackageManifest;
use pnpm_publish::is_tarball_path;
use serde_json::Value;
use std::path::Path;

/// Errors of the `--new-version` flag.
#[derive(Debug, Display, Error, Diagnostic)]
enum NewVersionError {
    #[display("Invalid version argument: {raw}. Must be a valid semver version (e.g. 1.2.3)")]
    #[diagnostic(code(ERR_PNPM_INVALID_VERSION_BUMP))]
    InvalidVersion { raw: String },

    #[display("--new-version cannot be used when publishing a tarball")]
    #[diagnostic(code(ERR_PNPM_NEW_VERSION_WITH_TARBALL))]
    Tarball,
}

impl PublishArgs {
    /// Reject an invalid `--new-version` value and the tarball combination
    /// before anything is changed or uploaded.
    pub(super) fn validate_new_version(&self) -> miette::Result<()> {
        let Some(raw) = &self.flags.manifest.new_version else { return Ok(()) };
        if self.package.as_deref().is_some_and(is_tarball_path) {
            return Err(NewVersionError::Tarball.into());
        }
        parse_new_version(raw)?;
        Ok(())
    }

    /// Write the `--new-version` value into the manifest of every package this
    /// run covers (the project directory, or every selected workspace package
    /// in recursive mode) so the pack and the already-published probe see the
    /// new version. Runs after the git checks: like
    /// `pnpm version --no-git-tag-version`, the rewrite leaves the working tree
    /// dirty on purpose.
    pub(super) fn apply_new_version(
        &self,
        dir: &Path,
        config: &Config,
        recursive: bool,
    ) -> miette::Result<()> {
        let Some(raw) = &self.flags.manifest.new_version else { return Ok(()) };
        let new_version = parse_new_version(raw)?;
        if !recursive {
            let project_dir = self.package
                .as_deref()
                .map_or_else(|| dir.to_path_buf(), |path| dir.join(path));
            return set_package_version(&project_dir, &new_version);
        }
        let base = config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
        let (projects, _) = discover_workspace_projects(&base, config)?;
        let selection =
            select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?;
        for pkg_dir in selection.selected.keys() {
            set_package_version(pkg_dir, &new_version)?;
        }
        Ok(())
    }
}

/// The normalized `--new-version` value: a valid semver version, with a
/// leading `v` stripped, as in `pnpm version`'s explicit-version parsing.
fn parse_new_version(raw: &str) -> miette::Result<String> {
    Version::parse(raw)
        .map(|version| version.to_string())
        .map_err(|_| NewVersionError::InvalidVersion { raw: raw.to_owned() }.into())
}

/// Set `version` in the manifest at `pkg_dir`. A directory without a manifest
/// is left for the pack to report; a nameless manifest is skipped, as
/// `pnpm version` skips it.
fn set_package_version(pkg_dir: &Path, new_version: &str) -> miette::Result<()> {
    let manifest_path = pnpm_workspace::project_manifest_path(pkg_dir);
    if !manifest_path.exists() {
        return Ok(());
    }
    let mut manifest = PackageManifest::from_path(manifest_path.clone())
        .wrap_err_with(|| format!("reading {}", manifest_path.display()))?;
    let named = manifest
        .value()
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(|name| !name.is_empty());
    if !named {
        return Ok(());
    }
    manifest
        .value_mut()
        .as_object_mut()
        .expect("the manifest is an object, its name field was just read")
        .insert("version".to_owned(), Value::String(new_version.to_owned()));
    manifest
        .save()
        .wrap_err_with(|| format!("saving {}", manifest_path.display()))
}

#[cfg(test)]
mod tests;
