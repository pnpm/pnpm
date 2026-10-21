use super::{
    Catalogs, Display, Error, InstallFamilySelection, LocalLockfileInstall, Lockfile,
    MaybeLazyLockfile, PnprBenchmarkRegistryOverride, PnprClient, PnprClientError, PnprLink,
    PnprRequestInputs, PnprTarget, Reporter, ResolveProject, ResolveProjectsOptions, State,
    TarballPrefetcher, WantedLockfileSatisfactionCheck, check_frozen_pnpmfile,
    check_pnpr_applied_settings, full_workspace_importer_ids, install_from_local_lockfile,
    link_pnpr_lockfile, merge_and_save_pnpr_lockfile, pnpr_catalogs,
    pnpr_config_validation::validate_pnpr_config, pnpr_lockfile_dir, pnpr_request_inputs,
    report_merged_lockfile_conflicts, resolve_projects_for_pnpr, resolve_projects_options,
    selection_importer_ids, wanted_lockfile_satisfies_workspace,
};

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
    target: PnprTarget<'_>,
    link: PnprLink<'_>,
) -> miette::Result<()> {
    Box::pin(install_via_pnpr_inner::<Reporter>(state, target, None, link)).await
}

pub(crate) async fn install_selected_via_pnpr<Reporter: self::Reporter + 'static>(
    state: &State,
    target: PnprTarget<'_>,
    selection: &InstallFamilySelection,
    link: PnprLink<'_>,
) -> miette::Result<()> {
    Box::pin(install_via_pnpr_inner::<Reporter>(state, target, Some(selection), link)).await
}

pub(super) async fn install_via_pnpr_inner<Reporter: self::Reporter + 'static>(
    state: &State,
    target: PnprTarget<'_>,
    selection: Option<&InstallFamilySelection>,
    link: PnprLink<'_>,
) -> miette::Result<()> {
    validate_pnpr_config(state.config, link.lockfile.frozen)?;

    let pnpr_server = target.server;
    let lockfile_dir = pnpr_lockfile_dir(state, &link);
    let session = prepare_pnpr_session::<Reporter>(state, selection, &link, lockfile_dir).await?;
    let inputs = pnpr_request_inputs(state, &link, lockfile_dir, target.pnpmfile_hook).await?;

    if (session.satisfied_without_server
        || (link.lockfile.frozen && (selection.is_some() || !link.lockfile.only)))
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
    let PreviousWanted {
        lockfile: previous_wanted,
        had_conflicts,
    } = load_previous_wanted::<Reporter>(state, link, lockfile_dir)?;
    let merge_wanted = merge_source(state, link, previous_wanted)?;

    let selection_importer_ids = selection_importer_ids(state, selection);
    let partial_selection = selection_importer_ids
        .as_ref()
        .is_some_and(|(real_importer_ids, selected_importer_ids)| {
            real_importer_ids != selected_importer_ids
        });
    let projects = resolve_projects_for_pnpr(state, selection, link.use_state_lockfile)?;
    let full_workspace_importer_ids =
        full_workspace_importer_ids(state, selection, link, &projects);

    let catalogs = pnpr_catalogs(state)?;

    // A merged conflict makes the on-disk lockfile a file no install may
    // leave as it found it, and only the server exchange writes it back.
    let satisfied_without_server = !had_conflicts
        && satisfied_without_server(
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
    let fetchers = hook
        .get_custom_fetchers()
        .await
        .map_err(|error| miette::miette!("{error}"))?;
    Ok(!fetchers
        .iter()
        .any(|fetcher| fetcher.has_can_fetch() && fetcher.has_fetch()))
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
struct PnprResolveResult {
    outcome: pnpm_pnpr_client::ResolveOutcome,
    prefetcher: Option<TarballPrefetcher>,
    known_requires_build: Option<std::collections::HashMap<pnpm_lockfile::PackageKey, bool>>,
}

async fn resolve_via_pnpr(
    pnpr_server: &str,
    opts: ResolveProjectsOptions,
    benchmark_registry_override: Option<&PnprBenchmarkRegistryOverride>,
    prefetcher: Option<TarballPrefetcher>,
) -> miette::Result<PnprResolveResult> {
    let client = PnprClient::new(pnpr_server);
    let mut known_requires_build = std::collections::HashMap::new();

    let result = client.resolve_projects_streaming(opts, |pkg| {
        if let Some(requires_build) = pkg.requires_build
            && let Ok(key) = pkg.id.parse::<pnpm_lockfile::PackageKey>()
        {
            known_requires_build.insert(key.without_peer(), requires_build);
        }
        if let Some(prefetcher) = prefetcher.as_ref() {
            let tarball = match benchmark_registry_override {
                Some(registry) => registry.client_tarball_url(&pkg.tarball),
                None => pkg.tarball.clone(),
            };
            prefetcher.prefetch(
                pkg.id,
                tarball,
                &pkg.integrity,
                pkg.unpacked_size,
                pkg.file_count,
                pkg.revision.is_some(),
            );
        }
    })
    .await;
    match result {
        Ok(outcome) => Ok(PnprResolveResult {
            outcome,
            prefetcher,
            known_requires_build: (!known_requires_build.is_empty()).then_some(
                known_requires_build,
            ),
        }),
        Err(err) => Err(map_pnpr_client_error(err)),
    }
}

#[derive(Debug, Display, Error)]
#[display("{message}")]
struct ServerErrorWithCode {
    message: String,
    code: String,
}

impl miette::Diagnostic for ServerErrorWithCode {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new(&self.code))
    }
}

