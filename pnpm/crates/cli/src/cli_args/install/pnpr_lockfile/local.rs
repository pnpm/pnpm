use super::{
    Context,
    DependencyGroup,
    ExoticSubdepsPolicy,
    IncludedDependencies,
    InstallFamilySelection,
    InstallFrozenLockfileError,
    Lockfile,
    LockfileStat,
    LockfileVerificationOverride,
    MaybeLazyLockfile,
    NodeLinker,
    PnprClient,
    PnprClientError,
    PnprLink,
    PnprRequestInputs,
    PnprSession,
    Reporter,
    SkippedSnapshots,
    State,
    TarballPrefetcher,
    VerifiedOfflineLockfile,
    VerifyLockfileOptions,
    VerifyLockfileResolutionsOptions,
    check_frozen_pnpmfile,
    lockfile_verification_is_cached_by_content,
    lockfile_verification_is_cached_with_stat,
    materialization_closure,
    record_lockfile_verified_by_content,
    record_lockfile_verified_with_stat,
    stat_from_metadata,
    state_resolution_verifiers,
    verify_lockfile_resolutions,
    verify_pnpr_input_lockfile,
    workspace_install_selection,
};

type SingleLockfileSource<'s> = (&'s std::path::Path, LockfileStat);
type OwnedLockfileSource = (std::path::PathBuf, LockfileStat);

/// What [`install_from_local_lockfile`] needs beyond the shared state.
pub(in crate::cli_args::install) struct LocalLockfileInstall<'a> {
    pub(in crate::cli_args::install) lockfile: &'a Lockfile,
    pub(in crate::cli_args::install) lockfile_dir: &'a std::path::Path,
    pub(in crate::cli_args::install) session: &'a PnprSession<'a>,
    pub(in crate::cli_args::install) inputs: &'a PnprRequestInputs,
}

/// Materialize from the lockfile already on disk, with the pnpr server
/// consulted only to verify it.
pub(in crate::cli_args::install) async fn install_from_local_lockfile<
    Reporter: self::Reporter + 'static,
>(
    state: &State,
    pnpr_server: &str,
    selection: Option<&InstallFamilySelection>,
    link: &PnprLink<'_>,
    local: &LocalLockfileInstall<'_>,
) -> miette::Result<()> {
    let verified_input = verify_pnpr_input_lockfile(link, local.lockfile)?;
    verify_local_lockfile_offline::<Reporter>(
        state,
        link,
        (single_on_disk_lockfile_source(state), &local.inputs.exotic_subdeps),
        local.lockfile,
    )
    .await?;
    let prefetcher = prefetch_local_lockfile(state, link, local).await;
    let verification_override =
        local_lockfile_verification(state, pnpr_server, link, local, verified_input)?;

    // On failure the prefetcher is dropped, not shut down: shutdown waits for
    // every in-flight prefetch download, which would hold the fail-fast abort
    // hostage to the remaining transfers.
    run_local_frozen_install::<Reporter>(state, selection, link, local, verification_override)
        .await
        .wrap_err("restoring dependencies from the local lockfile via pnpr verification")?;

    if let Some(prefetcher) = prefetcher {
        prefetcher.shutdown().await;
    }
    Ok(())
}

pub(in crate::cli_args::install) async fn verify_frozen_pnpr_preflight<
    Reporter: self::Reporter + 'static,
>(
    state: &State,
    link: &PnprLink<'_>,
    session: &PnprSession<'_>,
    inputs: &PnprRequestInputs,
) -> miette::Result<()> {
    if link.lockfile.frozen
        && let Some(previous_wanted) = session.previous_wanted
    {
        verify_pnpr_input_lockfile(link, previous_wanted)?;
        verify_local_lockfile_offline::<Reporter>(
            state,
            link,
            (single_on_disk_lockfile_source(state), &inputs.exotic_subdeps),
            previous_wanted,
        )
        .await?;
        check_frozen_pnpmfile(state, inputs.pnpmfile_hook.as_ref(), previous_wanted).await?;
    }
    Ok(())
}

fn single_on_disk_lockfile_source(state: &State) -> Option<SingleLockfileSource<'_>> {
    let clean_load = MaybeLazyLockfile::Lazy(&state.lockfile)
        .merged_conflict_files()
        .is_ok_and(|count| count == 0)
        && state.lockfile.pre_merge_importers().is_ok_and(|importers| importers.is_none());
    if !clean_load {
        return None;
    }
    let path = state.lockfile
        .loaded_path()
        .ok()
        .flatten()?;
    let metadata = state.lockfile
        .loaded_metadata()
        .ok()
        .flatten()?;
    Some((path, stat_from_metadata(metadata)))
}

