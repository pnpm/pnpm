use super::{
    Catalogs, Context, DependencyGroup, Diagnostic, Display, Error, InstallFamilySelection,
    LocalLockfileInstall, Lockfile, MaybeLazyLockfile, PnprBenchmarkRegistryOverride, PnprClient,
    PnprClientError, PnprLink, PnprRequestInputs, Reporter, ResolveProject, ResolveProjectsOptions,
    State, TarballPrefetcher, WantedLockfileSatisfactionCheck, full_workspace_importer_ids,
    install_from_local_lockfile, link_pnpr_lockfile, merge_and_save_pnpr_lockfile, pnpr_catalogs,
    pnpr_lockfile_dir, pnpr_request_inputs, resolve_projects_for_pnpr, resolve_projects_options,
    selection_importer_ids, wanted_lockfile_satisfies_workspace,
};

/// `frozenStore` was enabled together with a configured `pnprServer`.
/// The pnpr path writes resolved files into the store, which `frozenStore`
/// opens read-only, so the combination can't proceed. Mirrors pnpm's
/// `ERR_PNPM_FROZEN_STORE_INCOMPATIBLE_WITH_PNPR`.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "The pnpr server resolves dependencies and writes new entries into the store, which is opened read-only when frozenStore is enabled."
)]
#[diagnostic(
    code(ERR_PNPM_FROZEN_STORE_INCOMPATIBLE_WITH_PNPR),
    help(
        "Disable the pnpr server (unset `--pnpr-server` / `pnprServer` in pnpm-workspace.yaml) so the install reads from the existing store, or unset `frozenStore` to allow store writes."
    )
)]
struct FrozenStoreIncompatibleWithPnpr;

/// `--dry-run` was requested with a configured `pnprServer`. The pnpr path
/// resolves and links through the server, so it can't honor the dry-run
/// "writes nothing" contract. Mirrors pnpm's
/// `ERR_PNPM_CONFIG_CONFLICT_DRY_RUN_WITH_PNPR_SERVER`.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "Cannot use --dry-run with a configured pnpr server because the pnpr install path resolves and links through the server."
)]
#[diagnostic(
    code(ERR_PNPM_CONFIG_CONFLICT_DRY_RUN_WITH_PNPR_SERVER),
    help(
        "Unset the pnpr server (`--pnpr-server` / `pnprServer` in pnpm-workspace.yaml) to preview locally, or drop --dry-run."
    )
)]
pub(super) struct DryRunIncompatibleWithPnpr;

pub(super) fn resolve_project(
    dir: String,
    manifest: &pnpm_package_manifest::PackageManifest,
) -> ResolveProject {
    ResolveProject {
        dir,
        name: manifest.value().get("name").and_then(|value| value.as_str()).map(str::to_string),
        version: manifest
            .value()
            .get("version")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        dependencies: manifest
            .dependencies([DependencyGroup::Prod])
            .map(|(name, spec)| (name.to_string(), spec.to_string()))
            .collect(),
        dev_dependencies: manifest
            .dependencies([DependencyGroup::Dev])
            .map(|(name, spec)| (name.to_string(), spec.to_string()))
            .collect(),
        optional_dependencies: manifest
            .dependencies([DependencyGroup::Optional])
            .map(|(name, spec)| (name.to_string(), spec.to_string()))
            .collect(),
    }
}

/// Resolve the active project or selected workspace projects through a
/// `pnpr` server, then link them.
///
/// Sends the client's registries to the server, which resolves against
/// them and returns the resolved lockfile; writes that lockfile, then
/// runs a frozen install to materialize `node_modules` from it — the
/// frozen install fetches every tarball from the registries itself, like
/// a normal install. Under `--lockfile-only` it stops after writing the
/// lockfile (fetch nothing, link nothing).
///
/// The server is only contacted when there is resolution or
/// verification work for it: an install whose on-disk lockfile still
/// satisfies every manifest skips the resolve exchange and goes
/// straight to the frozen materialization, and the input-lockfile
/// verification round trip is skipped when the local
/// `lockfile-verified.jsonl` cache already covers the lockfile under
/// the current policy
/// ([pnpm/pnpm#13904](https://github.com/pnpm/pnpm/issues/13904)).
pub(crate) async fn install_via_pnpr<Reporter: self::Reporter + 'static>(
    state: &State,
    pnpr_server: &str,
    link: PnprLink<'_>,
) -> miette::Result<()> {
    Box::pin(install_via_pnpr_inner::<Reporter>(state, pnpr_server, None, link)).await
}

