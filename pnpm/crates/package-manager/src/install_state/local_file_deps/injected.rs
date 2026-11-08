//! Injected directories the fast path's manifest comparison cannot vouch
//! for: the gate reads the lockfile for them only when a project, an
//! override, or a package extension names a `workspace:` path.

use super::{
    Lockfile, OptimisticRepeatInstallCheck, PathBuf, dependency_fields,
    specs::{is_effective_workspace_path, is_workspace_path_spec},
};
use pnpm_config_parse_overrides::VersionOverride;
use pnpm_workspace::importer_id_from_root_dir;
use std::collections::HashSet;

/// Whether an override or a package extension adds a `workspace:` path to a
/// project's dependencies, which the manifest scan cannot see. An override's
/// specifier has already been read through the catalogs.
pub(super) fn settings_add_workspace_path(
    check: &OptimisticRepeatInstallCheck<'_>,
    overrides: &[VersionOverride],
) -> bool {
    if overrides.iter().any(|entry| is_workspace_path_spec(&entry.new_bare_specifier)) {
        return true;
    }
    let Some(extensions) = check.config.package_extensions.as_ref() else { return false };
    let optional_included = check.layout.included.optional_dependencies;
    extensions
        .values()
        .flat_map(|extension| {
            let optional = optional_included.then_some(extension.optional_dependencies.as_ref());
            std::iter::once(extension.dependencies.as_ref())
                .chain(optional)
                .flatten()
                .flatten()
        })
        .any(|(alias, spec)| is_effective_workspace_path(check.catalogs, alias, spec))
}

/// Whether an injected dependency the fast path's manifest comparison does
/// not vouch for rules it out. With a shared lockfile every workspace project
/// is an importer, so a change to an injected project's own dependencies is
/// caught by comparing its manifest, but an injected directory outside the
/// workspace patterns has no importer. With per-project lockfiles the
/// consumer's lockfile records every injected package's dependencies, which
/// nothing else checks.
pub(super) fn has_unvouched_injected_dep(
    check: &OptimisticRepeatInstallCheck<'_>,
    lockfile: &Lockfile,
) -> bool {
    let included_groups = dependency_fields(check)
        .into_iter()
        .filter_map(|(_, group, included)| included.then_some(group))
        .collect::<Vec<_>>();
    let project_dirs: Option<HashSet<PathBuf>> = check.config
        .shared_workspace_lockfile
        .then(|| {
            check.project_manifests
                .iter()
                .map(|(project_dir, _)| pnpm_fs::lexical_normalize(project_dir))
                .collect()
        });
    let vouched = |directory: &str| {
        project_dirs
            .as_ref()
            .is_some_and(|project_dirs| {
                project_dirs.contains(&pnpm_fs::lexical_normalize(
                    &check.workspace_root.join(directory),
                ))
            })
    };
    check.project_manifests
        .iter()
        .any(|(project_dir, _)| {
            let importer_id = importer_id_from_root_dir(check.workspace_root, project_dir);
            lockfile
                .injected_dependencies(&importer_id, included_groups.iter().copied())
                .any(|dependency| !vouched(&dependency.directory.directory))
        })
}
