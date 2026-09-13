pub(super) use selection::{full_workspace_importer_ids, selection_importer_ids};

use selection::selected_prefetch_lockfile;

use super::{
    Context, DependencyGroup, IncludedDependencies, InstallFamilySelection,
    InstallFrozenLockfileError, Lockfile, LockfileVerificationOverride, MaybeLazyLockfile,
    NodeLinker, PnprClient, PnprClientError, PnprLink, PnprRequestInputs, PnprSession, Reporter,
    ResolveProject, SkippedSnapshots, State, TarballPrefetcher, VerifyLockfileOptions,
    VerifyLockfileResolutionsOptions, build_resolution_verifiers, lockfile_verification_is_cached,
    lockfile_verification_is_cached_by_content, materialization_closure,
    merge_filtered_wanted_lockfile, record_lockfile_verified, verify_lockfile_resolutions,
    workspace_install_selection,
};

/// Link `node_modules` from the server-produced lockfile through the normal
/// frozen install.
pub(super) async fn link_pnpr_lockfile<Reporter: self::Reporter + 'static>(
    state: &State,
    selection: Option<&InstallFamilySelection>,
    link: PnprLink<'_>,
    lockfile: &Lockfile,
    pnpmfile_hook: Option<std::sync::Arc<dyn pnpm_hooks::PnpmfileHooks>>,
) -> miette::Result<()> {
    let install = {
        let mut base_install = state.install(link.dependency_groups);
        base_install.lockfile_policy.frozen = true;
        base_install.lockfile_policy.ignore_manifest_check = link.lockfile.ignore_manifest_check;
        base_install.lockfile_policy.trust = true;
        base_install.execution.skip_runtimes = link.skip_runtimes;
        base_install.execution.node_linker = link.node_linker;
        base_install.context.lockfile_path = link.lockfile_path;
        base_install.context.lockfile = MaybeLazyLockfile::Loaded(Some(lockfile));
        base_install.projects.supported_architectures = link.supported_architectures;
        base_install.projects.pnpmfile_hook_override = pnpmfile_hook;
        base_install
    };
    match selection {
        Some(selection) => {
            Box::pin(install.run_selected::<Reporter>(workspace_install_selection(selection))).await
        }
        None => install.run::<Reporter>().await,
    }
    .wrap_err("linking dependencies resolved via the pnpr server")
}

/// Fold the server's answer for the selected importers back into the
/// lockfile the rest of the workspace still resolves through. A run that
/// has neither a selection nor a workspace has nothing to merge into.
fn merge_selected_importers(
    state: &State,
    selection: Option<&InstallFamilySelection>,
    importer_ids: Option<&(
        std::collections::HashSet<String>,
        std::collections::HashSet<String>,
    )>,
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

/// A `--fix-lockfile` run over part of the workspace merges repaired
/// entries with reused ones, so the merged result is verified here rather
/// than by the server. `None` when the run is not that shape.
async fn verify_merged_repair<Reporter: self::Reporter + 'static>(
    state: &State,
    link: &PnprLink<'_>,
    partial_selection: bool,
    lockfile: &Lockfile,
) -> miette::Result<Option<Vec<std::sync::Arc<dyn pnpm_resolving_resolver_base::ResolutionVerifier>>>>
{
    if !(link.lockfile.fix && partial_selection) {
        return Ok(None);
    }
    let verifiers = if link.lockfile.trust {
        Vec::new()
    } else {
        state_resolution_verifiers(state)?
    };
    if !lockfile_verification_is_cached_by_content(&state.config.cache_dir, lockfile, &verifiers) {
        verify_lockfile_resolutions::<Reporter>(
            lockfile,
            &verifiers,
            &VerifyLockfileResolutionsOptions::default(),
        )
        .await
        .map_err(miette::Report::new)?;
    }
    Ok(Some(verifiers))
}