pub(crate) async fn install_selected_via_pnpr<Reporter: self::Reporter + 'static>(
    state: &State,
    pnpr_server: &str,
    selection: &InstallFamilySelection,
    link: PnprLink<'_>,
) -> miette::Result<()> {
    Box::pin(install_via_pnpr_inner::<Reporter>(state, pnpr_server, Some(selection), link)).await
}

pub(super) async fn install_via_pnpr_inner<Reporter: self::Reporter + 'static>(
    state: &State,
    pnpr_server: &str,
    selection: Option<&InstallFamilySelection>,
    link: PnprLink<'_>,
) -> miette::Result<()> {
    // The pnpr server resolves dependencies and streams missing files
    // straight into the store, so this path inherently writes the store.
    // `frozenStore` promises the store is complete and read-only, so the
    // two are mutually exclusive — refuse up front instead of failing on
    // the read-only write with the `FROZEN_STORE_INCOMPATIBLE_WITH_PNPR`
    // guard.
    if state.config.frozen_store {
        return Err(FrozenStoreIncompatibleWithPnpr.into());
    }

    let lockfile_dir = pnpr_lockfile_dir(state, &link);
    let session = prepare_pnpr_session::<Reporter>(state, selection, &link, lockfile_dir).await?;
    let inputs = pnpr_request_inputs(state, &link, lockfile_dir).await?;

    if (session.satisfied_without_server
        || (link.frozen_lockfile && (selection.is_some() || !link.lockfile_only)))
        && let Some(lockfile) = session.previous_wanted
    {
        return install_from_local_lockfile::<Reporter>(
            state,
            pnpr_server,
            selection,
            &link,
            &LocalLockfileInstall {
                lockfile,
                lockfile_dir,
                lockfile_path: &inputs.lockfile_path,
                selection_importer_ids: session.selection_importer_ids.as_ref(),
                overrides: inputs.overrides.as_ref(),
                resolve_registry: &inputs.resolve_registry,
                prefetch_allowed: inputs.prefetch_allowed,
                pnpmfile_hook: inputs.pnpmfile_hook,
            },
        )
        .await;
    }

    resolve_and_link_pnpr::<Reporter>(
        state,
        pnpr_server,
        selection,
        link,
        lockfile_dir,
        session,
        inputs,
    )
    .await
}

/// What the pnpr path knows about the workspace before deciding whether the
/// server has to resolve anything.
pub(super) struct PnprSession<'a> {
    pub(super) previous_wanted: Option<&'a Lockfile>,
    pub(super) merge_wanted: Option<&'a Lockfile>,
    pub(super) selection_importer_ids:
        Option<(std::collections::HashSet<String>, std::collections::HashSet<String>)>,
    pub(super) partial_selection: bool,
    pub(super) projects: Vec<ResolveProject>,
    pub(super) full_workspace_importer_ids:
        Option<(std::collections::HashSet<String>, std::collections::HashSet<String>)>,
    pub(super) catalogs: Option<Catalogs>,
    pub(super) satisfied_without_server: bool,
}

async fn prepare_pnpr_session<'a, Reporter: self::Reporter + 'static>(
    state: &'a State,
    selection: Option<&InstallFamilySelection>,
    link: &PnprLink<'_>,
    lockfile_dir: &std::path::Path,
) -> miette::Result<PnprSession<'a>> {
    let previous_wanted = load_previous_wanted::<Reporter>(state, link, lockfile_dir)?;
    let merge_wanted = merge_source(state, link, previous_wanted)?;

    let selection_importer_ids = selection_importer_ids(state, selection);
    let partial_selection = selection_importer_ids.as_ref().is_some_and(
        |(real_importer_ids, selected_importer_ids)| real_importer_ids != selected_importer_ids,
    );
    let projects = resolve_projects_for_pnpr(state, selection, link.use_state_lockfile)?;
    let full_workspace_importer_ids =
        full_workspace_importer_ids(state, selection, link, &projects);

    let catalogs = pnpr_catalogs(state)?;

    let satisfied_without_server = satisfied_without_server(
        state,
        link,
        previous_wanted,
        catalogs.as_ref(),
        partial_selection,
    )
    .await;
    Ok(PnprSession {
        previous_wanted,
        merge_wanted,
        selection_importer_ids,
        partial_selection,
        projects,
        full_workspace_importer_ids,
        catalogs,
        satisfied_without_server,
    })
}

