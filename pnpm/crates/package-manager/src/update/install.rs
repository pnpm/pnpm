pub(super) mod persistence;
pub(super) use persistence::{finish_single_update, settle_selected_update};

use super::{
    SelectedProjects, UpdateError, UpdateOptions, UpdateResources, UpdateSite, manifest_dir,
    prepare::{
        ReadPackageHook, SelectedUpdatePreparation, UpdatePreparation,
        apply_read_package_hook_to_update_manifest,
    },
    update_mutation,
};
use crate::{
    Install, PolicyExcludes, UpdateSeedPolicy, WorkspaceInstallSelection,
    catalog_cleanup::{write_workspace_catalogs, write_workspace_catalogs_selected},
    defer_post_install_errors,
    manifest_spec_bumps::ManifestSpecBumps,
};
use pipe_trait::Pipe;
use pnpm_catalogs_types::Catalogs;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::Reporter;
use pnpm_resolving_resolver_base::PreferredVersions;
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub(super) async fn run_prepared_selected_update<Reporter: self::Reporter + 'static>(
    update: UpdateOptions<'_>,
    owned: UpdateResources,
    manifest: &PackageManifest,
    selected: SelectedProjects<'_>,
    site: UpdateSite,
    unsaved: UnsavedManifests,
    mut prepared: SelectedUpdatePreparation,
) -> Result<(), UpdateError> {
    if update.version.save {
        write_workspace_catalogs_selected(
            update.config,
            site.catalogs_dir(prepared.workspace_dir_for_catalogs.as_deref()),
            &prepared.updated_catalogs,
            selected.projects,
        )
        .map_err(UpdateError::WriteWorkspaceManifest)?;
    }

    let bumps = (!prepared.bump_targets.is_empty()).then(|| ManifestSpecBumps {
        targets: std::mem::take(&mut prepared.bump_targets),
        range_spec_style: RangeSpecStyle::from_save_options(update.version.save_exact, None),
        applied: Mutex::default(),
    });
    let ignored_builds = run_selected_update_install::<Reporter, _>(
        update_install(
            update,
            owned,
            manifest,
            prepared.take_seed(update),
            site.read_package_hook.as_ref(),
        ),
        selected.selection(),
        unsaved,
        bumps.as_ref(),
    )
    .await?;

    settle_selected_update::<Reporter>(
        update,
        &site,
        selected.projects,
        manifest,
        prepared,
        bumps,
    )?;
    if let Some(ignored_builds) = ignored_builds {
        return Err(UpdateError::Install(ignored_builds));
    }
    Ok(())
}
pub(super) async fn run_prepared_update<Reporter: self::Reporter + 'static>(
    update: UpdateOptions<'_>,
    owned: UpdateResources,
    manifest: &mut PackageManifest,
    site: UpdateSite,
    unsaved: UnsavedManifests,
    mut prepared: UpdatePreparation,
) -> Result<(), UpdateError> {
    if update.version.save {
        write_workspace_catalogs(
            update.config,
            prepared.workspace_dir_for_catalogs.as_deref(),
            &prepared.updated_catalogs,
            manifest,
        )
        .map_err(UpdateError::WriteWorkspaceManifest)?;
    }
    let importer_id =
        pnpm_workspace::importer_id_from_root_dir(&site.workspace_root, manifest_dir(manifest));
    let bumps = (!prepared.bump_targets.is_empty()).then(|| ManifestSpecBumps {
        targets: BTreeMap::from([(
            importer_id.clone(),
            std::mem::take(&mut prepared.bump_targets),
        )]),
        range_spec_style: RangeSpecStyle::from_save_options(update.version.save_exact, None),
        applied: Mutex::default(),
    });
    let ignored_builds = run_update_install::<Reporter, _>(
        update_install(
            update,
            owned,
            manifest,
            prepared.take_seed(update.version.patches),
            site.read_package_hook.as_ref(),
        ),
        unsaved,
        bumps.as_ref(),
    )
    .await?;

    finish_single_update::<Reporter>(
        update,
        manifest,
        &prepared,
        &importer_id,
        bumps,
        ignored_builds,
    )
}

