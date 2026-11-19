//! `--new-version`: set the package version before publishing, mirroring
//! `pnpm version <version> --no-git-tag-version` followed by
//! `pnpm publish`. Only an exact semver version is accepted; the bump
//! keywords stay exclusive to `pnpm version`.

use super::PublishArgs;
use crate::cli_args::{
    recursive::{
        AutoExcludeRoot, SelectedProjectDirs, discover_workspace_projects,
        select_recursive_projects,
    },
    version::{parse_exact_version, set_manifest_version},
};
use derive_more::{Display, Error};
use miette::{Context, Diagnostic};
use pnpm_config::Config;
use pnpm_package_manifest::PackageManifest;
use pnpm_publish::is_tarball_path;
use pnpm_workspace::Project;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Arc,
};

#[derive(Debug, Display, Error, Diagnostic)]
enum NewVersionError {
    #[display("Invalid version argument: {raw}. Must be a valid semver version (e.g. 1.2.3)")]
    #[diagnostic(code(ERR_PNPM_INVALID_VERSION_BUMP))]
    InvalidVersion { raw: String },

    #[display("--new-version cannot be used when publishing a tarball")]
    #[diagnostic(code(ERR_PNPM_NEW_VERSION_WITH_TARBALL))]
    Tarball,
}

/// The names of the workspace packages whose manifests `--new-version`
/// rewrote, for the exportable manifest's `prefer_workspace_for` lookup.
pub(super) type BumpedPackages = Arc<HashSet<String>>;

impl PublishArgs {
    /// Reject an invalid `--new-version` value and the tarball combination
    /// before anything is changed or uploaded.
    pub(super) fn validate_new_version(&self) -> miette::Result<()> {
        if self.flags.manifest.new_version.is_some()
            && self.package.as_deref().is_some_and(is_tarball_path)
        {
            return Err(NewVersionError::Tarball.into());
        }
        self.new_version()?;
        Ok(())
    }

    /// The normalized `--new-version` value, with a leading `v` stripped as
    /// `pnpm version` strips it.
    pub(super) fn new_version(&self) -> miette::Result<Option<String>> {
        let Some(raw) = &self.flags.manifest.new_version else { return Ok(None) };
        parse_exact_version(raw)
            .map(|version| Some(version.to_string()))
            .ok_or_else(|| NewVersionError::InvalidVersion { raw: raw.to_owned() }.into())
    }

    /// Write the `--new-version` value into the manifest of the package
    /// published from `dir`. Runs after the git checks: like
    /// `pnpm version --no-git-tag-version`, the rewrite leaves the working
    /// tree dirty on purpose.
    pub(super) fn apply_new_version(&self, dir: &Path) -> miette::Result<Option<BumpedPackages>> {
        let Some(new_version) = self.new_version()? else { return Ok(None) };
        let project_dir = self.package
            .as_deref()
            .map(|path| dir.join(path))
            .unwrap_or_else(|| dir.to_path_buf());
        let manifest_path = pnpm_workspace::project_manifest_path(&project_dir);
        let mut bumped = HashSet::new();
        if manifest_path.exists() {
            let mut manifest = PackageManifest::from_path(manifest_path.clone())
                .wrap_err_with(|| format!("reading {}", manifest_path.display()))?;
            if let Some(name) = write_package_version(&mut manifest, &new_version)? {
                bumped.insert(name);
            }
        }
        Ok(Some(Arc::new(bumped)))
    }

    /// Discover the workspace, select the `--filter`ed projects, and write
    /// the `--new-version` value into the selected ones. The selection is
    /// made before the bump, which can change the graph edges.
    pub(super) fn select_and_bump_projects(
        &self,
        dir: &Path,
        workspace_root: &Path,
        config: &Config,
    ) -> miette::Result<(Vec<Project>, SelectedProjectDirs, Option<BumpedPackages>)> {
        // `publish` is not in pnpm's root-auto-exclusion command set
        // (`run` / `exec` / `add` / `test`), so the workspace root stays in the
        // selection; its own name/version/private eligibility check drops it
        // from publishing.
        let (mut projects, _patterns) = discover_workspace_projects(workspace_root, config)?;
        let selected_dirs =
            select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?
                .selected_dirs();
        let bumped = self
            .new_version()?
            .map(|new_version| set_projects_version(&mut projects, &selected_dirs, &new_version))
            .transpose()?;
        Ok((projects, selected_dirs, bumped))
    }
}

/// Write `new_version` into every selected project, on disk and in
/// `projects`, as `pnpm version -r` does. A name that several workspace
/// projects share is left out of the result: the workspace lookup is keyed
/// by name and cannot tell which of them was bumped.
pub(super) fn set_projects_version(
    projects: &mut [Project],
    selected: &SelectedProjectDirs,
    new_version: &str,
) -> miette::Result<BumpedPackages> {
    let mut name_counts = HashMap::<String, usize>::new();
    for project in projects.iter() {
        if let Some(name) = package_name(&project.manifest) {
            *name_counts.entry(name).or_default() += 1;
        }
    }
    let mut bumped = HashSet::new();
    for project in projects
        .iter_mut()
        .filter(|project| selected.contains(&project.root_dir))
    {
        if let Some(name) = write_package_version(&mut project.manifest, new_version)? {
            bumped.insert(name);
        }
    }
    bumped.retain(|name| name_counts.get(name) == Some(&1));
    Ok(Arc::new(bumped))
}

/// Set and save `version` in a named `manifest`, and in the manifest of its
/// `publishConfig.directory` when that directory already holds one, since
/// the pack reads the published version from there. Returns the package's
/// name, or `None` for a nameless manifest, which `pnpm version` skips too.
fn write_package_version(
    manifest: &mut PackageManifest,
    new_version: &str,
) -> miette::Result<Option<String>> {
    let Some(name) = package_name(manifest) else { return Ok(None) };
    set_manifest_version(manifest, new_version);
    save_manifest(manifest)?;
    let publish_dir = manifest
        .value()
        .get("publishConfig")
        .and_then(|config| config.get("directory"))
        .and_then(Value::as_str)
        .filter(|directory| !directory.is_empty())
        .zip(manifest.path().parent())
        .map(|(directory, project_dir)| project_dir.join(directory));
    if let Some(publish_dir) = publish_dir {
        let manifest_path = pnpm_workspace::project_manifest_path(&publish_dir);
        if manifest_path.exists() {
            let mut published = PackageManifest::from_path(manifest_path.clone())
                .wrap_err_with(|| format!("reading {}", manifest_path.display()))?;
            set_manifest_version(&mut published, new_version);
            save_manifest(&mut published)?;
        }
    }
    Ok(Some(name))
}

fn package_name(manifest: &PackageManifest) -> Option<String> {
    manifest
        .value()
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

fn save_manifest(manifest: &mut PackageManifest) -> miette::Result<()> {
    let path = manifest.path().to_path_buf();
    manifest
        .save()
        .wrap_err_with(|| format!("saving {}", path.display()))
}

#[cfg(test)]
mod tests;
