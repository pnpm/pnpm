use super::{
    Config, Context, DependencyGroup, DeployError, DeployWorkspaceConfig, DirectoryResolution,
    HashMap, HashSet, IntoDiagnostic, Lockfile, LockfileResolution, Map, PackageKey,
    PackageManifest, PackageMetadata, Path, PathBuf, PkgName, PkgNameVerPeer, Project, ProjectInfo,
    ProjectPathKey, ProjectSnapshot, ResolveBases, ResolvedDependencyMap, ResolvedDependencySpec,
    SelectedProject, SnapshotEntry, State, Value, bind_singleton_peers, convert_package_key,
    convert_package_metadata, convert_resolved_dependency_spec, convert_snapshot,
    create_file_url_key, is_ancestor_path, lexical_normalize, omit_peers_of_excluded_dependencies,
    project_snapshot_to_snapshot_entry, prune_deploy_lockfile_graph, relative_path, same_path,
    validate_lockfile_local_path,
};

pub(super) struct DeployFiles {
    pub(super) manifest: Value,
    pub(super) lockfile: Lockfile,
    pub(super) workspace_manifest: Option<Value>,
    pub(super) workspace_config: DeployWorkspaceConfig,
}

pub(super) struct ConvertCtx<'a> {
    pub(super) projects_by_path: &'a HashMap<ProjectPathKey, ProjectInfo>,
    pub(super) deploy_dir: &'a Path,
    pub(super) lockfile_dir: &'a Path,
    pub(super) deployed_project_root: &'a Path,
}

pub(super) fn manifest_dependency_names(
    manifest: &PackageManifest,
    groups: &[&str],
) -> Vec<PkgName> {
    groups
        .iter()
        .filter_map(|group| manifest.value().get(group))
        .filter_map(Value::as_object)
        .flat_map(|dependencies| dependencies.keys())
        .filter_map(|name| name.parse().ok())
        .collect()
}

pub(super) fn create_deploy_files(
    lockfile: &Lockfile,
    selected: &SelectedProject,
    project_id: &str,
    lockfile_dir: &Path,
    deploy_dir: &Path,
    config: &Config,
    dependency_groups: &[DependencyGroup],
) -> miette::Result<DeployFiles> {
    let input_snapshot = lockfile
        .importers
        .get(project_id)
        .ok_or_else(|| DeployError::MissingImporter { project_id: project_id.to_string() })?;
    let deployed_project_root =
        validate_lockfile_local_path(&lockfile_dir.join(project_id), lockfile_dir)?;
    let ctx = ConvertCtx {
        projects_by_path: &selected.projects_by_path,
        deploy_dir,
        lockfile_dir,
        deployed_project_root: &deployed_project_root,
    };
    let (declared_dependencies, peer_only_dependencies) =
        dependency_name_sets(&selected.project.manifest);
    let target_snapshot = deploy_snapshot(&DeployedDependencies {
        input_snapshot,
        dependency_groups,
        peer_only_dependencies: &peer_only_dependencies,
        selected,
        ctx: &ctx,
    })?;

    let packages =
        convert_deploy_packages(lockfile, project_id, lockfile_dir, deploy_dir, selected, &ctx)?;
    let converted = convert_deploy_snapshots(lockfile, project_id, lockfile_dir, selected, &ctx)?;
    let deploy_lockfile = converted_deploy_lockfile(
        lockfile,
        &target_snapshot,
        packages,
        converted,
        dependency_groups,
    )?;

    let manifest =
        deploy_manifest(&selected.project.manifest, &target_snapshot, &declared_dependencies);

    finish_deploy_files(lockfile, config, &ctx, manifest, deploy_lockfile)
}

/// The names the project declares as dependencies, and the peers it does
/// not also depend on itself.
fn dependency_name_sets(manifest: &PackageManifest) -> (HashSet<String>, HashSet<String>) {
    let declared_dependencies =
        manifest.available_dependency_names(None).into_iter().collect::<HashSet<_>>();
    let peer_only_dependencies = manifest
        .dependencies([DependencyGroup::Peer])
        .map(|(name, _)| name.to_string())
        .filter(|name| !declared_dependencies.contains(name))
        .collect::<HashSet<_>>();
    (declared_dependencies, peer_only_dependencies)
}