fn pnpr_verification_is_cached(
    state: &State,
    single_source: Option<SingleLockfileSource<'_>>,
    lockfile: &Lockfile,
    verifiers: &[std::sync::Arc<dyn pnpm_resolving_resolver_base::ResolutionVerifier>],
    exotic_subdeps: &ExoticSubdepsPolicy,
) -> bool {
    match single_source {
        Some((path, stat)) => lockfile_verification_is_cached_with_stat(
            &state.config.cache_dir,
            path,
            stat,
            lockfile,
            verifiers,
            exotic_subdeps,
        ),
        None => lockfile_verification_is_cached_by_content(
            &state.config.cache_dir,
            lockfile,
            verifiers,
            exotic_subdeps,
        ),
    }
}

pub(super) async fn verify_local_lockfile_offline<Reporter: self::Reporter + 'static>(
    state: &State,
    link: &PnprLink<'_>,
    (single_source, exotic_subdeps): (Option<SingleLockfileSource<'_>>, &ExoticSubdepsPolicy),
    lockfile: &Lockfile,
) -> miette::Result<()> {
    if link.lockfile.trust {
        return Ok(());
    }
    if pnpr_verification_is_cached(state, single_source, lockfile, &[], exotic_subdeps) {
        return Ok(());
    }
    verify_lockfile_resolutions::<Reporter>(
        lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            exotic_subdeps: exotic_subdeps.clone(),
            ..Default::default()
        },
    )
    .await
    .map_err(miette::Report::new)
}

async fn run_local_frozen_install<'a, Reporter: self::Reporter + 'static>(
    state: &'a State,
    selection: Option<&InstallFamilySelection>,
    link: &PnprLink<'a>,
    local: &LocalLockfileInstall<'a>,
    verification_override: Option<LockfileVerificationOverride<'a>>,
) -> Result<(), pnpm_package_manager::InstallError> {
    let mut install = state.install(link.dependency_groups.clone());
    install.lockfile_policy.frozen = true;
    install.lockfile_policy.ignore_manifest_check = link.lockfile.ignore_manifest_check;
    install.execution.skip_runtimes = link.skip_runtimes;
    install.execution.node_linker = link.node_linker;
    install.execution.lockfile_only = link.lockfile.only;
    install.context.lockfile_path = link.lockfile_path;
    install.context.lockfile = MaybeLazyLockfile::Loaded(Some(local.lockfile));
    install.projects.supported_architectures = link.supported_architectures.clone();
    install.projects.pnpmfile_hook_override = local.inputs.pnpmfile_hook.clone();

    install.run_pnpr_link::<Reporter>(
        selection.map(workspace_install_selection),
        verification_override,
        None,
        link.lockfile.trust,
    )
    .await
}

/// Warm the tarball mem cache from the lockfile this install is about to
/// materialize, so the frozen install below finds its downloads in
/// flight. Skipped under `--lockfile-only`, which materializes nothing,
/// and when a custom fetcher owns the fetch.
async fn prefetch_local_lockfile(
    state: &State,
    link: &PnprLink<'_>,
    local: &LocalLockfileInstall<'_>,
) -> Option<TarballPrefetcher> {
    if link.lockfile.only || !local.inputs.prefetch_allowed {
        return None;
    }
    let selected_prefetch_lockfile = selected_prefetch_lockfile(link, local, state.config);
    let prefetcher = TarballPrefetcher::new(
        state.config,
        &state.http_client,
        &state.tarball_mem_cache,
        None,
        &local.lockfile_dir.to_string_lossy(),
        None,
    )
    .await;
    prefetcher.prefetch_lockfile(
        selected_prefetch_lockfile.as_ref().unwrap_or(local.lockfile),
        state.config,
    )
    .await;
    tokio::task::yield_now().await;
    Some(prefetcher)
}

fn verified_input_is_cached(
    state: &State,
    verified_input: VerifiedOfflineLockfile<'_>,
    single_source: Option<SingleLockfileSource<'_>>,
    verifiers: &[std::sync::Arc<dyn pnpm_resolving_resolver_base::ResolutionVerifier>],
    exotic_subdeps: &ExoticSubdepsPolicy,
) -> bool {
    match single_source {
        Some((path, stat)) => verified_input.is_cached_with_stat(
            &state.config.cache_dir,
            path,
            stat,
            verifiers,
            exotic_subdeps,
        ),
        None => {
            verified_input.is_cached_by_content(&state.config.cache_dir, verifiers, exotic_subdeps)
        }
    }
}

