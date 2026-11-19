use std::path::Path;

use pnpm_lockfile::{ImporterDepVersion, Lockfile, PkgName, Prefix};

/// The exact runtime version resolved for this project in its nearest
/// ancestor lockfile. Missing, unreadable, or unsupported lockfiles leave
/// the manifest selector in force.
pub(super) fn locked_runtime_version(project_dir: &Path, name: &str) -> Option<String> {
    let package_name = PkgName::parse(name).ok()?;
    for lockfile_dir in project_dir.ancestors() {
        let lockfile_path = lockfile_dir.join(Lockfile::FILE_NAME);
        let Ok(Some(lockfile)) = Lockfile::load_from_path(&lockfile_path) else { continue };
        let importer_id = importer_id(project_dir, lockfile_dir)?;
        let Some(importer) = lockfile.importers.get(&importer_id) else { continue };
        let Some(version) = importer_runtime_version(importer, &package_name) else { continue };
        return Some(version);
    }
    None
}

fn importer_id(project_dir: &Path, lockfile_dir: &Path) -> Option<String> {
    let relative = project_dir
        .strip_prefix(lockfile_dir)
        .ok()?;
    Some(if relative.as_os_str().is_empty() {
        ".".to_string()
    } else {
        relative
            .to_string_lossy()
            .replace('\\', "/")
    })
}

fn importer_runtime_version(
    importer: &pnpm_lockfile::ProjectSnapshot,
    package_name: &PkgName,
) -> Option<String> {
    let dependency = importer
        .dev_dependencies
        .as_ref()
        .and_then(|dependencies| dependencies.get(package_name))
        .or_else(|| {
            importer
                .dependencies
                .as_ref()
                .and_then(|dependencies| dependencies.get(package_name))
        })?;
    let ImporterDepVersion::Regular(version) = &dependency.version else { return None };
    (version.prefix() == Prefix::Runtime)
        .then(|| {
            version
                .version_semver()
                .map(ToString::to_string)
        })
        .flatten()
}
