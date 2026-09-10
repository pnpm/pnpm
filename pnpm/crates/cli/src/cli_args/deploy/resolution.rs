use super::{
    Context, ConvertCtx, DeployError, DirectoryResolution, HashMap, ImporterDepVersion,
    IntoDiagnostic, LockfileResolution, PackageKey, PackageMetadata, Path, PathBuf, PkgName,
    PkgNameVerPeer, ProjectInfo, ProjectPathKey, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry, TarballResolution, VersionPart,
    is_child_path, lexical_normalize, relative_path, same_path,
};

pub(super) struct ResolveBases<'a> {
    pub(super) file_base: &'a Path,
    pub(super) link_base: &'a Path,
}

struct LocalResolve {
    resolved_path: PathBuf,
    suffix: String,
}

pub(super) fn convert_package_metadata(
    metadata: &PackageMetadata,
    ctx: &ConvertCtx,
) -> miette::Result<PackageMetadata> {
    let mut metadata = metadata.clone();
    metadata.resolution = match &metadata.resolution {
        LockfileResolution::Directory(resolution) => {
            let resolved = validate_lockfile_local_path(
                &ctx.lockfile_dir.join(&resolution.directory),
                ctx.lockfile_dir,
            )?;
            LockfileResolution::Directory(DirectoryResolution {
                directory: relative_path(ctx.deploy_dir, &resolved),
            })
        }
        LockfileResolution::Tarball(resolution) if resolution.tarball.starts_with("file:") => {
            let input_path = resolution.tarball.trim_start_matches("file:");
            let resolved =
                validate_lockfile_local_path(&ctx.lockfile_dir.join(input_path), ctx.lockfile_dir)?;
            LockfileResolution::Tarball(TarballResolution {
                tarball: format!("file:{}", relative_path(ctx.deploy_dir, &resolved)),
                integrity: resolution.integrity.clone(),
                revision: None,
                git_hosted: resolution.git_hosted,
                path: resolution.path.as_ref().map(|_| relative_path(ctx.deploy_dir, &resolved)),
            })
        }
        _ => metadata.resolution.clone(),
    };
    metadata.peer_dependencies = metadata.peer_dependencies.clone();
    Ok(metadata)
}

pub(super) fn convert_snapshot(
    snapshot: &SnapshotEntry,
    ctx: &ConvertCtx,
    link_base: &Path,
) -> miette::Result<SnapshotEntry> {
    let bases = ResolveBases { file_base: ctx.lockfile_dir, link_base };
    Ok(SnapshotEntry {
        dependencies: convert_snapshot_dep_map(snapshot.dependencies.as_ref(), ctx, &bases)?,
        optional_dependencies: convert_snapshot_dep_map(
            snapshot.optional_dependencies.as_ref(),
            ctx,
            &bases,
        )?,
        ..snapshot.clone()
    })
}

