use super::super::{
    Config, HashSet, IncludedDependencies, InstallError, Lockfile, Path, PathBuf,
    check_modules_settings_diff,
};

pub(super) fn is_safe_modules_purge_target(modules_dir: &Path, workspace_root: &Path) -> bool {
    modules_dir != workspace_root && modules_dir.starts_with(workspace_root)
}
/// The modules directory a drifted layout has to be rebuilt from.
pub(super) struct InconsistentModulesDir<'a> {
    pub(super) config: &'static Config,
    pub(super) workspace_root: &'a Path,
    pub(super) modules_manifest: Option<&'a pnpm_modules_yaml::ModulesLayout>,
    pub(super) installs_only: bool,
    pub(super) filtered_install: bool,
}
pub(super) fn purge_inconsistent_modules_dir(
    context: &InconsistentModulesDir<'_>,
) -> Result<(), InstallError> {
    // A plain install may recreate the drifted modules dir; `add` / `remove`
    // must surface the drift instead (upstream `validateModules` with
    // `forceNewModules = installsOnly`).
    if !context.installs_only
        && let Some(modules) = context.modules_manifest
    {
        check_modules_settings_diff(modules, context.config)?;
    }
    let (is_safe, target_dir) = purge_target(context.config, context.workspace_root);
    if !is_safe {
        if context.filtered_install {
            return Err(InstallError::UnsafeFilteredModulesDir {
                modules_dir: context.config.modules_dir.clone(),
                workspace_root: context.workspace_root.to_path_buf(),
            });
        }
        tracing::warn!(
            ?context.config.modules_dir,
            "refusing to remove inconsistent modules directory outside the project root",
        );
        return Ok(());
    }
    let Some(target) = target_dir else { return Ok(()) };
    purge_modules_dir_entries(&target, context.config, context.modules_manifest)
}
/// The canonicalized directory the purge may sweep, and whether sweeping it is
/// safe at all. Deleting from the validated path closes the
/// time-of-check/time-of-use gap a symlink swap would otherwise open.
pub(super) fn purge_target(config: &Config, workspace_root: &Path) -> (bool, Option<PathBuf>) {
    if !config.modules_dir.exists() {
        return (true, None);
    }
    match (std::fs::canonicalize(&config.modules_dir), std::fs::canonicalize(workspace_root)) {
        (Ok(modules_canon), Ok(root_canon)) => {
            (is_safe_modules_purge_target(&modules_canon, &root_canon), Some(modules_canon))
        }
        _ => (false, None),
    }
}
pub(super) fn purge_modules_dir_entries(
    target: &Path,
    config: &Config,
    modules_manifest: Option<&pnpm_modules_yaml::ModulesLayout>,
) -> Result<(), InstallError> {
    let read_error = |error| InstallError::ReadModulesDir { path: target.to_path_buf(), error };
    let entries = match std::fs::read_dir(target) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(read_error(error)),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(read_error(error)),
        };
        purge_modules_entry(&entry, config, modules_manifest)?;
    }
    Ok(())
}
/// A hidden entry pnpm does not own is the user's, and is left alone.
pub(super) fn purge_modules_entry(
    entry: &std::fs::DirEntry,
    config: &Config,
    modules_manifest: Option<&pnpm_modules_yaml::ModulesLayout>,
) -> Result<(), InstallError> {
    let file_name = entry.file_name();
    let file_name_str = file_name.to_string_lossy();
    if file_name_str.starts_with('.')
        && !is_pnpm_owned_entry(&file_name_str, config, modules_manifest)
    {
        return Ok(());
    }
    let entry_path = entry.path();
    if let Err(error) = pnpm_fs::remove_dirent(&entry_path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(InstallError::RemoveModulesDir { path: entry_path, error });
    }
    Ok(())
}
pub(super) fn is_pnpm_owned_entry(
    file_name: &str,
    config: &Config,
    modules_manifest: Option<&pnpm_modules_yaml::ModulesLayout>,
) -> bool {
    file_name == ".bin"
        || file_name == ".modules.yaml"
        || config.virtual_store_dir
            .file_name()
            .is_some_and(|name| name == file_name)
        || modules_manifest.is_some_and(|manifest| {
            recorded_virtual_store_name(manifest, config).is_some_and(|name| name == file_name)
        })
}
/// The `node_modules`-relative name the recorded virtual store occupies, when
/// it is inside the modules directory at all.
pub(super) fn recorded_virtual_store_name(
    manifest: &pnpm_modules_yaml::ModulesLayout,
    config: &Config,
) -> Option<std::ffi::OsString> {
    let mut recorded = PathBuf::from(&manifest.virtual_store_dir);
    if recorded.is_relative() {
        recorded = config.modules_dir.join(recorded);
    }
    if !recorded.starts_with(&config.modules_dir) {
        return None;
    }
    recorded.file_name().map(std::ffi::OsStr::to_os_string)
}
/// What decides whether direct links excluded by this run have to be pruned.
pub(super) struct ExcludedGroupPrune<'a> {
    pub(crate) eligibility: crate::install::state_options::PruneEligibility,
    pub(super) config: &'static Config,
    pub(super) workspace_root: &'a Path,
    pub(super) included: IncludedDependencies,
    pub(super) modules_manifest: Option<&'a pnpm_modules_yaml::ModulesLayout>,
    pub(super) current_lockfile: Option<&'a Lockfile>,
    pub(super) requested_importer_ids: Option<&'a HashSet<String>>,
    pub(super) manifest_links: ManifestLinkProjects<'a>,
}
pub(super) struct ManifestLinkProjects<'a> {
    pub(super) manifests: &'a [(PathBuf, &'a pnpm_package_manifest::PackageManifest)],
    pub(super) workspace_packages: Option<&'a pnpm_resolving_resolver_base::WorkspacePackages>,
}
/// Remove direct links from dependency groups excluded by this run.
/// Unfiltered installs can use the global `included` value recorded in
/// `.modules.yaml`; filtered installs may retain importers materialized with
/// different group sets, so they conservatively prune every excluded group
/// from only the selected workspace-link closure.
pub(super) fn prune_excluded_direct_deps(
    context: &ExcludedGroupPrune<'_>,
) -> Result<(), InstallError> {
    if context.eligibility.resolve_only || context.eligibility.is_inconsistent {
        return Ok(());
    }
    let Some(modules) = context.modules_manifest else { return Ok(()) };
    if !context.eligibility.filtered_install && modules.included == context.included {
        return Ok(());
    }
    let selected_prune_importer_ids = selected_prune_importer_ids(context);
    let previously_included = previously_included(context, modules);
    if let Some(current) = context.current_lockfile {
        crate::prune_direct_deps_excluded_by_groups(
            current,
            previously_included,
            context.included,
            context.workspace_root,
            context.config,
            selected_prune_importer_ids.as_ref(),
        )
        .map_err(InstallError::PruneDirectDeps)?;
    }
    crate::prune_manifest_link_deps(&crate::PruneManifestLinkDeps {
        workspace_root: context.workspace_root,
        project_manifests: context.manifest_links.manifests,
        importers: context.current_lockfile.map(|current| &current.importers),
        workspace_packages: context.manifest_links.workspace_packages,
        previously_included,
        new_included: context.included,
        modules_dir_name: context.config.modules_dir
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("node_modules")),
        prunable_importer_ids: selected_prune_importer_ids.as_ref(),
    })
    .map_err(InstallError::PruneDirectDeps)
}

fn previously_included(
    context: &ExcludedGroupPrune<'_>,
    modules: &pnpm_modules_yaml::ModulesLayout,
) -> IncludedDependencies {
    if context.eligibility.filtered_install {
        IncludedDependencies {
            dependencies: true,
            dev_dependencies: true,
            optional_dependencies: true,
        }
    } else {
        modules.included
    }
}

fn selected_prune_importer_ids(context: &ExcludedGroupPrune<'_>) -> Option<HashSet<String>> {
    context.requested_importer_ids.map(|requested| {
        context.current_lockfile.map_or_else(
            || requested.clone(),
            |current| {
                crate::materialization_closure(
                    current,
                    context.workspace_root,
                    requested,
                    crate::GroupSelection {
                        included: context.included,
                        peer_edges: context.config.peer_edge_options(),
                    },
                    &crate::SkippedSnapshots::new(),
                )
                .importer_ids
            },
        )
    })
}
