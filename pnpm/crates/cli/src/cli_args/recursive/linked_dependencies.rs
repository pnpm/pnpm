//! The workspace dependency edges that stay symlinks when every project has
//! its own lockfile and `injectWorkspacePackages` copies the rest.

use super::filter_graphs::configured_overrides_rewriter;
use crate::cli_args::catalogs::{
    configured_catalogs,
    workspace_catalogs,
};
use pnpm_catalogs_resolver::{
    CatalogAnchor,
    CatalogResolutionResult,
    WantedDependency,
    resolve_from_catalog,
};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_fs::lexical_normalize;
use pnpm_package_manager::VersionsOverrider;
use pnpm_workspace::GraphPkg;
use pnpm_workspace_projects_graph::{
    DependencyGroups,
    DependencyRewriter,
    GraphProject,
    WorkspaceCatalogs,
};
use std::path::{
    Path,
    PathBuf,
};

/// The configured catalogs and `overrides` that [`LinkedEdges`] reads
/// specifiers through.
pub(super) struct LinkedEdgesConfig {
    catalogs: Catalogs,
    rewriter: Option<VersionsOverrider>,
}

impl LinkedEdgesConfig {
    pub(super) fn load(config: &Config, prefix: &Path) -> miette::Result<Self> {
        let catalogs = configured_catalogs(config)?;
        let rewriter = configured_overrides_rewriter(config, &catalogs, prefix)?;
        Ok(LinkedEdgesConfig { catalogs, rewriter })
    }

    pub(super) fn edges<'a>(&'a self, config: &'a Config) -> LinkedEdges<'a> {
        LinkedEdges {
            catalogs: workspace_catalogs(config, &self.catalogs),
            rewriter: self.rewriter.as_ref(),
        }
    }
}

/// Keeps the edges whose specifier is `link:` once `overrides` and
/// `catalog:` are applied, the way the project's install reads it.
pub(super) struct LinkedEdges<'a> {
    catalogs: Option<WorkspaceCatalogs<'a>>,
    rewriter: Option<&'a VersionsOverrider>,
}

impl LinkedEdges<'_> {
    /// The members of `deps`, the graph edges of the project at `parent_dir`,
    /// that the project links to.
    pub(super) fn retain<'graph>(
        &self,
        parent_dir: &Path,
        pkg: GraphPkg<'_>,
        deps: &'graph [PathBuf],
    ) -> Vec<&'graph Path> {
        if deps.is_empty() {
            return Vec::new();
        }
        let linked = self.linked_dependency_dirs(parent_dir, pkg);
        deps.iter()
            .map(PathBuf::as_path)
            .filter(|dep_dir| linked.contains(&lexical_normalize(dep_dir)))
            .collect()
    }

    fn linked_dependency_dirs(
        &self,
        parent_dir: &Path,
        pkg: GraphPkg<'_>,
    ) -> rustc_hash::FxHashSet<PathBuf> {
        let mut groups = pkg.dependency_groups(false);
        if let Some(rewriter) = self.rewriter {
            rewriter.rewrite_dependencies(&pkg, &mut groups);
        }
        let DependencyGroups { peer, dev, optional, prod } = groups;
        [peer, dev.unwrap_or_default(), optional, prod]
            .into_iter()
            .flatten()
            .filter_map(|(dep_name, spec)| {
                let resolved = resolve_catalog_spec(parent_dir, &dep_name, &spec, self.catalogs);
                let rel = resolved
                    .as_deref()
                    .unwrap_or(&spec)
                    .strip_prefix("link:")?;
                Some(lexical_normalize(&parent_dir.join(rel.replace('\\', "/"))))
            })
            .collect()
    }
}

fn resolve_catalog_spec(
    consumer_dir: &Path,
    dep_name: &str,
    raw_spec: &str,
    catalogs: Option<WorkspaceCatalogs<'_>>,
) -> Option<String> {
    let catalogs = catalogs?;
    if !raw_spec.starts_with("catalog:") {
        return None;
    }
    let wanted =
        WantedDependency { alias: dep_name.to_string(), bare_specifier: raw_spec.to_string() };
    let anchor = CatalogAnchor::Reanchor {
        workspace_dir: catalogs.workspace_dir,
        consumer_dir: Some(consumer_dir),
    };
    match resolve_from_catalog(catalogs.catalogs, &wanted, anchor) {
        CatalogResolutionResult::Found(found) => Some(found.resolution.specifier),
        CatalogResolutionResult::Misconfiguration(_) | CatalogResolutionResult::Unused => None,
    }
}