struct DeployedDependencies<'a> {
    input_snapshot: &'a ProjectSnapshot,
    dependency_groups: &'a [DependencyGroup],
    peer_only_dependencies: &'a HashSet<String>,
    selected: &'a SelectedProject,
    ctx: &'a ConvertCtx<'a>,
}

/// Fill the deployed importer's dependency maps. An excluded group's direct
/// dependencies are left out of both the deployed manifest and the deployed
/// importer, because the graph prune drops the packages they would point at;
/// a peer the project only declares stays in whichever group carries it.
fn fill_target_dependencies(
    target_snapshot: &mut ProjectSnapshot,
    deployed: &DeployedDependencies<'_>,
) -> miette::Result<()> {
    let selected_root = lexical_normalize(&deployed.selected.project.root_dir);
    let selected_bases =
        ResolveBases { file_base: deployed.ctx.lockfile_dir, link_base: &selected_root };
    for (group, target, source) in [
        (
            DependencyGroup::Prod,
            &mut target_snapshot.dependencies,
            &deployed.input_snapshot.dependencies,
        ),
        (
            DependencyGroup::Dev,
            &mut target_snapshot.dev_dependencies,
            &deployed.input_snapshot.dev_dependencies,
        ),
        (
            DependencyGroup::Optional,
            &mut target_snapshot.optional_dependencies,
            &deployed.input_snapshot.optional_dependencies,
        ),
    ] {
        let included = deployed.dependency_groups.contains(&group);
        fill_target_dependency_map(
            target,
            source.iter().flatten().filter(|(name, _)| {
                included || deployed.peer_only_dependencies.contains(&name.to_string())
            }),
            deployed.ctx,
            &selected_bases,
        )?;
    }
    drop_empty_dependency_map(&mut target_snapshot.dependencies);
    drop_empty_dependency_map(&mut target_snapshot.dev_dependencies);
    drop_empty_dependency_map(&mut target_snapshot.optional_dependencies);
    Ok(())
}

/// The deployed lockfile: the source lockfile with the deployed project as
/// its only importer and the converted graph, without the workspace-level
/// configuration the target does not carry. The deployed manifest holds
/// concrete dependency versions, so catalog snapshots would refer to
/// configuration that is not copied to the target.
fn converted_deploy_lockfile(
    lockfile: &Lockfile,
    target_snapshot: &ProjectSnapshot,
    packages: HashMap<PackageKey, PackageMetadata>,
    converted: DeploySnapshots,
    dependency_groups: &[DependencyGroup],
) -> miette::Result<Lockfile> {
    let mut deploy_lockfile = lockfile.clone();
    deploy_lockfile.catalogs = None;
    deploy_lockfile.patched_dependencies = None;
    deploy_lockfile.overrides = None;
    deploy_lockfile.package_extensions_checksum = None;
    deploy_lockfile.pnpmfile_checksum = None;
    if let Some(settings) = deploy_lockfile.settings.as_mut() {
        settings.inject_workspace_packages = false;
    }
    deploy_lockfile.importers =
        HashMap::from([(Lockfile::ROOT_IMPORTER_KEY.to_string(), target_snapshot.clone())]);
    deploy_lockfile.packages = (!packages.is_empty()).then_some(packages);
    deploy_lockfile.snapshots = (!converted.snapshots.is_empty()).then_some(converted.snapshots);
    prune_deploy_lockfile_graph(&mut deploy_lockfile, dependency_groups);
    bind_singleton_peers(&mut deploy_lockfile, &converted.linked_workspace_projects)?;
    Ok(deploy_lockfile)
}

/// The deployed lockfile's `packages` map: every source package with its
/// paths rewritten, plus a directory entry for each other workspace
/// importer the deploy links.
fn convert_deploy_packages(
    lockfile: &Lockfile,
    project_id: &str,
    lockfile_dir: &Path,
    deploy_dir: &Path,
    selected: &SelectedProject,
    ctx: &ConvertCtx<'_>,
) -> miette::Result<HashMap<PackageKey, PackageMetadata>> {
    let mut packages = HashMap::new();
    for (key, metadata) in lockfile.packages.iter().flatten() {
        let output_key = convert_package_key(key, ctx)?;
        packages.insert(output_key, convert_package_metadata(metadata, ctx)?);
    }
    for importer_path in lockfile.importers.keys() {
        if importer_path == project_id {
            continue;
        }
        let project_root =
            validate_lockfile_local_path(&lockfile_dir.join(importer_path), lockfile_dir)?;
        let package_key = create_file_url_key(&project_root, "", &selected.projects_by_path, None)?;
        packages.insert(
            package_key,
            PackageMetadata {
                resolution: LockfileResolution::Directory(DirectoryResolution {
                    directory: relative_path(deploy_dir, &project_root),
                }),
                version: None,
                engines: None,
                cpu: None,
                os: None,
                libc: None,
                deprecated: None,
                has_bin: None,
                prepare: None,
                bundled_dependencies: None,
                peer_dependencies: None,
                peer_dependencies_meta: None,
            },
        );
    }
    Ok(packages)
}