/// Whether the tarball prefetcher may run: a custom fetcher owns the
/// fetch for the packages it claims, so prefetching would race it.
pub(super) async fn prefetch_allowed(
    pnpmfile_hook: Option<&std::sync::Arc<dyn pnpm_hooks::PnpmfileHooks>>,
) -> miette::Result<bool> {
    let Some(hook) = pnpmfile_hook else {
        return Ok(true);
    };
    let fetchers = hook.get_custom_fetchers().await.map_err(|error| miette::miette!("{error}"))?;
    Ok(!fetchers.iter().any(|fetcher| fetcher.has_can_fetch() && fetcher.has_fetch()))
}

/// Whether the resolve streams its packages into a prefetcher, and what
/// the prefetcher needs to start.
struct ResolveStreaming<'a> {
    lockfile_dir: &'a std::path::Path,
    lockfile_only: bool,
    partial_selection: bool,
    prefetch_allowed: bool,
}

/// Ask the pnpr server to resolve the projects, streaming each resolved
/// package into a tarball prefetcher as its frame arrives so the fetch
/// overlaps the server's resolution
/// ([pnpm/pnpm#12234](https://github.com/pnpm/pnpm/issues/12234)); the
/// frozen materialization install then finds every tarball already in the
/// shared mem cache.
///
/// Under `--lockfile-only` nothing is materialized, so the stream is
/// consumed with a no-op callback. A partial install also waits for the
/// merged lockfile before fetching, because only then is the selected
/// workspace closure known.
async fn resolve_via_pnpr(
    state: &State,
    pnpr_server: &str,
    opts: ResolveProjectsOptions,
    benchmark_registry_override: Option<&PnprBenchmarkRegistryOverride>,
    streaming: ResolveStreaming<'_>,
) -> miette::Result<(pnpm_pnpr_client::ResolveOutcome, Option<TarballPrefetcher>)> {
    let client = PnprClient::new(pnpr_server);
    let prefetcher = streaming_prefetcher(state, &streaming).await;

    let result = match prefetcher.as_ref() {
        Some(prefetcher) => {
            client
                .resolve_projects_streaming(opts, |pkg| {
                    let tarball = benchmark_registry_override.map_or_else(
                        || pkg.tarball.clone(),
                        |registry| registry.client_tarball_url(&pkg.tarball),
                    );
                    prefetcher.prefetch(
                        pkg.id,
                        tarball,
                        &pkg.integrity,
                        pkg.unpacked_size,
                        pkg.file_count,
                        pkg.revision.is_some(),
                    );
                })
                .await
        }
        None => client.resolve_projects(opts).await,
    };
    match result {
        Ok(outcome) => Ok((outcome, prefetcher)),
        // The server rejected the input lockfile under our policy.
        // Surface the reconstructed `VerifyError` so the abort + the
        // `ERR_PNPM_*` diagnostic code match the local gate exactly.
        Err(PnprClientError::Verification(verify_err)) => Err(miette::Report::new(verify_err)),
        Err(err) => {
            Err(miette::miette!("{err}")).wrap_err("resolving dependencies via the pnpr server")
        }
    }
}

/// The lockfile this install starts from, borrowed from the shared
/// state rather than cloned: the frozen branch never needs an owned
/// lockfile, so the exchange-free paths pay no deep copy. The
/// server-exchange paths clone at the point a request body needs one.
///
/// A broken lockfile is a hard error only for a frozen install; any
/// other install reports it and resolves from scratch.
fn load_previous_wanted<'a, Reporter: self::Reporter + 'static>(
    state: &'a State,
    link: &PnprLink<'_>,
    lockfile_dir: &std::path::Path,
) -> miette::Result<Option<&'a Lockfile>> {
    if !link.use_state_lockfile {
        return Ok(None);
    }
    let loaded =
        if link.fix_lockfile { state.lockfile.get_for_fix() } else { state.lockfile.get() };
    match loaded {
        Ok(lockfile) => Ok(lockfile),
        Err(error) if !link.frozen_lockfile => {
            <Reporter as pnpm_reporter::Reporter>::emit(&pnpm_reporter::LogEvent::Pnpm(
                pnpm_reporter::PnpmLog {
                    level: pnpm_reporter::LogLevel::Warn,
                    message: format!(
                        "Ignoring broken lockfile at {}: {error}",
                        lockfile_dir.display(),
                    ),
                    prefix: lockfile_dir.to_string_lossy().into_owned(),
                },
            ));
            Ok(None)
        }
        Err(error) => Err(miette::Report::new(error).wrap_err("load the lockfile")),
    }
}

