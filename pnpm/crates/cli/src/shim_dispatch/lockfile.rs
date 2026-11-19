use std::path::Path;

use pnpm_config::{Config, Host};
use pnpm_lockfile::{ImporterDepVersion, Lockfile, PkgName, Prefix};

enum LockfileRuntimeVersion {
    NoResolution,
    NotRuntime,
    Runtime(String),
}

/// The exact runtime version resolved for this project in the wanted lockfile.
/// Missing, unreadable, or unsupported lockfiles leave the manifest selector
/// in force.
pub(super) fn locked_runtime_version(project_dir: &Path, name: &str) -> Option<String> {
    let package_name = PkgName::parse(name).ok()?;
    let config = Config::default().current::<Host>(project_dir).ok()?;
    let lockfile_dir = config.lockfile_dir_for(project_dir);
    let selection = config.wanted_lockfile_selection();
    if selection.merge_git_branch_lockfiles {
        let lockfile = Lockfile::load_wanted(lockfile_dir, &selection).ok()??;
        let importer_id = importer_id(project_dir, lockfile_dir)?;
        return lockfile
            .importers
            .get(&importer_id)
            .and_then(|importer| importer_runtime_version(importer, &package_name))
            .flatten();
    }
    for file_name in selection.read_order() {
        let lockfile_path = lockfile_dir.join(file_name);
        if !lockfile_path.exists() {
            continue;
        }
        return match runtime_version_in_lockfile(
            project_dir,
            lockfile_dir,
            &lockfile_path,
            &package_name,
        ) {
            LockfileRuntimeVersion::Runtime(version) => Some(version),
            LockfileRuntimeVersion::NotRuntime | LockfileRuntimeVersion::NoResolution => None,
        };
    }
    None
}

fn runtime_version_in_lockfile(
    project_dir: &Path,
    lockfile_dir: &Path,
    lockfile_path: &Path,
    package_name: &PkgName,
) -> LockfileRuntimeVersion {
    let Some(importer_id) = importer_id(project_dir, lockfile_dir) else {
        return LockfileRuntimeVersion::NoResolution;
    };
    let Some(importer) = load_importer_snapshot(lockfile_path, &importer_id) else {
        return LockfileRuntimeVersion::NoResolution;
    };
    match importer_runtime_version(&importer, package_name) {
        Some(Some(version)) => LockfileRuntimeVersion::Runtime(version),
        Some(None) => LockfileRuntimeVersion::NotRuntime,
        None => LockfileRuntimeVersion::NotRuntime,
    }
}

fn load_importer_snapshot(
    lockfile_path: &Path,
    importer_id: &str,
) -> Option<pnpm_lockfile::ProjectSnapshot> {
    match Lockfile::load_importer_from_path(lockfile_path, importer_id) {
        Ok(importer) => importer,
        Err(_) => Lockfile::load_from_path(lockfile_path)
            .ok()
            .flatten()?
            .importers
            .get(importer_id)
            .cloned(),
    }
}

fn importer_id(project_dir: &Path, lockfile_dir: &Path) -> Option<String> {
    let relative = project_dir.strip_prefix(lockfile_dir).ok()?;
    Some(if relative.as_os_str().is_empty() {
        ".".to_string()
    } else {
        relative.to_string_lossy().replace('\\', "/")
    })
}

fn importer_runtime_version(
    importer: &pnpm_lockfile::ProjectSnapshot,
    package_name: &PkgName,
) -> Option<Option<String>> {
    let dependency = importer.dev_dependencies
        .as_ref()
        .and_then(|dependencies| dependencies.get(package_name))
        .or_else(|| {
            importer.dependencies
                .as_ref()
                .and_then(|dependencies| dependencies.get(package_name))
        })?;
    let ImporterDepVersion::Regular(version) = &dependency.version else { return Some(None) };
    if version.prefix() != Prefix::Runtime {
        return Some(None);
    }
    Some(version.version_semver().map(ToString::to_string))
}