/// The deployed lockfile's `snapshots` map, and the linked workspace
/// packages whose peers [`bind_singleton_peers`] still has to resolve.
struct DeploySnapshots {
    snapshots: HashMap<PkgNameVerPeer, SnapshotEntry>,
    linked_workspace_projects: HashMap<PkgNameVerPeer, ProjectInfo>,
}

fn convert_deploy_snapshots(
    lockfile: &Lockfile,
    project_id: &str,
    lockfile_dir: &Path,
    selected: &SelectedProject,
    ctx: &ConvertCtx<'_>,
) -> miette::Result<DeploySnapshots> {
    let mut snapshots = HashMap::new();
    for (key, snapshot) in lockfile.snapshots.iter().flatten() {
        let output_key = convert_package_key(key, ctx)?;
        snapshots.insert(output_key, convert_snapshot(snapshot, ctx, lockfile_dir)?);
    }
    let mut linked_workspace_projects = HashMap::new();
    for (importer_path, project_snapshot) in &lockfile.importers {
        if importer_path == project_id {
            continue;
        }
        let project_root =
            validate_lockfile_local_path(&lockfile_dir.join(importer_path), lockfile_dir)?;
        let bases = ResolveBases { file_base: lockfile_dir, link_base: &project_root };
        let package_key = create_file_url_key(&project_root, "", &selected.projects_by_path, None)?;
        if let Some(project) = selected.projects_by_path.get(&ProjectPathKey::new(&project_root))
            && !project.peer_dependencies.is_empty()
        {
            linked_workspace_projects.insert(package_key.clone(), project.clone());
        }
        snapshots.insert(
            package_key,
            project_snapshot_to_snapshot_entry(project_snapshot, ctx, &bases)?,
        );
    }
    Ok(DeploySnapshots { snapshots, linked_workspace_projects })
}

/// The `pnpm-workspace.yaml` the deploy writes, and the same settings in
/// the shape the deploy install consumes. Only the settings that survive
/// a deploy are carried: patch files, rewritten to paths relative to the
/// deploy dir, and the build allow-list.
fn deploy_workspace_settings(
    lockfile: &Lockfile,
    config: &Config,
    lockfile_dir: &Path,
    deploy_dir: &Path,
    deploy_lockfile: &mut Lockfile,
) -> miette::Result<(Map<String, Value>, DeployWorkspaceConfig)> {
    let mut workspace_manifest = Map::new();
    let mut workspace_config =
        DeployWorkspaceConfig { patched_dependencies: None, allow_builds: HashMap::new() };
    if lockfile.patched_dependencies.is_some()
        && let Some(patched_dependencies) = config.patched_dependencies.as_ref()
    {
        deploy_lockfile.patched_dependencies.clone_from(&lockfile.patched_dependencies);
        let rewritten = patched_dependencies
            .iter()
            .map(|(name, value)| {
                let absolute = if Path::new(value).is_absolute() {
                    PathBuf::from(value)
                } else {
                    lockfile_dir.join(value)
                };
                (name.clone(), relative_path(deploy_dir, &absolute))
            })
            .collect::<indexmap::IndexMap<_, _>>();
        workspace_manifest.insert(
            "patchedDependencies".to_string(),
            serde_json::to_value(&rewritten).into_diagnostic()?,
        );
        workspace_config.patched_dependencies = Some(rewritten);
    }
    if !config.allow_builds.is_empty() {
        workspace_manifest.insert(
            "allowBuilds".to_string(),
            serde_json::to_value(&config.allow_builds).into_diagnostic()?,
        );
        workspace_config.allow_builds.clone_from(&config.allow_builds);
    }
    Ok((workspace_manifest, workspace_config))
}

