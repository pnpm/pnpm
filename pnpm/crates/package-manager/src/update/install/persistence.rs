use super::super::{
    UpdateError, UpdateSite, UpdateView,
    prepare::{SelectedUpdatePreparation, UpdatePreparation},
};
use crate::{
    InstallError,
    catalog_cleanup::{
        post_install_prune, write_workspace_catalogs, write_workspace_catalogs_selected,
    },
    emit_initial_package_manifest,
    manifest_spec_bumps::ManifestSpecBumps,
    package_manifest_prefix,
};
use pnpm_config::Config;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LogEvent, LogLevel, PackageManifestLog, PackageManifestMessage, Reporter};
use std::{collections::BTreeMap, path::Path};

pub(in super::super) fn finish_single_update<Reporter: self::Reporter>(
    update: UpdateView<'_>,
    manifest: &mut PackageManifest,
    prepared: &UpdatePreparation,
    importer_id: &str,
    bumps: Option<ManifestSpecBumps>,
    ignored_builds: Option<InstallError>,
) -> Result<(), UpdateError> {
    let applied = bumps.map(|bumps| bumps.applied.into_inner().expect("never poisoned"));
    settle_update_manifest::<Reporter>(
        manifest,
        update.config,
        SettleUpdate {
            save: update.save,
            should_persist_manifest: prepared.persist_manifest,
            importer_id,
            applied: applied.as_ref(),
            workspace_dir_for_catalogs: prepared.workspace_dir_for_catalogs.as_deref(),
        },
    )?;

    if let Some(ignored_builds) = ignored_builds {
        return Err(UpdateError::Install(ignored_builds));
    }
    Ok(())
}
/// Write back the manifests the update rewrote and the catalogs the install
/// settled on, then prune the workspace manifest.
pub(in super::super) fn settle_selected_update<Reporter: self::Reporter>(
    update: UpdateView<'_>,
    site: &UpdateSite,
    projects: &mut [pnpm_workspace::Project],
    manifest: &PackageManifest,
    prepared: SelectedUpdatePreparation,
    bumps: Option<ManifestSpecBumps>,
) -> Result<(), UpdateError> {
    let applied = bumps.map(|bumps| bumps.applied.into_inner().expect("never poisoned"));
    let persist_indices = bumped_persist_indices::<Reporter>(
        projects,
        &site.workspace_root,
        applied.as_ref(),
        prepared.persist_indices,
    );
    persist_selected_manifests::<Reporter>(projects, &persist_indices)?;
    let workspace_dir = site.catalogs_dir(prepared.workspace_dir_for_catalogs.as_deref());
    if update.save
        && let Some(applied) = applied.as_ref().filter(|applied| !applied.catalogs.is_empty())
    {
        write_workspace_catalogs_selected(
            update.config,
            workspace_dir,
            &applied.catalogs,
            projects,
        )
        .map_err(UpdateError::WriteWorkspaceManifest)?;
    }
    if update.save {
        post_install_prune(update.config, Some(workspace_dir), manifest)
            .map_err(UpdateError::WriteWorkspaceManifest)?;
    }
    Ok(())
}
/// What deciding the post-install manifest writes depends on.
#[derive(Clone, Copy)]
pub(in super::super) struct SettleUpdate<'a> {
    save: bool,
    should_persist_manifest: bool,
    importer_id: &'a str,
    applied: Option<&'a crate::AppliedSpecBumps>,
    workspace_dir_for_catalogs: Option<&'a Path>,
}
/// Write back what the install settled on: the bumped manifest ranges, the
/// catalogs the bumps moved, and the workspace-manifest prune.
pub(in super::super) fn settle_update_manifest<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
    config: &Config,
    settle: SettleUpdate<'_>,
) -> Result<(), UpdateError> {
    let bumped_manifest = settle
        .applied
        .and_then(|applied| applied.manifests.get(settle.importer_id))
        .is_some_and(|bumped| {
            apply_bumped_manifest_specs::<Reporter>(
                manifest,
                bumped,
                !settle.should_persist_manifest,
            )
        });
    if settle.should_persist_manifest || bumped_manifest {
        persist_manifest::<Reporter>(manifest)?;
    }
    if settle.save
        && let Some(applied) = settle.applied.filter(|applied| !applied.catalogs.is_empty())
    {
        write_workspace_catalogs(
            config,
            settle.workspace_dir_for_catalogs,
            &applied.catalogs,
            manifest,
        )
        .map_err(UpdateError::WriteWorkspaceManifest)?;
    }
    if settle.save {
        post_install_prune(config, settle.workspace_dir_for_catalogs, manifest)
            .map_err(UpdateError::WriteWorkspaceManifest)?;
    }
    Ok(())
}
/// Write the install's bumped ranges into every project that got one, and
/// report which projects now need persisting.
pub(in super::super) fn bumped_persist_indices<Reporter: self::Reporter>(
    projects: &mut [pnpm_workspace::Project],
    workspace_root: &Path,
    applied: Option<&crate::AppliedSpecBumps>,
    mut persist_indices: Vec<usize>,
) -> Vec<usize> {
    let Some(applied) = applied else { return persist_indices };
    for (index, project) in projects.iter_mut().enumerate() {
        let importer_id =
            pnpm_workspace::importer_id_from_root_dir(workspace_root, &project.root_dir);
        let Some(bumped) = applied.manifests.get(&importer_id) else { continue };
        let already_persisting = persist_indices.contains(&index);
        if apply_bumped_manifest_specs::<Reporter>(
            &mut project.manifest,
            bumped,
            !already_persisting,
        ) && !already_persisting
        {
            persist_indices.push(index);
        }
    }
    persist_indices
}
/// Write the ranges the install settled on into `manifest`, reporting
/// whether anything changed. The alias keeps the group it is declared under:
/// an update moves a range, it never moves a dependency between groups.
///
/// `announce_initial` emits the manifest's pre-rewrite shape, which the
/// reporter pairs with the one [`persist_manifest`] emits. Manifest
/// preparation already announced a manifest it rewrote before resolving, so
/// only a manifest this is the first to touch needs it.
pub(in super::super) fn apply_bumped_manifest_specs<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
    bumped: &BTreeMap<String, (DependencyGroup, String)>,
    announce_initial: bool,
) -> bool {
    let declared = bumped
        .iter()
        .filter(|(alias, (group, _))| {
            manifest.dependencies([*group]).any(|(name, _)| name == alias.as_str())
        })
        .collect::<Vec<_>>();
    if declared.is_empty() {
        return false;
    }
    if announce_initial {
        emit_initial_package_manifest::<Reporter>(manifest);
    }
    for (alias, (group, specifier)) in declared {
        // Written in place rather than through `add_dependency`, which
        // moves the alias into the target group by deleting it from the
        // others. An update moves a range, never a dependency.
        manifest.value_mut()[<&str>::from(*group)][alias] =
            serde_json::Value::String(specifier.clone());
    }
    true
}
pub(in super::super) fn persist_selected_manifests<Reporter: self::Reporter>(
    projects: &mut [pnpm_workspace::Project],
    selected_indices: &[usize],
) -> Result<(), UpdateError> {
    for &index in selected_indices {
        persist_manifest::<Reporter>(&mut projects[index].manifest)?;
    }
    Ok(())
}
pub(in super::super) fn persist_manifest<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
) -> Result<(), UpdateError> {
    let updated = manifest.save_and_get_written_value().map_err(UpdateError::SaveManifest)?;
    let prefix = package_manifest_prefix(manifest);
    Reporter::emit(&LogEvent::PackageManifest(PackageManifestLog {
        level: LogLevel::Debug,
        message: PackageManifestMessage::Updated { prefix, updated },
    }));
    Ok(())
}
