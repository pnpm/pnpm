use super::{VersionError, package_version_identity};
use crate::cli_args::recursive::{
    AutoExcludeRoot, discover_workspace_projects, select_recursive_projects,
};
use miette::Context;
use pnpm_config::Config;
use pnpm_package_manifest::PackageManifest;
use std::{collections::BTreeMap, path::Path};

/// Report the package versions already on disk without running lifecycle
/// hooks or performing the git checks used by version bumps.
pub(super) fn report_current_versions(
    config: &Config,
    dir: &Path,
    recursive: bool,
) -> miette::Result<()> {
    let mut versions = BTreeMap::new();
    if recursive {
        let base = config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
        let (projects, _) = discover_workspace_projects(&base, config)?;
        let selection =
            select_recursive_projects(&projects, config, &base, AutoExcludeRoot::Disabled)?;
        for pkg_dir in selection.selected.keys() {
            read_current_version(pkg_dir, &mut versions)?;
        }
    } else {
        read_current_version(dir, &mut versions)?;
    }
    if versions.is_empty() {
        return Err(VersionError::NoPackagesToVersion.into());
    }
    println!("{}", serde_json::to_string_pretty(&versions).expect("serialize versions"));
    Ok(())
}

fn read_current_version(
    pkg_dir: &Path,
    versions: &mut BTreeMap<String, String>,
) -> miette::Result<()> {
    let manifest_path = pnpm_workspace::project_manifest_path(pkg_dir);
    let manifest = PackageManifest::from_path(manifest_path.clone())
        .wrap_err_with(|| format!("reading {}", manifest_path.display()))?;
    if let Some((name, version)) = package_version_identity(&manifest) {
        versions.insert(name, version);
    }
    Ok(())
}