fn map_pnpr_client_error(err: PnprClientError) -> miette::Report {
    match err {
        PnprClientError::Verification(verify_err) => miette::Report::new(verify_err),
        PnprClientError::Server { message, code: Some(code) } => {
            miette::Report::new(ServerErrorWithCode { message, code })
        }
        other => miette::miette!("{other}").wrap_err("resolving dependencies via the pnpr server"),
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
) -> miette::Result<PreviousWanted<'a>> {
    if !link.use_state_lockfile {
        return Ok(PreviousWanted::default());
    }
    let lockfile_source = if link.lockfile.fix {
        MaybeLazyLockfile::Repair(&state.lockfile)
    } else {
        MaybeLazyLockfile::Lazy(&state.lockfile)
    };
    let loaded = lockfile_source.get();
    match loaded {
        Ok(lockfile) => {
            let merged_conflict_files =
                lockfile_source.merged_conflict_files().map_err(miette::Report::new)?;
            report_merged_lockfile_conflicts::<Reporter>(
                merged_conflict_files,
                &lockfile_dir.to_string_lossy(),
            );
            Ok(PreviousWanted { lockfile, had_conflicts: merged_conflict_files > 0 })
        }
        Err(error) if !link.lockfile.frozen => {
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
            Ok(PreviousWanted::default())
        }
        Err(error) => Err(miette::Report::new(error).wrap_err("load the lockfile")),
    }
}

/// The lockfile this install starts from, and whether reading it meant
/// merging Git conflict markers away. The markers are still in the file,
/// so a run that reused this lockfile unchanged would leave them there.
#[derive(Default)]
struct PreviousWanted<'a> {
    lockfile: Option<&'a Lockfile>,
    had_conflicts: bool,
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
    if !(link.lockfile.fix && link.use_state_lockfile) {
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
    let exchange_free = !link.lockfile.frozen
        && !link.lockfile.update_patches
        && !link.lockfile.fix
        && !link.lockfile.only
        && link.lockfile.prefer_frozen
        && !partial_selection;
    let Some(lockfile) = previous_wanted.filter(|_| exchange_free) else {
        return false;
    };
    wanted_lockfile_satisfies_workspace(&WantedLockfileSatisfactionCheck {
        config: state.config,
        manifest: &state.manifest,
        catalogs: &catalogs.cloned().unwrap_or_default(),
        lockfile,
        ignore_manifest_check: link.lockfile.ignore_manifest_check,
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
    if link.lockfile.frozen
        && let Some(previous_wanted) = session.previous_wanted
    {
        check_frozen_pnpmfile(state, inputs.pnpmfile_hook.as_ref(), previous_wanted).await?;
    }
    let prefetcher = streaming_prefetcher(state, lockfile_dir, &link, &session, &inputs).await;
    let opts = resolve_projects_options(state, pnpr_server, &link, &mut session, &mut inputs);
    let resolved = resolve_via_pnpr(
        pnpr_server,
        opts,
        inputs.benchmark_registry_override.as_ref(),
        prefetcher,
    )
    .await?;
    check_pnpr_applied_settings(state.config, resolved.outcome.lockfile.settings.as_ref())?;
    let lockfile = merge_and_save_pnpr_lockfile::<Reporter>(
        state,
        selection,
        &link,
        &session,
        &inputs,
        resolved.outcome.lockfile,
    )
    .await?;
    if !link.lockfile.only {
        link_pnpr_lockfile::<Reporter>(
            state,
            selection,
            link,
            &lockfile,
            resolved.known_requires_build,
            inputs.pnpmfile_hook,
        )
        .await?;
        // The materialization install has awaited every tarball's mem-cache
        // slot, so all prefetch downloads have finished and queued their
        // store-index rows. Drain the writer so those rows are persisted for
        // the next install before returning.
        if let Some(prefetcher) = resolved.prefetcher {
            prefetcher.shutdown().await;
        }
    }
    Ok(())
}

async fn streaming_prefetcher(
    state: &State,
    lockfile_dir: &std::path::Path,
    link: &PnprLink<'_>,
    session: &PnprSession<'_>,
    inputs: &PnprRequestInputs,
) -> Option<TarballPrefetcher> {
    if link.lockfile.only || session.partial_selection || !inputs.prefetch_allowed {
        None
    } else {
        Some(
            TarballPrefetcher::new(
                state.config,
                &state.http_client,
                &state.tarball_mem_cache,
                None,
                &lockfile_dir.to_string_lossy(),
                None,
            )
            .await,
        )
    }
}