/// Write the server-resolved lockfile and record that it verified, so
/// the next install can skip the verification round trip. A run with
/// `lockfile: false` writes nothing.
fn save_pnpr_lockfile(
    state: &State,
    link: &PnprLink<'_>,
    lockfile: &Lockfile,
    lockfile_path: &std::path::Path,
    merged_repair_verifiers: Option<
        &[std::sync::Arc<dyn pnpm_resolving_resolver_base::ResolutionVerifier>],
    >,
) -> miette::Result<()> {
    if !state.config.lockfile {
        return Ok(());
    }
    lockfile
        .save_to_path(lockfile_path)
        .map_err(|err| miette::miette!("{err}"))
        .wrap_err("writing the pnpr-resolved lockfile")?;
    if link.lockfile.trust {
        return Ok(());
    }
    if let Some(verifiers) = merged_repair_verifiers {
        record_lockfile_verified(
            Some(&state.config.cache_dir),
            lockfile_path,
            lockfile,
            verifiers,
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
    ) {
        record_lockfile_verified(
            Some(&state.config.cache_dir),
            lockfile_path,
            lockfile,
            &verifiers,
        );
    }
    Ok(())
}

/// What [`install_from_local_lockfile`] needs beyond the shared state.
pub(super) struct LocalLockfileInstall<'a> {
    pub(super) lockfile: &'a Lockfile,
    pub(super) lockfile_dir: &'a std::path::Path,
    pub(super) lockfile_path: &'a std::path::Path,
    pub(super) selection_importer_ids: Option<&'a (
        std::collections::HashSet<String>,
        std::collections::HashSet<String>,
    )>,
    pub(super) overrides: Option<&'a serde_json::Value>,
    pub(super) resolve_registry: &'a str,
    pub(super) prefetch_allowed: bool,
    pub(super) pnpmfile_hook: Option<std::sync::Arc<dyn pnpm_hooks::PnpmfileHooks>>,
}