/// The lockfile the filtered merge below reuses entries from. A
/// `--fix-lockfile` run merges against the repaired read, which drops the
/// fields the repair regenerates.
fn merge_source<'a>(
    state: &'a State,
    link: &PnprLink<'_>,
    previous_wanted: Option<&'a Lockfile>,
) -> miette::Result<Option<&'a Lockfile>> {
    if previous_wanted.is_none() {
        return Ok(None);
    }
    if !(link.fix_lockfile && link.use_state_lockfile) {
        return Ok(previous_wanted);
    }
    MaybeLazyLockfile::Repair(&state.lockfile)
        .get_for_merge()
        .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile for filtered merge"))
}

/// Whether the on-disk lockfile already describes this install, so the
/// server exchange can be skipped.
///
/// Filtered installs keep the exchange even when satisfied: their merge
/// semantics live there. A workspace-wide install has a selection too —
/// every project — and skips the server like any single-project one.
async fn satisfied_without_server(
    state: &State,
    link: &PnprLink<'_>,
    previous_wanted: Option<&Lockfile>,
    catalogs: Option<&Catalogs>,
    partial_selection: bool,
) -> bool {
    let exchange_free = !link.frozen_lockfile
        && !link.update_patches
        && !link.fix_lockfile
        && !link.lockfile_only
        && link.prefer_frozen_lockfile
        && !partial_selection;
    let Some(lockfile) = previous_wanted.filter(|_| exchange_free) else {
        return false;
    };
    wanted_lockfile_satisfies_workspace(&WantedLockfileSatisfactionCheck {
        config: state.config,
        manifest: &state.manifest,
        catalogs: &catalogs.cloned().unwrap_or_default(),
        lockfile,
        ignore_manifest_check: link.ignore_manifest_check,
    })
    .await
}

async fn resolve_and_link_pnpr<Reporter: self::Reporter + 'static>(
    state: &State,
    pnpr_server: &str,
    selection: Option<&InstallFamilySelection>,
    link: PnprLink<'_>,
    lockfile_dir: &std::path::Path,
    mut session: PnprSession<'_>,
    mut inputs: PnprRequestInputs,
) -> miette::Result<()> {
    let opts = resolve_projects_options(state, pnpr_server, &link, &mut session, &mut inputs);
    let (mut outcome, prefetcher) = resolve_via_pnpr(
        state,
        pnpr_server,
        opts,
        inputs.benchmark_registry_override.as_ref(),
        ResolveStreaming {
            lockfile_dir,
            lockfile_only: link.lockfile_only,
            partial_selection: session.partial_selection,
            prefetch_allowed: inputs.prefetch_allowed,
        },
    )
    .await?;

    outcome.lockfile = merge_and_save_pnpr_lockfile::<Reporter>(
        state,
        selection,
        &link,
        &session,
        &inputs,
        outcome.lockfile,
    )
    .await?;

    // `--lockfile-only`: the server resolved and returned the lockfile
    // but fetched nothing; pnpm links nothing in this mode, so stop after
    // writing the lockfile rather than running the materialization pass.
    // See [pnpm/pnpm#12146](https://github.com/pnpm/pnpm/issues/12146).
    if link.lockfile_only {
        return Ok(());
    }

    link_pnpr_lockfile::<Reporter>(state, selection, link, &outcome.lockfile, inputs.pnpmfile_hook)
        .await?;

    // The materialization install has awaited every tarball's mem-cache
    // slot, so all prefetch downloads have finished and queued their
    // store-index rows. Drain the writer so those rows are persisted for
    // the next install before returning.
    if let Some(prefetcher) = prefetcher {
        prefetcher.shutdown().await;
    }

    Ok(())
}

async fn streaming_prefetcher(
    state: &State,
    streaming: &ResolveStreaming<'_>,
) -> Option<TarballPrefetcher> {
    if streaming.lockfile_only || streaming.partial_selection || !streaming.prefetch_allowed {
        None
    } else {
        Some(
            TarballPrefetcher::new(
                state.config,
                &state.http_client,
                &state.tarball_mem_cache,
                None,
                &streaming.lockfile_dir.to_string_lossy(),
            )
            .await,
        )
    }
}
