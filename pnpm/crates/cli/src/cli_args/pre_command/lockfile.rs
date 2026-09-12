use super::{
    Config, Context, EnvLockfile, EnvLockfileSync, HashSet, LockfileResolution, PackageKey,
    PackageManagerToSync, PackageMetadata, Path, PathBuf, PinRoots, PmOnFail, PreCommandInput,
    PreCommandPlan, SwitchSource, Value, VersionPart, is_package_manager_resolved,
    package_manager_to_sync, pnpm_package_to_install, version_satisfies,
};
use pnpm_lockfile::SnapshotEntry;

pub(super) fn env_lockfile_sync_plan(
    input: &PreCommandInput,
    config: Config,
    env_root: PathBuf,
    package_manager: PackageManagerToSync,
) -> PreCommandPlan {
    PreCommandPlan::SyncEnvLockfile(EnvLockfileSync {
        frozen_lockfile: input.switch.frozen_lockfile.or(config.frozen_lockfile).unwrap_or(false),
        config,
        env_root,
        package_manager,
    })
}

/// pnpm's `syncEnvLockfile`: the pnpm version a project pins is recorded in
/// the env lockfile's `packageManagerDependencies` here, for every command,
/// so the entry is the same whichever command a contributor happens to run
/// first.
///
/// `None` when the project doesn't pin a persisting pnpm version, when
/// `lockfile` is turned off, or when the lockfile already records a version
/// that satisfies the pin.
pub(super) fn env_lockfile_sync(
    config: &Config,
    root_manifest: &Value,
    roots: &PinRoots,
    on_fail: PmOnFail,
    read_lockfile: ReadEnvLockfile<'_>,
) -> miette::Result<Option<PackageManagerToSync>> {
    if !config.lockfile {
        return Ok(None);
    }
    let Some(package_manager) =
        package_manager_to_sync(root_manifest, &roots.manifest, Some(on_fail))
    else {
        return Ok(None);
    };
    let read;
    let env_lockfile = match read_lockfile {
        ReadEnvLockfile::Already(env_lockfile) => Some(env_lockfile),
        ReadEnvLockfile::NotYet => {
            read = read_env_lockfile(&roots.env)?;
            read.as_ref()
        }
    };
    if env_lockfile.is_some_and(|env_lockfile| {
        is_package_manager_resolved(
            env_lockfile,
            &package_manager.specifier,
            &package_manager.version,
        )
    }) {
        return Ok(None);
    }
    Ok(Some(package_manager))
}

/// Whether the caller already holds the parsed env lockfile. The download
/// path reads it to look for a version to switch to, so reading and parsing
/// it a second time to answer the same question would repeat that work on
/// every command in a pinned project.
#[derive(Clone, Copy)]
pub(super) enum ReadEnvLockfile<'a> {
    Already(&'a EnvLockfile),
    NotYet,
}

pub(super) fn read_env_lockfile(root_dir: &Path) -> miette::Result<Option<EnvLockfile>> {
    EnvLockfile::read(root_dir).map_err(miette::Report::new).wrap_err_with(|| {
        format!("read the package-manager env lockfile in {}", root_dir.display())
    })
}

/// Switch straight to the version the env lockfile records — unless its
/// entries don't satisfy the bootstrap rules (resolutions carrying
/// tarball URLs written by an earlier pnpm, say). Those are not an
/// error: they are discarded and re-resolved afresh through the trusted
/// bootstrap registries, which yields entries in the accepted shape.
pub(super) fn locked_switch_source(
    env: EnvLockfile,
    version: String,
    roots: &PinRoots,
    frozen_lockfile: bool,
) -> SwitchSource {
    if assert_package_manager_lockfile_uses_registry_resolutions(&env).is_ok() {
        return SwitchSource::LockedEnv { env, version };
    }
    SwitchSource::Resolve {
        env_root: roots.env.clone(),
        frozen_lockfile,
        force_resync: true,
        locked_version: Some(version),
    }
}

pub(super) fn locked_package_manager_version(
    env: &EnvLockfile,
    wanted_range: &str,
) -> miette::Result<Option<String>> {
    let Some(version) = env
        .importers
        .get(EnvLockfile::ROOT_IMPORTER_KEY)
        .and_then(|importer| importer.package_manager_dependencies.as_ref())
        .and_then(|dependencies| dependencies.get("pnpm"))
        .map(|dependency| dependency.version.clone())
    else {
        return Ok(None);
    };
    if !version_satisfies(&version, wanted_range) {
        return Ok(None);
    }
    if !package_manager_dependencies_are_resolved(env, &version) {
        return Ok(None);
    }
    Ok(Some(version))
}

