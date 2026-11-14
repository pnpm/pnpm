//! Which Python projects an install acts on.

use super::{EcosystemManifest, EcosystemWorkspaceInventory};
use crate::cli_args::{
    pipelines::WorkspaceScope,
    recursive::{filter_graphs, parse_configured_selectors, recursive_filter_options},
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
    let manifests = workspace_manifests(inventory).await?;
    pnpm_python_installer::discover(config, inventory.workspace_root(), &manifests).await
}

/// Every Python project in the workspace, and the one at `project` whether
/// or not the walk reached it. An add edits that one, and reads the
/// workspace around it for the members it may take from the repository
/// and the environment it may share.
pub(crate) async fn discover_around(
    config: &'static Config,
    inventory: &EcosystemWorkspaceInventory,
    project: &Path,
) -> Result<Discovery> {
    discover_around_projects(config, inventory, &[project.to_path_buf()]).await
}

/// [`discover_around`] for a batch of target directories sharing one
/// workspace inventory.
pub(crate) async fn discover_around_projects(
    config: &'static Config,
    inventory: &EcosystemWorkspaceInventory,
    projects: &[PathBuf],
) -> Result<Discovery> {
    let mut manifests = workspace_manifests(inventory).await?;
    for project in projects {
        let manifest = project.join("pyproject.toml");
        if !manifests.contains(&manifest) {
            manifests.push(manifest);
        }
    }
    pnpm_python_installer::discover(config, inventory.workspace_root(), &manifests).await
}

async fn workspace_manifests(inventory: &EcosystemWorkspaceInventory) -> Result<Vec<PathBuf>> {
    let mut manifests = inventory.manifests(EcosystemManifest::Python).await?.to_vec();
    for path in inventory.manifests(EcosystemManifest::Requirements).await? {
        // A `requirements.txt` beside a `pyproject.toml` is that project's
        // own file, so only a directory without one is a project of its own.
        if !manifests.contains(&path.with_file_name("pyproject.toml")) {
            manifests.push(path.clone());
        }
    }
    Ok(manifests)
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
    let selectors = parse_configured_selectors(config, root_selector, prefix);
    let prod_graph = (!config.filter_prod.is_empty()).then(|| discovery.production_graph());
    let filtered = filter_graphs(&discovery.graph(), prod_graph.as_ref(), &selectors, &options)?;
    Ok(filtered.selected_projects.into_iter().collect())
}