/// What `--no-save` hands the install: the manifest paths the read-package
/// hook already rewrote, and the on-disk manifests the lockfile's importer
/// specifiers come from.
pub(super) struct UnsavedManifests {
    pub(super) hooked_paths: HashSet<PathBuf>,
    pub(super) lockfile_specifiers: Option<Vec<(PathBuf, PackageManifest)>>,
}
/// What the resolve seeds from: the pins it keeps or drops, the versions it
/// prefers, and the catalogs as the update rewrote them.
pub(super) struct UpdateSeed {
    pub(super) policy: UpdateSeedPolicy,
    pub(super) preferred_versions_override: PreferredVersions,
    pub(super) catalogs_override: Option<Catalogs>,
}
/// An explicitly selected peer group reaches resolution; the install honors
/// `autoInstallPeers` when deciding whether to materialize those peers.
/// `update` always re-resolves against the registry, so the
/// auto-frozen / repeat-install fast paths must not fire.
pub(super) fn update_install<'i>(
    update: UpdateOptions<'i>,
    owned: UpdateResources,
    manifest: &'i PackageManifest,
    seed: UpdateSeed,
    read_package_hook: Option<&ReadPackageHook>,
) -> Install<'i, Vec<DependencyGroup>> {
    let dependency_groups = update_dependency_groups(&update, &owned);
    Install {
        lockfile_policy: update.lockfile_policy(),
        execution: crate::InstallExecution {
            skip_runtimes: update.config.skip_runtimes,
            mutation: update_mutation(update.selection.packages, update.version.latest),
            installs_only: true,
            node_linker: update.config.node_linker,
            lockfile_only: update.lockfile_only,
            dry_run: false,
        },
        resolution: crate::ResolutionInputs {
            update_seed_policy: seed.policy,
            preferred_versions_override: Some(seed.preferred_versions_override),
            auth_override: None,
            observer: owned.resolution_observer,
            peer_issues_sink: None,
            deps_requiring_build_sink: None,
        },
        context: crate::InstallInvocation {
            http_client: update.http_client,
            config: update.config,
            manifest,
            emit_initial_manifest: false,
            lockfile: update.lockfile.source,
            lockfile_path: update.lockfile.path,
        },
        fetching: crate::InstallFetching {
            tarball_mem_cache: owned.tarball_mem_cache,
            http_client_arc: owned.http_client_arc,
            resolved_packages: update.resolved_packages,
        },
        projects: crate::InstallProjects {
            dependency_groups,
            supported_architectures: owned.supported_architectures,
            catalogs_override: seed.catalogs_override,
            pnpmfile_hook_override: read_package_hook.map(|(hook, _)| Arc::clone(hook)),
            workspace_projects_override: None,
        },
    }
}
fn update_dependency_groups(
    update: &UpdateOptions<'_>,
    owned: &UpdateResources,
) -> Vec<DependencyGroup> {
    let prior_included = pnpm_modules_yaml::read_modules_layout::<pnpm_modules_yaml::Host>(
        &update.config.modules_dir,
    )
    .ok()
    .flatten()
    .map(|layout| layout.included);
    let is_explicit_dev = owned.explicit_groups.dev;
    let is_explicit_prod = owned.explicit_groups.prod;
    let is_explicit_optional = owned.explicit_groups.optional;

    let (prod, dev, optional) = if let Some(included) = prior_included {
        (
            included.dependencies || is_explicit_prod,
            included.dev_dependencies || is_explicit_dev,
            !owned.explicit_groups.no_optional
                && (is_explicit_optional
                    || (included.optional_dependencies && update.config.optional)),
        )
    } else {
        (
            true,
            !is_explicit_prod || is_explicit_dev,
            !owned.explicit_groups.no_optional
                && (is_explicit_optional || update.config.optional),
        )
    };

    std::iter::empty()
        .chain(prod.then_some(DependencyGroup::Prod))
        .chain(dev.then_some(DependencyGroup::Dev))
        .chain(optional.then_some(DependencyGroup::Optional))
        .chain(
            owned.include_direct.contains(&DependencyGroup::Peer).then_some(DependencyGroup::Peer),
        )
        .collect()
}
/// A selector that matched nothing at depth 0 is an error; anything else
/// leaves the command a no-op.
pub(super) fn nothing_to_update(
    depth: usize,
    packages: &[String],
    latest: bool,
) -> Result<(), UpdateError> {
    if depth == 0 && !packages.is_empty() && !latest {
        return Err(UpdateError::NoPackageInDependencies);
    }
    Ok(())
}
pub(super) async fn run_update_install<Reporter, DependencyGroupList>(
    install: Install<'_, DependencyGroupList>,
    unsaved: UnsavedManifests,
    bumps: Option<&ManifestSpecBumps>,
) -> Result<Option<crate::InstallError>, UpdateError>
where
    Reporter: self::Reporter + 'static,
    DependencyGroupList: IntoIterator<Item = DependencyGroup> + Send,
{
    match unsaved.lockfile_specifiers {
        Some(manifests) => {
            install.run_with_lockfile_specifier_project_manifests::<Reporter>(
                manifests,
                unsaved.hooked_paths,
            )
            .await
        }
        None => match bumps {
            Some(bumps) => install.run_with_manifest_spec_bumps::<Reporter>(bumps).await,
            None => install.run::<Reporter>().await,
        },
    }
    .pipe(defer_post_install_errors)
    .map_err(UpdateError::Install)
}
/// Run the pnpmfile's `readPackage` hook over every selected project's
/// manifest, and over the active one, each at most once.
pub(super) async fn hook_selected_manifests(
    projects: &mut [pnpm_workspace::Project],
    manifest: &mut PackageManifest,
    hook: &Arc<dyn pnpm_hooks::PnpmfileHooks>,
    log: &pnpm_hooks::LogFn,
    hooked_paths: &mut HashSet<PathBuf>,
) -> Result<(), UpdateError> {
    for project in projects.iter_mut() {
        if hooked_paths.insert(project.manifest.path().to_path_buf()) {
            apply_read_package_hook_to_update_manifest(&mut project.manifest, hook, log).await?;
        }
    }
    if hooked_paths.insert(manifest.path().to_path_buf()) {
        apply_read_package_hook_to_update_manifest(manifest, hook, log).await?;
    }
    Ok(())
}
pub(super) async fn run_selected_update_install<Reporter, DependencyGroupList>(
    install: Install<'_, DependencyGroupList>,
    selection: WorkspaceInstallSelection<'_>,
    unsaved: UnsavedManifests,
    bumps: Option<&ManifestSpecBumps>,
) -> Result<Option<crate::InstallError>, UpdateError>
where
    Reporter: self::Reporter + 'static,
    DependencyGroupList: IntoIterator<Item = DependencyGroup> + Send,
{
    match unsaved.lockfile_specifiers {
        Some(manifests) => {
            install.run_selected_with_lockfile_specifier_project_manifests::<Reporter>(
                selection,
                manifests,
                unsaved.hooked_paths,
            )
            .await
        }
        None => match bumps {
            Some(bumps) => {
                install.run_selected_with_manifest_spec_bumps::<Reporter>(selection, bumps).await
            }
            None => install.run_selected::<Reporter>(selection).await,
        },
    }
    .pipe(defer_post_install_errors)
    .map_err(UpdateError::Install)
}

impl UpdateOptions<'_> {
    fn lockfile_policy(self) -> crate::InstallLockfilePolicy {
        crate::InstallLockfilePolicy {
            frozen: false,
            prefer_frozen: Some(false),
            ignore_manifest_check: false,
            trust: self.config.trust_lockfile,
            update_checksums: self.version.patches,
            excludes: if self.version.save {
                PolicyExcludes::Persist
            } else {
                PolicyExcludes::Forbidden
            },
            disable_optimistic_repeat: false,
            manifest_freshness: crate::ManifestFreshness::Mtime,
        }
    }
}
