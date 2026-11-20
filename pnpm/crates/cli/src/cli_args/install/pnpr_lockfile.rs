pub(super) use local::{
    LocalLockfileInstall,
    install_from_local_lockfile,
    verify_frozen_pnpr_preflight,
};

mod local;

use super::{
    Context,
    DependencyGroup,
    IncludedDependencies,
    InstallFamilySelection,
    InstallFrozenLockfileError,
    Lockfile,
    LockfileVerificationOverride,
    MaybeLazyLockfile,
    NodeLinker,
    PnprClient,
    PnprClientError,
    PnprLink,
    PnprRequestInputs,
    PnprSession,
    Reporter,
    ResolveProject,
    SkippedSnapshots,
    State,
    TarballPrefetcher,
    VerifyLockfileOptions,
    build_resolution_verifiers,
    check_frozen_pnpmfile,
    config_exotic_subdeps_policy,
    materialization_closure,
    merge_filtered_wanted_lockfile,
    record_pnpmfile,
    state_resolution_verifiers,
    verified_project_workspace_dirs,
    verify_pnpr_input_lockfile,
    verify_settled_pnpr_lockfile,
    workspace_install_selection,
};
use pnpm_lockfile_verification::{
    ExoticSubdepsPolicy,
    LockfileStat,
    VerifiedOfflineLockfile,
    VerifyLockfileResolutionsOptions,
    lockfile_verification_is_cached_by_content,
    lockfile_verification_is_cached_with_stat,
    record_lockfile_verified_by_content,
    record_lockfile_verified_with_stat,
    stat_from_metadata,
    verify_lockfile_resolutions,
};

pub(in crate::cli_args::install) fn pnpr_exotic_subdeps_policy(
    state: &State,
    link: &PnprLink<'_>,
    session: &PnprSession<'_>,
) -> ExoticSubdepsPolicy {
    if link.lockfile.trust || !state.config.block_exotic_subdeps {
        return ExoticSubdepsPolicy::Allow;
    }
    let lockfile_dir = pnpr_lockfile_dir(state, link);
    let verified_dirs = pnpr_verified_workspace_dirs(lockfile_dir, &session.projects);
    let extra_dirs = verified_dirs.iter().map(String::as_str);
    let real_ids = session.selection_importer_ids
        .as_ref()
        .or(session.full_workspace_importer_ids.as_ref())
        .map(|(real, _)| real);
    match real_ids {
        Some(ids) => config_exotic_subdeps_policy(
            state.config,
            Some(ids.iter().map(String::as_str)),
            extra_dirs,
        ),
        None if !session.projects.is_empty() => config_exotic_subdeps_policy(
            state.config,
            Some(session.projects.iter().map(|project| project.dir.as_str())),
            extra_dirs,
        ),
        None => config_exotic_subdeps_policy(
            state.config,
            Some([Lockfile::ROOT_IMPORTER_KEY]),
            extra_dirs,
        ),
    }
}

fn pnpr_verified_workspace_dirs(
    lockfile_dir: &std::path::Path,
    projects: &[ResolveProject],
) -> Vec<String> {
    let mut dirs = std::collections::BTreeSet::new();
    for project in projects {
        add_resolve_project_dirs(lockfile_dir, project, &mut dirs);
    }
    dirs.into_iter().collect()
}

fn add_resolve_project_dirs(
    lockfile_dir: &std::path::Path,
    project: &ResolveProject,
    dirs: &mut std::collections::BTreeSet<String>,
) {
    let root = pnpm_fs::lexical_normalize(&lockfile_dir.join(&project.dir));
    dirs.insert(pnpm_workspace::importer_id_from_root_dir(lockfile_dir, &root));
    if let Some(pub_cfg) = &project.publish_config
        && pub_cfg.link_directory.unwrap_or(true)
    {
        let pub_dir = pnpm_fs::lexical_normalize(&root.join(&pub_cfg.directory));
        if pnpm_package_manager::is_verified_publish_dir(&root, &pub_dir) {
            dirs.insert(pnpm_workspace::importer_id_from_root_dir(lockfile_dir, &pub_dir));
        }
    }
    let has_ws_spec = project.dependencies
        .values()
        .chain(project.dev_dependencies.values())
        .chain(project.optional_dependencies.values())
        .any(|spec| spec.starts_with("workspace:"));
    if has_ws_spec {
        let synthetic = serde_json::json!({ "dependencies": &project.dependencies, "devDependencies": &project.dev_dependencies, "optionalDependencies": &project.optional_dependencies });
        dirs.extend(
            verified_project_workspace_dirs(lockfile_dir, [(root.as_path(), &synthetic)])
                .into_iter()
                .filter(|dir| !dir.starts_with(".."))
                .filter(|dir| !dir.starts_with('/')),
        );
    }
}

