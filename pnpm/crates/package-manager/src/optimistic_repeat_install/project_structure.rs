//! Whether the workspace still has the projects the last install recorded.

use super::{
    Config, LinkWorkspacePackages, PackageManifest, PathBuf,
    local_file_deps::is_workspace_path_spec, manifest_string_field,
};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::PackageExtension;
use pnpm_local_spec::is_local_filesystem_specifier;
use pnpm_package_manifest::DependencyGroup;
use pnpm_workspace_spec::WorkspaceSpec;
use pnpm_workspace_state::WorkspaceState;
use std::collections::HashSet;

/// Project count + per-project (key, name, version) match between the
/// cached state and today's walk. The key is the project's root dir;
/// `build_workspace_state` and pnpm both use it as the map key, so a
/// renamed / removed / added project trips the check immediately.
///
/// A version decides only how dependencies resolve to the project, so a
/// changed one counts only for a project [`ResolvableProjects`] holds.
pub(super) fn project_structure_matches(
    state: &WorkspaceState,
    project_manifests: &[(PathBuf, &PackageManifest)],
    config: &Config,
    catalogs: &Catalogs,
) -> bool {
    if state.projects.len() != project_manifests.len() {
        return false;
    }
    let mut bumped_names = Vec::new();
    for (root_dir, manifest) in project_manifests {
        let Some(entry) = state.projects.get(root_dir.to_string_lossy().as_ref()) else {
            return false;
        };
        let name = manifest_string_field(manifest, "name");
        if entry.name != name {
            return false;
        }
        if entry.version.as_deref().unwrap_or("0.0.0")
            != manifest_string_field(manifest, "version").as_deref().unwrap_or("0.0.0")
        {
            bumped_names.push(name);
        }
    }
    if bumped_names.is_empty() {
        return true;
    }
    let resolvable = ResolvableProjects::new(state, project_manifests, config, catalogs);
    !bumped_names
        .iter()
        .any(|name| resolvable.holds(name.as_deref()))
}

/// The workspace projects a dependency may resolve to by their version. It
/// errs toward holding a project: a version it wrongly ignores lets scripts
/// run against a stale `node_modules`.
enum ResolvableProjects<'a> {
    /// Under [`LinkWorkspacePackages::Deep`] any transitive dependency may
    /// resolve to a project. In a workspace of several projects a pnpmfile
    /// hook may add a dependency on any of them, and a dependency given by a
    /// local path may point at any of them under any alias.
    All,
    /// Outside deep linking, no dependency resolves to a lone project.
    None,
    /// The projects some dependency names, and those a setting mentions.
    Depended { names: HashSet<String>, config: &'a Config },
}

impl<'a> ResolvableProjects<'a> {
    fn new(
        state: &WorkspaceState,
        project_manifests: &[(PathBuf, &PackageManifest)],
        config: &'a Config,
        catalogs: &Catalogs,
    ) -> Self {
        if config.link_workspace_packages == LinkWorkspacePackages::Deep {
            return Self::All;
        }
        if project_manifests.len() == 1 {
            return Self::None;
        }
        if !state.pnpmfiles.is_empty() {
            return Self::All;
        }
        let mut names = HashSet::new();
        for (alias, spec) in dependency_specs(project_manifests, catalogs) {
            if is_local_filesystem_specifier(spec) || is_workspace_path_spec(spec) {
                return Self::All;
            }
            names.insert(depended_name(alias, spec));
        }
        Self::Depended { names, config }
    }

    fn holds(&self, name: Option<&str>) -> bool {
        match self {
            Self::All => true,
            Self::None => false,
            Self::Depended { names, config } => {
                name.is_some_and(|name| names.contains(name) || settings_mention(config, name))
            }
        }
    }
}

/// The `(alias, spec)` pairs of the projects' dependencies and of the
/// catalog entries.
fn dependency_specs<'a>(
    project_manifests: &'a [(PathBuf, &PackageManifest)],
    catalogs: &'a Catalogs,
) -> impl Iterator<Item = (&'a str, &'a str)> {
    const GROUPS: [DependencyGroup; 4] = [
        DependencyGroup::Prod,
        DependencyGroup::Dev,
        DependencyGroup::Optional,
        DependencyGroup::Peer,
    ];
    let manifest_dependencies = project_manifests
        .iter()
        .flat_map(|(_, manifest)| manifest.dependencies(GROUPS));
    let catalog_entries = catalogs
        .values()
        .flatten()
        .map(|(alias, spec)| (alias.as_str(), spec.as_str()));
    manifest_dependencies.chain(catalog_entries)
}

/// The package name a dependency names, through an `npm:` or `workspace:`
/// alias where there is one.
fn depended_name(alias: &str, spec: &str) -> String {
    match WorkspaceSpec::parse(spec) {
        Some(workspace_spec) => workspace_spec.alias.unwrap_or_else(|| alias.to_string()),
        None => PackageManifest::resolve_registry_dependency(alias, spec).0.to_string(),
    }
}

/// Whether an override, a package extension, or a patch mentions `name`
/// anywhere in its selector or its value. Each may select a project by its
/// version or point a dependency at it, and a substring match is the
/// cheapest test that never misses one.
fn settings_mention(config: &Config, name: &str) -> bool {
    let overrides = config.overrides
        .iter()
        .flatten()
        .flat_map(|(selector, spec)| [selector.as_str(), spec.as_str()]);
    let extensions = config.package_extensions
        .iter()
        .flatten()
        .flat_map(|(selector, extension)| {
            std::iter::once(selector.as_str()).chain(extension_dependencies(extension))
        });
    let patches = config.patched_dependencies
        .iter()
        .flatten()
        .map(|(selector, _)| selector.as_str());
    overrides
        .chain(extensions)
        .chain(patches)
        .any(|text| text.contains(name))
}

fn extension_dependencies(extension: &PackageExtension) -> impl Iterator<Item = &str> {
    [&extension.dependencies, &extension.optional_dependencies, &extension.peer_dependencies]
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|(alias, spec)| [alias.as_str(), spec.as_str()])
}

#[cfg(test)]
mod tests;
