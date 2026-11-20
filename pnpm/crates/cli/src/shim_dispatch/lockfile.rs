use std::path::Path;

use pnpm_config::{
    Config,
    Host,
};
use pnpm_lockfile::{
    ImporterDepVersion,
    ImporterLoad,
    Lockfile,
    PkgName,
    Prefix,
};

/// The exact runtime version resolved for this project in its selected lockfile.
/// Branch lockfile selection does not read the shared lockfile when a branch
/// lockfile is missing or has no importer.
pub(super) fn locked_runtime_version(project_dir: &Path, name: &str) -> Option<String> {
    let package_name = PkgName::parse(name).ok()?;
    let config = Config::default().current::<Host>(project_dir).ok()?;
    let lockfile_dir = config.lockfile_dir_for(project_dir);
    let selection = config.wanted_lockfile_selection();
    if selection.merge_git_branch_lockfiles {
        let lockfile = Lockfile::load_wanted(lockfile_dir, &selection).ok()??;
        let importer_id = importer_id(project_dir, lockfile_dir);
        return lockfile.importers
            .get(&importer_id)
            .and_then(|importer| importer_runtime_version(importer, &package_name))
            .flatten();
    }
    let file_count = selection.branch_lockfile_candidates.len().max(1);
    for file_name in selection.read_order().take(file_count) {
        let lockfile_path = lockfile_dir.join(file_name);
        let importer_id = importer_id(project_dir, lockfile_dir);
        match load_importer_snapshot(&lockfile_path, &importer_id) {
            Ok(ImporterLoad::MissingLockfile) => continue,
            Ok(ImporterLoad::Importer(Some(importer))) => {
                return importer_runtime_version(importer.as_ref(), &package_name).flatten();
            }
            Ok(ImporterLoad::Importer(None)) | Err(_) => return None,
        }
    }
    None
}

fn load_importer_snapshot(
    lockfile_path: &Path,
    importer_id: &str,
) -> Result<ImporterLoad, pnpm_lockfile::LoadLockfileError> {
    match Lockfile::load_importer_from_path(lockfile_path, importer_id) {
        Ok(importer) => Ok(importer),
        Err(_) => Lockfile::load_from_path(lockfile_path)
            .map(|lockfile| match lockfile {
                Some(lockfile) => ImporterLoad::Importer(
                    lockfile.importers
                        .get(importer_id)
                        .cloned()
                        .map(Box::new),
                ),
                None => ImporterLoad::MissingLockfile,
            }),
    }
}

fn importer_id(project_dir: &Path, lockfile_dir: &Path) -> String {
    pnpm_workspace::importer_id_from_root_dir(lockfile_dir, project_dir)
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

#[cfg(test)]
mod tests;