/// Materialize from the lockfile already on disk, with the pnpr server
/// consulted only to verify it.
pub(super) async fn install_from_local_lockfile<Reporter: self::Reporter + 'static>(
    state: &State,
    pnpr_server: &str,
    selection: Option<&InstallFamilySelection>,
    link: &PnprLink<'_>,
    local: &LocalLockfileInstall<'_>,
) -> miette::Result<()> {
    let prefetcher = prefetch_local_lockfile(state, link, local).await;

    let lockfile_verification_override =
        local_lockfile_verification(state, pnpr_server, link, local)?;

    let install = local_lockfile_install(state, link, local);

    let result = match (selection, lockfile_verification_override) {
        (Some(selection), Some(lockfile_verification_override)) => {
            Box::pin(install.run_selected_with_lockfile_verification::<Reporter>(
                workspace_install_selection(selection),
                lockfile_verification_override,
            ))
            .await
        }
        (Some(selection), None) => {
            Box::pin(install.run_selected::<Reporter>(workspace_install_selection(selection))).await
        }
        (None, Some(lockfile_verification_override)) => {
            install.run_with_lockfile_verification::<Reporter>(lockfile_verification_override).await
        }
        (None, None) => install.run::<Reporter>().await,
    };
    // On failure the prefetcher is dropped, not shut down: shutdown
    // waits for every in-flight prefetch download (each task holds a
    // store-index writer handle), which would hold the fail-fast
    // abort hostage to the remaining transfers. The index rows are
    // best-effort — a dropped row only costs a later re-download.
    result.wrap_err("restoring dependencies from the local lockfile via pnpr verification")?;

    if let Some(prefetcher) = prefetcher {
        prefetcher.shutdown().await;
    }

    Ok(())
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
    if link.lockfile.only || !local.prefetch_allowed {
        return None;
    }
    let selected_prefetch_lockfile = selected_prefetch_lockfile(link, local);
    let prefetcher = TarballPrefetcher::new(
        state.config,
        &state.http_client,
        &state.tarball_mem_cache,
        None,
        &local.lockfile_dir.to_string_lossy(),
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

/// The verification the frozen install runs concurrently with its fetch.
/// `None` under `--trust-lockfile`, and when this exact lockfile was
/// already verified against the same resolvers.
fn local_lockfile_verification<'a>(
    state: &'a State,
    pnpr_server: &str,
    link: &PnprLink<'_>,
    local: &LocalLockfileInstall<'a>,
) -> miette::Result<Option<LockfileVerificationOverride<'a>>> {
    if link.lockfile.trust {
        return Ok(None);
    }
    let verifiers = state_resolution_verifiers(state)?;
    if lockfile_verification_is_cached(
        &state.config.cache_dir,
        local.lockfile_path,
        local.lockfile,
        &verifiers,
    ) {
        return Ok(None);
    }
    let verify_opts = local_verify_options(state, pnpr_server, link, local);
    let verify_client = PnprClient::new(pnpr_server);
    let cache_dir = state.config.cache_dir.clone();
    let record_lockfile_path = local.lockfile_path.to_path_buf();
    let lockfile = local.lockfile;
    Ok(Some(Box::pin(async move {
        match verify_client.verify_lockfile(verify_opts).await {
            Ok(()) => {
                record_lockfile_verified(
                    Some(&cache_dir),
                    &record_lockfile_path,
                    lockfile,
                    &verifiers,
                );
                Ok(())
            }
            Err(PnprClientError::Verification(verify_err)) => {
                Err(InstallFrozenLockfileError::LockfileVerification(verify_err))
            }
            Err(err) => Err(InstallFrozenLockfileError::ExternalLockfileVerification(
                err.to_string(),
            )),
        }
    }) as LockfileVerificationOverride<'a>))
}

fn local_verify_options(
    state: &State,
    pnpr_server: &str,
    link: &PnprLink<'_>,
    local: &LocalLockfileInstall<'_>,
) -> VerifyLockfileOptions {
    VerifyLockfileOptions {
        overrides: local.overrides.cloned(),
        lockfile: local.lockfile.clone(),
        trust_lockfile: link.lockfile.trust,
        routing: pnpm_pnpr_client::RegistryRouting {
            registry: local.resolve_registry.to_owned(),
            registries: state.config.registry_declarations(),
            authorization: state.config.auth_headers.for_url(pnpr_server),
        },
        verification: pnpm_pnpr_client::VerificationPolicy {
            minimum_release_age: state.config.minimum_release_age,
            minimum_release_age_exclude: state.config.minimum_release_age_exclude.clone(),
            minimum_release_age_ignore_missing_time: state.config
                .minimum_release_age_ignore_missing_time,
            trust_policy: state.config.trust_policy,
            trust_policy_exclude: state.config.trust_policy_exclude.clone(),
            trust_policy_ignore_after: state.config.trust_policy_ignore_after,
        },
    }
}

fn state_resolution_verifiers(
    state: &State,
) -> miette::Result<Vec<std::sync::Arc<dyn pnpm_resolving_resolver_base::ResolutionVerifier>>> {
    build_resolution_verifiers(
        state.config,
        std::sync::Arc::clone(&state.http_client),
        None,
        None,
        None,
        None,
    )
    .map_err(miette::Report::new)
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
    lockfile = merge_selected_importers(
        state,
        selection,
        session.selection_importer_ids
            .as_ref()
            .or(session.full_workspace_importer_ids.as_ref()),
        session.merge_wanted,
        lockfile,
    )?;
    let merged_repair_verifiers =
        verify_merged_repair::<Reporter>(state, link, session.partial_selection, &lockfile).await?;

    save_pnpr_lockfile(
        state,
        link,
        &lockfile,
        &inputs.lockfile_path,
        merged_repair_verifiers.as_deref(),
    )?;

    Ok(lockfile)
}

mod selection;

fn local_lockfile_install<'a>(
    state: &'a State,
    link: &PnprLink<'a>,
    local: &LocalLockfileInstall<'a>,
) -> pnpm_package_manager::Install<'a, Vec<DependencyGroup>> {
    let mut base_install = state.install(link.dependency_groups.clone());
    base_install.lockfile_policy.frozen = true;
    base_install.lockfile_policy.ignore_manifest_check = link.lockfile.ignore_manifest_check;
    base_install.lockfile_policy.trust = true;
    base_install.execution.skip_runtimes = link.skip_runtimes;
    base_install.execution.node_linker = link.node_linker;
    base_install.execution.lockfile_only = link.lockfile.only;
    base_install.context.lockfile_path = link.lockfile_path;
    base_install.context.lockfile = MaybeLazyLockfile::Loaded(Some(local.lockfile));
    base_install.projects.supported_architectures.clone_from(&link.supported_architectures);
    base_install.projects.pnpmfile_hook_override.clone_from(&local.pnpmfile_hook);
    base_install
}