/// A lockfile importer records a dependency group only when it has entries.
fn drop_empty_dependency_map(dependencies: &mut Option<ResolvedDependencyMap>) {
    if dependencies.as_ref().is_some_and(HashMap::is_empty) {
        *dependencies = None;
    }
}

fn fill_target_dependency_map<'a>(
    output: &mut Option<ResolvedDependencyMap>,
    input: impl Iterator<Item = (&'a PkgName, &'a ResolvedDependencySpec)>,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<()> {
    let output = output.get_or_insert_with(HashMap::new);
    for (name, spec) in input {
        output.insert(name.clone(), convert_resolved_dependency_spec(name, spec, ctx, bases)?);
    }
    Ok(())
}

fn set_manifest_dependencies(
    manifest: &mut Value,
    field: &str,
    dependencies: Option<&ResolvedDependencyMap>,
) {
    let deps = dependencies
        .into_iter()
        .flatten()
        .map(|(name, spec)| (name.to_string(), Value::String(spec.version.to_string())))
        .collect::<Map<_, _>>();
    if let Some(object) = manifest.as_object_mut() {
        object.insert(field.to_string(), Value::Object(deps));
    }
}

/// Importer paths must remain inside the shared lockfile directory.
pub(super) fn load_deploy_lockfile(
    workspace_dir: &Path,
    lockfile_dir: &Path,
) -> miette::Result<Result<Lockfile, String>> {
    if !same_path(workspace_dir, lockfile_dir) && !is_ancestor_path(lockfile_dir, workspace_dir) {
        return Ok(Err(format!(
            "The lockfile at {} does not contain the workspace, so its importer paths cannot be deployed. Falling back to installing without it.",
            lockfile_dir.display(),
        )));
    }
    let Some(lockfile) = Lockfile::load_wanted_from_dir(lockfile_dir)
        .map_err(miette::Report::new)
        .wrap_err("read shared lockfile")?
    else {
        return Ok(Err(
            "Shared lockfile not found. Falling back to installing without a lockfile.".to_string(),
        ));
    };

    Ok(Ok(lockfile))
}

fn deploy_manifest(
    source: &PackageManifest,
    target_snapshot: &ProjectSnapshot,
    declared_dependencies: &HashSet<String>,
) -> Value {
    let mut manifest = source.value().clone();
    set_manifest_dependencies(&mut manifest, "dependencies", target_snapshot.dependencies.as_ref());
    set_manifest_dependencies(
        &mut manifest,
        "devDependencies",
        target_snapshot.dev_dependencies.as_ref(),
    );
    set_manifest_dependencies(
        &mut manifest,
        "optionalDependencies",
        target_snapshot.optional_dependencies.as_ref(),
    );
    omit_peers_of_excluded_dependencies(&mut manifest, declared_dependencies, target_snapshot);

    manifest
}

fn deploy_snapshot(deployed: &DeployedDependencies<'_>) -> miette::Result<ProjectSnapshot> {
    let mut snapshot = ProjectSnapshot {
        specifiers: Some(HashMap::new()),
        dependencies: Some(HashMap::new()),
        dev_dependencies: Some(HashMap::new()),
        optional_dependencies: Some(HashMap::new()),
        ..deployed.input_snapshot.clone()
    };
    fill_target_dependencies(&mut snapshot, deployed)?;
    Ok(snapshot)
}

/// The deployed project is the entire workspace, even when the source root contained siblings.
pub(super) fn deployed_workspace_projects(
    state: &State,
    deploy_dir: &Path,
    legacy: bool,
) -> Option<Vec<Project>> {
    (!legacy).then(|| {
        vec![Project {
            root_dir: deploy_dir.to_path_buf(),
            manifest: state.manifest.clone(),
            dependency_manifest: None,
        }]
    })
}

fn finish_deploy_files(
    lockfile: &Lockfile,
    config: &Config,
    ctx: &ConvertCtx<'_>,
    manifest: Value,
    mut deploy_lockfile: Lockfile,
) -> miette::Result<DeployFiles> {
    let (workspace_manifest, workspace_config) = deploy_workspace_settings(
        lockfile,
        config,
        ctx.lockfile_dir,
        ctx.deploy_dir,
        &mut deploy_lockfile,
    )?;

    Ok(DeployFiles {
        manifest,
        lockfile: deploy_lockfile,
        workspace_manifest: (!workspace_manifest.is_empty())
            .then_some(Value::Object(workspace_manifest)),
        workspace_config,
    })
}