/// The verification the frozen install runs concurrently with its fetch.
/// `None` under `--trust-lockfile`, and when this exact lockfile was
/// already verified against the same resolvers.
fn local_lockfile_verification<'a>(
    state: &'a State,
    pnpr_server: &str,
    link: &PnprLink<'_>,
    local: &LocalLockfileInstall<'a>,
    verified_input: VerifiedOfflineLockfile<'a>,
) -> miette::Result<Option<LockfileVerificationOverride<'a>>> {
    if link.lockfile.trust {
        return Ok(None);
    }
    let verifiers = state_resolution_verifiers(state)?;
    let exotic_subdeps = local.inputs.exotic_subdeps.clone();
    let single_source = single_on_disk_lockfile_source(state);
    if verified_input_is_cached(
        state,
        verified_input,
        single_source.clone(),
        &verifiers,
        &exotic_subdeps,
    ) {
        return Ok(Some(Box::pin(async { Ok(()) }) as LockfileVerificationOverride<'a>));
    }
    let owned_source: Option<OwnedLockfileSource> =
        single_source.map(|(path, stat)| (path.to_path_buf(), stat));
    let verify_opts = local_verify_options(state, pnpr_server, link, local);
    let verify_client = PnprClient::new(pnpr_server);
    let record_cache_dir = state.config.cache_dir.clone();
    let lockfile = local.lockfile;
    Ok(Some(Box::pin(async move {
        verify_client.verify_lockfile(verify_opts).await.map_err(map_pnpr_verify_error)?;
        record_local_pnpr_verified(
            &record_cache_dir,
            owned_source,
            lockfile,
            &verifiers,
            &exotic_subdeps,
        );
        Ok(())
    }) as LockfileVerificationOverride<'a>))
}

fn map_pnpr_verify_error(err: PnprClientError) -> InstallFrozenLockfileError {
    match err {
        PnprClientError::Verification(verify_err) => {
            InstallFrozenLockfileError::LockfileVerification(verify_err)
        }
        other => InstallFrozenLockfileError::ExternalLockfileVerification(other.to_string()),
    }
}

fn record_local_pnpr_verified(
    cache_dir: &std::path::Path,
    owned_source: Option<OwnedLockfileSource>,
    lockfile: &Lockfile,
    verifiers: &[std::sync::Arc<dyn pnpm_resolving_resolver_base::ResolutionVerifier>],
    exotic_subdeps: &ExoticSubdepsPolicy,
) {
    match owned_source {
        Some((record_path, stat)) => {
            record_lockfile_verified_with_stat(
                Some(cache_dir),
                &record_path,
                stat,
                lockfile,
                verifiers,
                exotic_subdeps,
            );
        }
        None => {
            record_lockfile_verified_by_content(
                Some(cache_dir),
                lockfile,
                verifiers,
                exotic_subdeps,
            );
        }
    }
}

fn local_verify_options(
    state: &State,
    pnpr_server: &str,
    link: &PnprLink<'_>,
    local: &LocalLockfileInstall<'_>,
) -> VerifyLockfileOptions {
    VerifyLockfileOptions {
        overrides: local.inputs.overrides.clone(),
        lockfile: local.lockfile.clone(),
        trust_lockfile: link.lockfile.trust,
        routing: pnpm_pnpr_client::RegistryRouting {
            registry: local.inputs.resolve_registry.clone(),
            registries: state.config.registry_declarations(),
            authorization: state.config.auth_headers.for_url(pnpr_server),
        },
        verification: super::super::pnpr_request::verification_policy(state.config),
    }
}

fn selected_prefetch_lockfile(
    link: &PnprLink<'_>,
    local: &LocalLockfileInstall<'_>,
    config: &pnpm_config::Config,
) -> Option<Lockfile> {
    local.session.selection_importer_ids
        .as_ref()
        .map(|(_, selected_importer_ids)| {
            let hoisted_ids = matches!(link.node_linker, NodeLinker::Hoisted).then(|| {
                local.lockfile.importers
                    .keys()
                    .cloned()
                    .collect()
            });
            let initial_importer_ids = hoisted_ids.as_ref().unwrap_or(selected_importer_ids);
            materialization_closure(
                local.lockfile,
                local.lockfile_dir,
                initial_importer_ids,
                &pnpm_package_manager::GroupSelection::classify(
                    local.lockfile,
                    IncludedDependencies {
                        dependencies: link.dependency_groups.contains(&DependencyGroup::Prod),
                        dev_dependencies: link.dependency_groups.contains(&DependencyGroup::Dev),
                        optional_dependencies: link.dependency_groups.contains(
                            &DependencyGroup::Optional,
                        ),
                    },
                    config.peer_edge_options(),
                ),
                &SkippedSnapshots::new(),
            )
            .lockfile
        })
}