/// Link `node_modules` from the server-produced lockfile through the normal
/// frozen install.
pub(super) async fn link_pnpr_lockfile<Reporter: self::Reporter + 'static>(
    state: &State,
    selection: Option<&InstallFamilySelection>,
    link: PnprLink<'_>,
    lockfile: &Lockfile,
    known_requires_build: Option<std::collections::HashMap<pnpm_lockfile::PackageKey, bool>>,
    pnpmfile_hook: Option<std::sync::Arc<dyn pnpm_hooks::PnpmfileHooks>>,
) -> miette::Result<()> {
    let recorded_trust = link.lockfile.trust;
    let install = {
        let mut base_install = state.install(link.dependency_groups);
        base_install.lockfile_policy.frozen = true;
        base_install.lockfile_policy.ignore_manifest_check = link.lockfile.ignore_manifest_check;
        base_install.execution.skip_runtimes = link.skip_runtimes;
        base_install.execution.node_linker = link.node_linker;
        base_install.context.lockfile_path = link.lockfile_path;
        base_install.context.lockfile = MaybeLazyLockfile::Loaded(Some(lockfile));
        base_install.projects.supported_architectures = link.supported_architectures;
        base_install.projects.pnpmfile_hook_override = pnpmfile_hook;
        base_install
    };
    install
        .run_pnpr_link::<Reporter>(
            selection.map(workspace_install_selection),
            None,
            known_requires_build,
            recorded_trust,
        )
        .await
        .wrap_err("linking dependencies resolved via the pnpr server")
}

/// Fold the server's answer for the selected importers back into the
/// lockfile the rest of the workspace still resolves through. A run that
/// has neither a selection nor a workspace has nothing to merge into.
fn merge_selected_importers(
    state: &State,
    selection: Option<&InstallFamilySelection>,
    importer_ids: Option<&(std::collections::HashSet<String>, std::collections::HashSet<String>)>,
    merge_wanted: Option<&Lockfile>,
    resolved: Lockfile,
) -> miette::Result<Lockfile> {
    let workspace_root = selection
        .map(|selection| selection.workspace_root.as_path())
        .or(state.config.workspace_dir.as_deref())
        .map(|root| state.config.lockfile_dir_for(root));
    let (Some((real_importer_ids, selected_importer_ids)), Some(workspace_root)) =
        (importer_ids, workspace_root)
    else {
        return Ok(resolved);
    };
    merge_filtered_wanted_lockfile(
        merge_wanted,
        resolved,
        real_importer_ids,
        selected_importer_ids,
        workspace_root,
    )
    .map_err(miette::Report::new)
}

/// The importer ids of the selected projects, as `(every project of the
/// selection, the ones being installed)`.
///
/// Importer ids name projects relative to the lockfile, which
/// `lockfileDir` can pin somewhere other than the workspace the selection
/// was resolved in. The server request, the merge, and the lockfile on
/// disk all have to agree on them.
pub(super) fn selection_importer_ids(
    state: &State,
    selection: Option<&InstallFamilySelection>,
) -> Option<(std::collections::HashSet<String>, std::collections::HashSet<String>)> {
    let selection = selection?;
    let importer_root = state.config.lockfile_dir_for(&selection.workspace_root);
    let real_importer_ids = selection.projects
        .iter()
        .map(|project| pnpm_workspace::importer_id_from_root_dir(importer_root, &project.root_dir))
        .collect();
    let selected_importer_ids = selection.install_dirs
        .iter()
        .map(|project_dir| pnpm_workspace::importer_id_from_root_dir(importer_root, project_dir))
        .collect();
    Some((real_importer_ids, selected_importer_ids))
}

