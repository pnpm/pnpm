//! Which Python projects an install acts on.

use super::{EcosystemManifest, EcosystemWorkspaceInventory};
use crate::cli_args::{
    pipelines::WorkspaceScope,
    recursive::{filter_against, recursive_filter_options},
};
use miette::Result;
use pnpm_config::Config;
use pnpm_python_installer::Discovery;
use std::{
    collections::{BTreeSet, HashSet},
    path::{Path, PathBuf},
};

/// Every Python project in the workspace, read once for both the selection
/// and the install.
pub(crate) async fn discover(
    config: &'static Config,
    inventory: &EcosystemWorkspaceInventory,
) -> Result<Discovery> {
    let mut manifests = inventory.manifests(EcosystemManifest::Python).await?.to_vec();
    for path in inventory.manifests(EcosystemManifest::Requirements).await? {
        // A `requirements.txt` beside a `pyproject.toml` is that project's
        // own file, so only a directory without one is a project of its own.
        if !manifests.contains(&path.with_file_name("pyproject.toml")) {
            manifests.push(path.clone());
        }
    }
    pnpm_python_installer::discover(config, &manifests).await
}

/// The Python projects the workspace selection asks for.
///
/// A project sharing a directory with an npm workspace project is installed
/// exactly when that project is selected: that directory's name in the
/// workspace is the npm project's. A project in a directory of its own is
/// matched by the `--filter` selectors themselves, against its distribution
/// name, its path, and the projects it declares a source for.
pub(crate) fn selected_projects(
    config: &Config,
    prefix: &Path,
    discovery: &Discovery,
    scope: Option<&WorkspaceScope>,
) -> Result<BTreeSet<PathBuf>> {
    let Some(scope) = scope else {
        return Ok(discovery
            .project_roots()
            .map(Path::to_path_buf)
            .collect());
    };
    let matched = matching_projects(config, prefix, discovery, scope.root_selector.as_deref())?;
    Ok(discovery
        .project_roots()
        .filter(|root| {
            if scope.projects.contains(*root) {
                scope.selected.contains(*root)
            } else {
                matched.contains(*root)
            }
        })
        .map(Path::to_path_buf)
        .collect())
}

/// The Python projects the `--filter` / `--filter-prod` selectors select,
/// together with the `{<workspace-root>}` selector pnpm generates for the
/// run. A recursive run those leave unnarrowed selects every Python project,
/// as it does every npm project.
fn matching_projects(
    config: &Config,
    prefix: &Path,
    discovery: &Discovery,
    root_selector: Option<&str>,
) -> Result<HashSet<PathBuf>> {
    if config.filter.is_empty() && config.filter_prod.is_empty() && root_selector.is_none() {
        return Ok(discovery
            .project_roots()
            .map(Path::to_path_buf)
            .collect());
    }
    let options = recursive_filter_options(config, prefix);
    // The generated selector follows the pass a `--filter-prod` selector
    // routes the run through, as it does for npm.
    let prod = !config.filter_prod.is_empty();
    let mut selected: HashSet<PathBuf> = filter_against(
        &discovery.graph(),
        &config.filter,
        root_selector.filter(|_| !prod),
        false,
        prefix,
        &options,
    )?
    .into_iter()
    .collect();
    if prod {
        selected.extend(filter_against(
            &discovery.production_graph(),
            &config.filter_prod,
            root_selector.filter(|_| prod),
            true,
            prefix,
            &options,
        )?);
    }
    Ok(selected)
}