fn package_manager_dependencies_are_resolved(env: &EnvLockfile, version: &str) -> bool {
    let Some(dependencies) = env
        .importers
        .get(EnvLockfile::ROOT_IMPORTER_KEY)
        .and_then(|importer| importer.package_manager_dependencies.as_ref())
    else {
        return false;
    };
    if dependencies.get("pnpm").is_none_or(|dep| dep.version != version) {
        return false;
    }
    let wrapper_pkg_name = pnpm_package_to_install(version).name;
    wrapper_pkg_name == "pnpm"
        || dependencies.get(wrapper_pkg_name).is_some_and(|dep| dep.version == version)
}

fn assert_package_manager_lockfile_uses_registry_resolutions(
    env: &EnvLockfile,
) -> miette::Result<()> {
    let mut visited = HashSet::new();
    let mut pending = package_manager_root_keys(env)?;
    while let Some(key) = pending.pop() {
        if !visited.insert(key.clone()) {
            continue;
        }

        let package_key = key.without_peer();
        let package_info =
            env.packages.get(&package_key).ok_or_else(|| invalid_package_manager_lockfile(&key))?;
        let snapshot =
            env.snapshots.get(&key).ok_or_else(|| invalid_package_manager_lockfile(&key))?;

        assert_registry_package_path(&key, package_info)?;
        assert_integrity_only_resolution(&key, &package_info.resolution)?;

        append_snapshot_dependencies(snapshot, &key, &mut pending)?;
    }
    Ok(())
}

fn append_snapshot_dependencies(
    snapshot: &SnapshotEntry,
    key: &PackageKey,
    pending: &mut Vec<PackageKey>,
) -> miette::Result<()> {
    for dependencies in
        [&snapshot.dependencies, &snapshot.optional_dependencies].into_iter().flatten()
    {
        for (name, reference) in dependencies {
            let next_key =
                reference.resolve(name).ok_or_else(|| invalid_package_manager_lockfile(key))?;
            pending.push(next_key);
        }
    }
    Ok(())
}

/// The lockfile keys of the root importer's `packageManager` dependencies.
fn package_manager_root_keys(env: &EnvLockfile) -> miette::Result<Vec<PackageKey>> {
    let Some(package_manager_dependencies) = env
        .importers
        .get(EnvLockfile::ROOT_IMPORTER_KEY)
        .and_then(|importer| importer.package_manager_dependencies.as_ref())
    else {
        return Err(miette::miette!(
            "The packageManager dependencies were not found in pnpm-lock.yaml"
        ));
    };
    let mut pending = Vec::with_capacity(package_manager_dependencies.len());
    for (name, dependency) in package_manager_dependencies {
        let key = format!("{name}@{}", dependency.version)
            .parse::<PackageKey>()
            .map_err(|_| invalid_package_manager_lockfile(name))?;
        pending.push(key);
    }
    Ok(pending)
}

fn assert_registry_package_path(
    key: &PackageKey,
    package_info: &PackageMetadata,
) -> miette::Result<()> {
    if key.suffix.prefix() != pnpm_lockfile::Prefix::None
        || !matches!(key.suffix.version(), VersionPart::Semver(_))
    {
        return Err(invalid_package_manager_lockfile(key));
    }
    if let Some(version) = &package_info.version
        && version != &key.suffix.without_peer().to_string()
    {
        return Err(invalid_package_manager_lockfile(key));
    }
    Ok(())
}

fn assert_integrity_only_resolution(
    key: &PackageKey,
    resolution: &LockfileResolution,
) -> miette::Result<()> {
    match resolution {
        LockfileResolution::Registry(resolution)
            if !resolution.integrity.to_string().is_empty() =>
        {
            Ok(())
        }
        LockfileResolution::Registry(_)
        | LockfileResolution::Tarball(_)
        | LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_)
        | LockfileResolution::Variations(_)
        | LockfileResolution::Custom(_) => Err(invalid_package_manager_lockfile(key)),
    }
}

fn invalid_package_manager_lockfile(dep_path: impl std::fmt::Display) -> miette::Report {
    miette::miette!(
        r#"The packageManager dependency "{}" in pnpm-lock.yaml must use a registry package path and an integrity-only resolution"#,
        dep_path,
    )
}

/// A nonpersisted pin uses pnpm's global state, outside the project's frozen lockfile.
pub(super) fn switch_env_root(
    config: &Config,
    roots: &PinRoots,
    frozen_lockfile: bool,
    persist_lockfile: bool,
) -> miette::Result<(PathBuf, bool)> {
    let (env_root, frozen_lockfile) = if persist_lockfile {
        (roots.env.clone(), frozen_lockfile)
    } else {
        let global_pkg_dir = config.global_pkg_dir.clone().ok_or_else(|| {
            miette::miette!(
                r#"Unable to find the global packages directory. Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable."#,
            )
        })?;
        (global_pkg_dir, false)
    };
    Ok((env_root, frozen_lockfile))
}
