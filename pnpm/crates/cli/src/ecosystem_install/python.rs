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
/// and the install. A `requirements.txt` beside a `pyproject.toml` is that
/// project's own file, so only a directory without one is a project of its
/// own.
pub(crate) async fn discover(
    config: &'static Config,
    inventory: &EcosystemWorkspaceInventory,
) -> Result<Discovery> {
    let mut manifests = inventory.manifests(EcosystemManifest::Python).await?.to_vec();
    for path in inventory.manifests(EcosystemManifest::Requirements).await? {
        if !manifests.contains(&path.with_file_name("pyproject.toml")) {
            manifests.push(path.clone());
        }
    }
    pnpm_python_installer::discover(config, manifests).await
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
    let matched = matching_projects(config, prefix, discovery)?;
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

/// The Python projects the `--filter` / `--filter-prod` selectors select. A
/// recursive run without a selector selects every one of them, as it does
/// every npm project.
fn matching_projects(
    config: &Config,
    prefix: &Path,
    discovery: &Discovery,
) -> Result<HashSet<PathBuf>> {
    if config.filter.is_empty() && config.filter_prod.is_empty() {
        return Ok(discovery
            .project_roots()
            .map(Path::to_path_buf)
            .collect());
    }
    let graph = discovery.graph();
    let options = recursive_filter_options(config, prefix);
    let mut selected: HashSet<PathBuf> =
        filter_against(&graph, &config.filter, None, false, prefix, &options)?
            .into_iter()
            .collect();
    // `[tool.uv.sources]` says where a requirement comes from, not which
    // dependency group declares it, so a production selector reaches the
    // same projects a regular one does.
    selected.extend(filter_against(&graph, &config.filter_prod, None, true, prefix, &options)?);
    Ok(selected)
}
