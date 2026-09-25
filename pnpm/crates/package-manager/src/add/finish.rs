use super::{AddError, AddOptions};
use crate::{InstallError, catalog_cleanup::post_install_prune, package_manifest_prefix};
use pnpm_package_manifest::PackageManifest;
use pnpm_reporter::{LogEvent, LogLevel, PackageManifestLog, PackageManifestMessage, Reporter};

pub(super) fn finish_single_add<Reporter: self::Reporter>(
    add: AddOptions<'_>,
    manifest: &mut PackageManifest,
    workspace_dir: &std::path::Path,
    install_result: Result<Option<InstallError>, InstallError>,
) -> Result<(), AddError> {
    let ignored_builds = match install_result {
        Ok(ignored_builds) => ignored_builds,
        Err(InstallError::ProjectLifecycleScript(err)) => {
            Some(InstallError::ProjectLifecycleScript(err))
        }
        Err(err) => return Err(AddError::Install(err)),
    };
    persist_manifest::<Reporter>(manifest)?;
    post_install_prune(add.config, Some(workspace_dir), manifest)
        .map_err(AddError::WriteWorkspaceManifest)?;
    if let Some(err) = ignored_builds {
        return Err(AddError::Install(err));
    }
    Ok(())
}

pub(super) fn finish_selected_add<Reporter: self::Reporter>(
    add: AddOptions<'_>,
    manifest: &PackageManifest,
    projects: &mut [pnpm_workspace::Project],
    indices: &[usize],
    workspace_dir: &std::path::Path,
    install_result: Result<Option<InstallError>, InstallError>,
) -> Result<(), AddError> {
    let ignored_builds = match install_result {
        Ok(ignored_builds) => ignored_builds,
        Err(InstallError::ProjectLifecycleScript(err)) => {
            Some(InstallError::ProjectLifecycleScript(err))
        }
        Err(err) => return Err(AddError::Install(err)),
    };
    persist_selected_manifests::<Reporter>(projects, indices)?;

    post_install_prune(add.config, Some(workspace_dir), manifest)
        .map_err(AddError::WriteWorkspaceManifest)?;
    if let Some(err) = ignored_builds {
        return Err(AddError::Install(err));
    }
    Ok(())
}

pub(super) fn persist_selected_manifests<Reporter: self::Reporter>(
    projects: &mut [pnpm_workspace::Project],
    selected_indices: &[usize],
) -> Result<(), AddError> {
    for &index in selected_indices {
        persist_manifest::<Reporter>(&mut projects[index].manifest)?;
    }
    Ok(())
}

pub(super) fn persist_manifest<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
) -> Result<(), AddError> {
    let updated = manifest.save_and_get_written_value().map_err(AddError::SaveManifest)?;
    let prefix = package_manifest_prefix(manifest);
    Reporter::emit(&LogEvent::PackageManifest(PackageManifestLog {
        level: LogLevel::Debug,
        message: PackageManifestMessage::Updated { prefix, updated },
    }));
    Ok(())
}