/// A workspace-wide install has no selection, but its merge still has to
/// name every importer the one shared lockfile covers.
pub(super) fn full_workspace_importer_ids(
    state: &State,
    selection: Option<&InstallFamilySelection>,
    link: &PnprLink<'_>,
    projects: &[ResolveProject],
) -> Option<(std::collections::HashSet<String>, std::collections::HashSet<String>)> {
    if selection.is_some()
        || !link.use_state_lockfile
        || !state.config.shares_one_lockfile()
        || state.config.workspace_dir.is_none()
    {
        return None;
    }
    let importer_ids: std::collections::HashSet<_> = projects
        .iter()
        .map(|project| project.dir.clone())
        .collect();
    Some((importer_ids.clone(), importer_ids))
}

/// Write the server-resolved lockfile and record that it verified, so
/// the next install can skip the verification round trip. A run with
/// `lockfile: false` writes nothing.
fn save_pnpr_lockfile(
    state: &State,
    link: &PnprLink<'_>,
    lockfile: &Lockfile,
    lockfile_path: &std::path::Path,
    exotic_subdeps: &ExoticSubdepsPolicy,
    merged_repair_verifiers: Option<
        &[std::sync::Arc<dyn pnpm_resolving_resolver_base::ResolutionVerifier>],
    >,
) -> miette::Result<()> {
    if !state.config.lockfile {
        return Ok(());
    }
    lockfile
        .save_to_path(lockfile_path, state.config.symlinked_lockfile())
        .map_err(|err| miette::miette!("{err}"))
        .wrap_err("writing the pnpr-resolved lockfile")?;
    if link.lockfile.trust {
        return Ok(());
    }
    if let Some(verifiers) = merged_repair_verifiers {
        record_lockfile_verified_by_content(
            Some(&state.config.cache_dir),
            lockfile,
            verifiers,
            exotic_subdeps,
        );
        return Ok(());
    }
    if let Ok(verifiers) = build_resolution_verifiers(
        state.config,
        std::sync::Arc::clone(&state.http_client),
        None,
        None,
        None,
        None,
        None,
    ) {
        record_lockfile_verified_by_content(
            Some(&state.config.cache_dir),
            lockfile,
            &verifiers,
            exotic_subdeps,
        );
    }
    Ok(())
}

pub(super) fn pnpr_lockfile_dir<'a>(state: &'a State, link: &PnprLink<'a>) -> &'a std::path::Path {
    link.lockfile_path
        .and_then(|path| path.parent())
        .unwrap_or_else(|| {
            state.manifest
                .path()
                .parent()
                .expect("manifest path always has a parent dir")
        })
}

pub(super) async fn merge_and_save_pnpr_lockfile<Reporter: self::Reporter + 'static>(
    state: &State,
    selection: Option<&InstallFamilySelection>,
    link: &PnprLink<'_>,
    session: &PnprSession<'_>,
    inputs: &PnprRequestInputs,
    mut lockfile: Lockfile,
) -> miette::Result<Lockfile> {
    if let Some(registry) = inputs.benchmark_registry_override.as_ref() {
        registry.rewrite_lockfile(&mut lockfile);
    }
    let importer_ids = session.selection_importer_ids
        .as_ref()
        .or(session.full_workspace_importer_ids.as_ref());
    lockfile =
        merge_selected_importers(state, selection, importer_ids, session.merge_wanted, lockfile)?;
    if !link.lockfile.frozen {
        record_pnpmfile(inputs.pnpmfile_hook.as_ref(), &mut lockfile).await?;
    }
    let merged_repair_verifiers = verify_settled_pnpr_lockfile::<Reporter>(
        state,
        link,
        session,
        &inputs.exotic_subdeps,
        &lockfile,
    )
    .await?;

    save_pnpr_lockfile(
        state,
        link,
        &lockfile,
        &inputs.lockfile_path,
        &inputs.exotic_subdeps,
        merged_repair_verifiers.as_deref(),
    )?;

    Ok(lockfile)
}
