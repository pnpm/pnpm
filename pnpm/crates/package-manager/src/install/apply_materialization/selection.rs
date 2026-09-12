use super::super::{
    Config, HashSet, IncludedDependencies, InstallError, InstallWithFreshLockfileError, Lockfile,
    NodeLinker, PackageManifest, Path, PathBuf, Reporter,
};

pub(super) struct SelectMaterializedStateInputs<'a> {
    pub(super) fresh_lockfile: Option<&'a Lockfile>,
    pub(super) loaded_wanted_lockfile: Option<&'a Lockfile>,
    pub(super) requested_importer_ids: Option<&'a HashSet<String>>,
    pub(super) real_importer_ids: &'a HashSet<String>,
    pub(super) workspace_root: &'a Path,
    pub(super) included: IncludedDependencies,
    pub(super) install_skipped: &'a crate::SkippedSnapshots,
    pub(super) node_linker: NodeLinker,
    pub(super) current_lockfile: Option<&'a Lockfile>,
    pub(super) is_inconsistent: bool,
    pub(super) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
}
pub(super) struct MaterializedState<'a> {
    pub(super) wanted_lockfile: Option<&'a Lockfile>,
    pub(super) selected_current_lockfile: Option<Lockfile>,
    pub(super) current_lockfile: Option<Lockfile>,
    pub(super) project_manifests: Vec<(PathBuf, &'a PackageManifest)>,
}
pub(super) fn select_materialized_state<'a>(
    inputs: &SelectMaterializedStateInputs<'a>,
) -> MaterializedState<'a> {
    let wanted_lockfile = inputs.fresh_lockfile.or(inputs.loaded_wanted_lockfile);
    let selected_current_lockfile = wanted_lockfile.and_then(|wanted| {
        inputs.requested_importer_ids.map(|requested| {
            crate::materialization_closure(
                wanted,
                inputs.workspace_root,
                requested,
                inputs.included,
                inputs.install_skipped,
            )
            .lockfile
        })
    });
    let current_lockfile =
        wanted_lockfile.map(|wanted| materialized_current_lockfile(inputs, wanted));
    let project_anchor_importer_ids = project_anchor_importers(inputs, wanted_lockfile);
    let project_manifests = inputs
        .project_manifests
        .iter()
        .filter(|(project_dir, _)| {
            let importer_id =
                pnpm_workspace::importer_id_from_root_dir(inputs.workspace_root, project_dir);
            project_anchor_importer_ids.contains(&importer_id)
        })
        .cloned()
        .collect();

    MaterializedState {
        wanted_lockfile,
        selected_current_lockfile,
        current_lockfile,
        project_manifests,
    }
}
pub(super) fn project_anchor_importers(
    inputs: &SelectMaterializedStateInputs<'_>,
    wanted_lockfile: Option<&Lockfile>,
) -> HashSet<String> {
    match inputs.requested_importer_ids {
        Some(requested) if matches!(inputs.node_linker, NodeLinker::Hoisted) => requested.clone(),
        Some(requested) => wanted_lockfile.map_or_else(
            || requested.clone(),
            |wanted| {
                crate::materialization_closure(
                    wanted,
                    inputs.workspace_root,
                    requested,
                    inputs.included,
                    inputs.install_skipped,
                )
                .importer_ids
            },
        ),
        None => inputs.real_importer_ids.clone(),
    }
}
pub(super) fn materialized_current_lockfile(
    inputs: &SelectMaterializedStateInputs<'_>,
    wanted: &Lockfile,
) -> Lockfile {
    if inputs.requested_importer_ids.is_some() && matches!(inputs.node_linker, NodeLinker::Hoisted)
    {
        crate::filter_lockfile_for_current(wanted, inputs.included, inputs.install_skipped)
    } else if let Some(requested_importer_ids) = inputs.requested_importer_ids {
        crate::merge_filtered_current_lockfile(
            (!inputs.is_inconsistent).then_some(inputs.current_lockfile).flatten(),
            wanted,
            requested_importer_ids,
            inputs.included,
            inputs.install_skipped,
            inputs.workspace_root,
        )
    } else {
        crate::filter_lockfile_for_current(wanted, inputs.included, inputs.install_skipped)
    }
}
pub(super) struct LinkMaterializedProjectsInputs<'a> {
    pub(super) filtered_install: bool,
    pub(super) node_linker: NodeLinker,
    pub(super) config: &'static Config,
    pub(super) current_lockfile: Option<&'a Lockfile>,
    pub(super) wanted_lockfile: Option<&'a Lockfile>,
    pub(super) workspace_root: &'a Path,
    pub(super) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub(super) materialized_project_manifests: &'a [(PathBuf, &'a PackageManifest)],
}
pub(super) async fn link_materialized_projects<Reporter: self::Reporter + 'static>(
    inputs: LinkMaterializedProjectsInputs<'_>,
) -> Result<(), InstallError> {
    if inputs.filtered_install
        && !matches!(inputs.node_linker, NodeLinker::Hoisted)
        && crate::should_write_package_map(inputs.config, inputs.node_linker)
        && let Some(current) = inputs.current_lockfile.as_ref()
    {
        write_filtered_package_map(&inputs, current).await?;
    }

    // Materialize `link:` direct deps straight from the in-memory
    // project manifests. `excludeLinksFromLockfile` keeps them out
    // of the lockfile importers, so the lockfile-driven symlink
    // passes cannot see them. Aliases the wanted lockfile *does*
    // track are skipped — those belong to the lockfile passes (and
    // their dedupe decisions). See [`crate::link_manifest_link_deps`].
    // These are importer symlinks like any other, so
    // `virtualStoreOnly` skips them too.
    if !inputs.config.virtual_store_only {
        crate::link_manifest_link_deps::<Reporter>(
            inputs.workspace_root,
            inputs.materialized_project_manifests,
            inputs.wanted_lockfile.and_then(|lockfile| {
                (!lockfile.importers.is_empty()).then_some(&lockfile.importers)
            }),
            // Honor a `modulesDir` override the same way the
            // lockfile-driven symlink pass does.
            inputs
                .config
                .modules_dir
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("node_modules")),
            &crate::shim_link_options(inputs.config, inputs.node_linker),
        )
        .map_err(InstallError::LinkManifestLinkDeps)?;
    }

    Ok(())
}
pub(super) async fn write_filtered_package_map(
    inputs: &LinkMaterializedProjectsInputs<'_>,
    current: &Lockfile,
) -> Result<(), InstallError> {
    let engine_name = package_map_engine_name(inputs.config, current).await;
    let allow_build_policy = crate::AllowBuildPolicy::from_config(inputs.config)
        .map_err(InstallWithFreshLockfileError::AllowBuildsPolicy)
        .map_err(InstallError::WithFreshLockfile)?;
    let layout = crate::VirtualStoreLayout::new(
        inputs.config,
        engine_name.as_deref(),
        current.snapshots.as_ref(),
        current.packages.as_ref(),
        Some(&allow_build_policy),
        Some(inputs.workspace_root),
    );
    crate::package_map::write_package_map(
        current,
        &crate::package_map::PackageMapOptions {
            lockfile_dir: inputs.workspace_root,
            modules_dir: &inputs.config.modules_dir,
            package_map_type: inputs.config.node_package_map_type,
            layout: &layout,
            project_manifests: inputs.project_manifests,
        },
    )
    .map_err(InstallError::WritePackageMap)?;
    Ok(())
}
pub(super) async fn package_map_engine_name(
    config: &'static Config,
    current: &Lockfile,
) -> Option<String> {
    let runtime_major =
        crate::install_frozen_lockfile::find_runtime_node_major(current.snapshots.as_ref());
    let configured_major = config
        .node_version
        .as_deref()
        .and_then(crate::install_frozen_lockfile::parse_major_from_version);
    match runtime_major.or(configured_major) {
        Some(major) => Some(pnpm_graph_hasher::engine_name(major, None, None)),
        None if config.enable_global_virtual_store => tokio::task::spawn_blocking(|| {
            pnpm_graph_hasher::detect_node_major()
                .map(|major| pnpm_graph_hasher::engine_name(major, None, None))
        })
        .await
        .ok()
        .flatten(),
        None => None,
    }
}