pub(super) fn project_snapshot_to_snapshot_entry(
    snapshot: &ProjectSnapshot,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<SnapshotEntry> {
    Ok(SnapshotEntry {
        dependencies: convert_importer_dep_map_to_snapshot_deps(
            snapshot.dependencies.as_ref(),
            ctx,
            bases,
        )?,
        optional_dependencies: convert_importer_dep_map_to_snapshot_deps(
            snapshot.optional_dependencies.as_ref(),
            ctx,
            bases,
        )?,
        ..Default::default()
    })
}

pub(super) fn convert_resolved_dependency_spec(
    name: &PkgName,
    spec: &ResolvedDependencySpec,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<ResolvedDependencySpec> {
    let mut spec = spec.clone();
    spec.version = convert_importer_dep_version(name, &spec.version, ctx, bases)?;
    spec.specifier = spec.version.to_string();
    Ok(spec)
}

fn convert_importer_dep_map_to_snapshot_deps(
    input: Option<&ResolvedDependencyMap>,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<Option<HashMap<PkgName, SnapshotDepRef>>> {
    let Some(input) = input else { return Ok(None) };
    let mut output = HashMap::new();
    for (name, spec) in input {
        output.insert(
            name.clone(),
            convert_importer_version_to_snapshot_ref(name, &spec.version, ctx, bases)?,
        );
    }
    Ok((!output.is_empty()).then_some(output))
}

fn convert_snapshot_dep_map(
    input: Option<&HashMap<PkgName, SnapshotDepRef>>,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<Option<HashMap<PkgName, SnapshotDepRef>>> {
    let Some(input) = input else { return Ok(None) };
    let mut output = HashMap::new();
    for (name, dep_ref) in input {
        output.insert(name.clone(), convert_snapshot_dep_ref(name, dep_ref, ctx, bases)?);
    }
    Ok((!output.is_empty()).then_some(output))
}

fn convert_importer_dep_version(
    alias: &PkgName,
    version: &ImporterDepVersion,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<ImporterDepVersion> {
    if let Some(local) = resolve_importer_dep_version(version, bases) {
        return local_to_importer_dep_version(alias, &local, ctx);
    }
    Ok(version.clone())
}

fn convert_importer_version_to_snapshot_ref(
    alias: &PkgName,
    version: &ImporterDepVersion,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<SnapshotDepRef> {
    if let Some(local) = resolve_importer_dep_version(version, bases) {
        return local_to_snapshot_dep_ref(alias, &local, ctx);
    }
    Ok(match version {
        ImporterDepVersion::Regular(version) => SnapshotDepRef::Plain(version.clone()),
        ImporterDepVersion::Alias(alias) => SnapshotDepRef::Alias(alias.clone()),
        ImporterDepVersion::Link(target) => SnapshotDepRef::Link(target.clone()),
        ImporterDepVersion::File(payload) => {
            let local = resolve_file_payload(bases.file_base, payload).with_alias(alias);
            local_to_snapshot_dep_ref(alias, &local, ctx)?
        }
    })
}

fn convert_snapshot_dep_ref(
    alias: &PkgName,
    dep_ref: &SnapshotDepRef,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<SnapshotDepRef> {
    if let Some(local) = resolve_snapshot_dep_ref(alias, dep_ref, bases) {
        return local_to_snapshot_dep_ref(alias, &local, ctx);
    }
    Ok(dep_ref.clone())
}

fn resolve_importer_dep_version(
    version: &ImporterDepVersion,
    bases: &ResolveBases,
) -> Option<LocalResolve> {
    match version {
        ImporterDepVersion::Regular(version) => resolve_pkg_ver_peer(version, bases.file_base),
        ImporterDepVersion::Alias(key) => resolve_pkg_ver_peer(&key.suffix, bases.file_base)
            .map(|local| local.with_alias(&key.name)),
        ImporterDepVersion::Link(target) => Some(resolve_link_payload(bases.link_base, target)),
        ImporterDepVersion::File(payload) => Some(resolve_file_payload(bases.file_base, payload)),
    }
}

fn resolve_snapshot_dep_ref(
    alias: &PkgName,
    dep_ref: &SnapshotDepRef,
    bases: &ResolveBases,
) -> Option<LocalResolve> {
    match dep_ref {
        SnapshotDepRef::Plain(version) => {
            resolve_pkg_ver_peer(version, bases.file_base).map(|local| local.with_alias(alias))
        }
        SnapshotDepRef::Alias(key) => resolve_pkg_ver_peer(&key.suffix, bases.file_base)
            .map(|local| local.with_alias(&key.name)),
        SnapshotDepRef::Link(target) => Some(resolve_link_payload(bases.link_base, target)),
    }
}

fn resolve_pkg_ver_peer(version: &pnpm_lockfile::PkgVerPeer, base: &Path) -> Option<LocalResolve> {
    let VersionPart::File(path) = version.version() else { return None };
    Some(LocalResolve {
        resolved_path: lexical_normalize(&base.join(path)),
        suffix: version.peer().to_string(),
    })
}

fn resolve_file_payload(base: &Path, payload: &str) -> LocalResolve {
    let (path, suffix) = split_local_payload(payload);
    LocalResolve { resolved_path: lexical_normalize(&base.join(path)), suffix: suffix.to_string() }
}

fn resolve_link_payload(base: &Path, payload: &str) -> LocalResolve {
    let (path, suffix) = split_local_payload(payload);
    LocalResolve { resolved_path: lexical_normalize(&base.join(path)), suffix: suffix.to_string() }
}

pub(super) fn split_local_payload(payload: &str) -> (&str, &str) {
    let suffix = pnpm_deps_path::index_of_dep_path_suffix(payload);
    match suffix.patch_hash_index.or(suffix.peers_index) {
        Some(index) => (&payload[..index], &payload[index..]),
        None => (payload, ""),
    }
}

impl LocalResolve {
    fn with_alias(self, _alias: &PkgName) -> Self {
        self
    }
}

fn local_to_importer_dep_version(
    alias: &PkgName,
    local: &LocalResolve,
    ctx: &ConvertCtx,
) -> miette::Result<ImporterDepVersion> {
    let resolved_path = validate_lockfile_local_path(&local.resolved_path, ctx.lockfile_dir)?;
    if same_path(&resolved_path, ctx.deployed_project_root) {
        return Ok(ImporterDepVersion::Link(".".to_string()));
    }
    let key =
        create_file_url_key(&resolved_path, &local.suffix, ctx.projects_by_path, Some(alias))?;
    Ok(ImporterDepVersion::Alias(key))
}

fn local_to_snapshot_dep_ref(
    alias: &PkgName,
    local: &LocalResolve,
    ctx: &ConvertCtx,
) -> miette::Result<SnapshotDepRef> {
    let resolved_path = validate_lockfile_local_path(&local.resolved_path, ctx.lockfile_dir)?;
    if same_path(&resolved_path, ctx.deployed_project_root) {
        return Ok(SnapshotDepRef::Link(".".to_string()));
    }
    Ok(SnapshotDepRef::Alias(create_file_url_key(
        &resolved_path,
        &local.suffix,
        ctx.projects_by_path,
        Some(alias),
    )?))
}

pub(super) fn convert_package_key(
    key: &PackageKey,
    ctx: &ConvertCtx,
) -> miette::Result<PackageKey> {
    let VersionPart::File(path) = key.suffix.version() else { return Ok(key.clone()) };
    let resolved = validate_lockfile_local_path(&ctx.lockfile_dir.join(path), ctx.lockfile_dir)?;
    create_file_url_key(&resolved, key.suffix.peer(), ctx.projects_by_path, Some(&key.name))
}

pub(super) fn validate_lockfile_local_path(
    path: &Path,
    lockfile_dir: &Path,
) -> miette::Result<PathBuf> {
    let normalized = lexical_normalize(path);
    let workspace_dir = lexical_normalize(lockfile_dir);
    if same_path(&normalized, &workspace_dir) || is_child_path(&normalized, &workspace_dir) {
        return Ok(normalized);
    }
    Err(DeployError::UnsafeLockfilePath { path: normalized, workspace_dir }.into())
}

pub(super) fn create_file_url_key(
    resolved_path: &Path,
    suffix: &str,
    projects_by_path: &HashMap<ProjectPathKey, ProjectInfo>,
    package_name: Option<&PkgName>,
) -> miette::Result<PkgNameVerPeer> {
    let normalized = lexical_normalize(resolved_path);
    let normalized_display = normalized.display();
    let dep_file_url = url::Url::from_file_path(&normalized)
        .map_err(|()| miette::miette!("could not convert {} to a file URL", normalized_display))?
        .to_string();
    let name = projects_by_path
        .get(&ProjectPathKey::new(&normalized))
        .and_then(|project| project.name.as_deref())
        .map(str::to_string)
        .or_else(|| package_name.map(PkgName::to_string))
        .or_else(|| normalized.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_else(|| normalized.display().to_string());
    format!("{name}@{dep_file_url}{suffix}")
        .parse()
        .into_diagnostic()
        .wrap_err("create deploy file URL dependency path")
}
