mod fast_path;
use fast_path::{UpToDateCheck, install_is_already_up_to_date};

mod dispatch;
use dispatch::{Dispatched, Settled, dispatch};

mod manifests;
use manifests::{HookedManifests, manifest_freshness_inputs, resolve_pnpmfile_hook};

mod wanted;
use wanted::{Lockfiles, settle_wanted_lockfile};

mod lockfile_load;
use lockfile_load::{Loaded, load_lockfiles};

mod workspace;
use workspace::{InstallScope, InstallWorkspace, workspace_projects};

mod execution;

use super::{
    Arc, DependencyGroup, InMemoryPackageMetaCache, IncludedDependencies, Install, InstallError,
    InstallRunOptions, IsTerminal, Lockfile, PackageManifest, Path, PathBuf, Reporter,
    UpdateSeedPolicy, build_resolution_verifiers, lockfile_root_dir,
};
use pnpm_config::Config;
use pnpm_store_dir::VerifiedFileIntegrity;

use crate::{PolicyExcludes, ProjectMutation, catalog_cleanup::post_install_prune};

impl<'a, DependencyGroupList> Install<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    /// Runs the install, then the passes over what it wrote: deleting the
    /// per-branch lockfiles it has just folded into the wanted lockfile,
    /// and pruning the `pnpm-workspace.yaml` exclude entries that lockfile
    /// no longer resolves.
    ///
    /// Both live out here because every success path of
    /// [`Self::run_inner_impl`] — including the short-circuits that do
    /// nothing but rewrite the lockfile — has to run them.
    pub(super) async fn run_inner<Reporter: self::Reporter + 'static>(
        self,
        options: InstallRunOptions<'a, '_>,
    ) -> Result<(), InstallError> {
        // The branch lockfiles become disposable only once the merge has
        // been written for good. An install that neither reads nor saves a
        // lockfile never merged them, and one that only reports what it
        // would do has its lockfile taken back afterwards — deleting them
        // in either case drops resolutions no file is left holding.
        let merge_will_be_saved = self.config.merge_git_branch_lockfiles
            && self.config.lockfile
            && options.save_lockfile
            && !options.lockfile_check
            && !self.dry_run;
        let branch_lockfiles_to_clean = merge_will_be_saved
            .then(|| {
                let manifest_dir =
                    self.manifest.path().parent().expect("manifest path always has a parent dir");
                lockfile_root_dir(self.config, manifest_dir).map_err(InstallError::FindWorkspaceDir)
            })
            .transpose()?;
        let prune_excludes = self.prunes_workspace_excludes(&options);
        let (config, manifest) = (self.config, self.manifest);
        let outcome = Box::pin(self.run_inner_impl::<Reporter>(options)).await?;
        if let Some(lockfile_dir) = branch_lockfiles_to_clean {
            Lockfile::clean_git_branch_lockfiles(&lockfile_dir)
                .map_err(InstallError::CleanGitBranchLockfiles)?;
        }
        if prune_excludes
            && let InstallRunOutcome::LockfileSettled { workspace_manifest_dir } = outcome
        {
            post_install_prune(config, Some(&workspace_manifest_dir), manifest)
                .map_err(InstallError::WriteWorkspaceManifest)?;
        }
        Ok(())
    }

    /// Whether this run owes the [`post_install_prune`] pass over the
    /// `minimumReleaseAgeExclude` / `trustPolicyExclude` lists. `add`,
    /// `update` and `remove` run it themselves once their manifest edits
    /// are persisted; the whole-workspace commands (`install`, `dedupe`)
    /// have only this run to do it. It reads the lockfile back from disk,
    /// so the gates of the branch-lockfile cleanup apply: a run that
    /// leaves the lockfile untouched, has it restored afterwards, or only
    /// reports gives it nothing new to see.
    fn prunes_workspace_excludes(&self, options: &InstallRunOptions<'_, '_>) -> bool {
        self.policy_excludes == PolicyExcludes::Persist
            && matches!(self.mutation, ProjectMutation::InstallWorkspace)
            && self.config.lockfile
            && options.save_lockfile
            && !options.lockfile_check
            && !self.dry_run
            && (self.config.minimum_release_age_exclude_prune
                || self.config.trust_policy_exclude_prune)
    }

    /// Separate what every phase reads from what one of them consumes.
    fn split(self) -> (InstallView<'a>, InstallOwned) {
        (
            InstallView {
                http_client: self.http_client,
                config: self.config,
                manifest: self.manifest,
                emit_initial_manifest: self.emit_initial_manifest,
                lockfile: self.lockfile,
                lockfile_path: self.lockfile_path,
                frozen_lockfile: self.frozen_lockfile,
                prefer_frozen_lockfile: self.prefer_frozen_lockfile,
                ignore_manifest_check: self.ignore_manifest_check,
                skip_runtimes: self.skip_runtimes,
                trust_lockfile: self.trust_lockfile,
                update_checksums: self.update_checksums,
                mutation: self.mutation,
                installs_only: self.installs_only,
                node_linker: self.node_linker,
                lockfile_only: self.lockfile_only,
                dry_run: self.dry_run,
                policy_excludes: self.policy_excludes,
                disable_optimistic_repeat_install: self.disable_optimistic_repeat_install,
            },
            InstallOwned {
                tarball_mem_cache: self.tarball_mem_cache,
                http_client_arc: self.http_client_arc,
                dependency_groups: self.dependency_groups.into_iter().collect(),
                supported_architectures: self.supported_architectures,
                update_seed_policy: self.update_seed_policy,
                preferred_versions_override: self.preferred_versions_override,
                auth_override: self.auth_override,
                resolution_observer: self.resolution_observer,
                peer_issues_sink: self.peer_issues_sink,
                deps_requiring_build_sink: self.deps_requiring_build_sink,
                catalogs_override: self.catalogs_override,
                pnpmfile_hook_override: self.pnpmfile_hook_override,
                workspace_projects_override: self.workspace_projects_override,
            },
        )
    }

    async fn run_inner_impl<Reporter: self::Reporter + 'static>(
        self,
        options: InstallRunOptions<'a, '_>,
    ) -> Result<InstallRunOutcome, InstallError> {
        let (install, mut owned) = self.split();
        install.http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
        owned.http_client_arc.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
        let mode = RunMode::settle(install, &owned, &options)?;
        let mut workspace = InstallWorkspace::discover::<Reporter>(install, &mut owned, &options)?;
        let loaded_workspace_projects = workspace.loaded_workspace_projects.take();
        Box::pin(
            RunExecution {
                install,
                owned,
                mode,
                workspace,
                options,
                loaded_workspace_projects: loaded_workspace_projects.as_deref(),
            }
            .run::<Reporter>(),
        )
        .await
    }
}

struct RunExecution<'a> {
    install: InstallView<'a>,
    owned: InstallOwned,
    mode: RunMode,
    workspace: InstallWorkspace<'a>,
    options: InstallRunOptions<'a, 'a>,
    loaded_workspace_projects: Option<&'a [pnpm_workspace::Project]>,
}

/// How far [`Install::run_inner_impl`] got, for the passes
/// [`Install::run_inner`] runs over what it wrote.
enum InstallRunOutcome {
    /// The repeat-install fast path found nothing to do; no file changed.
    AlreadyUpToDate,
    /// The run settled the wanted lockfile, rewriting it wherever it had
    /// drifted, so the exclude lists in `pnpm-workspace.yaml` under
    /// `workspace_manifest_dir` may now name versions nothing resolves.
    LockfileSettled { workspace_manifest_dir: PathBuf },
}

#[derive(Clone, Copy)]
pub(super) struct InstallView<'a> {
    pub(super) http_client: &'a super::ThrottledClient,
    pub(super) config: &'static Config,
    pub(super) manifest: &'a PackageManifest,
    pub(super) emit_initial_manifest: bool,
    pub(super) lockfile: super::MaybeLazyLockfile<'a>,
    pub(super) lockfile_path: Option<&'a Path>,
    pub(super) frozen_lockfile: bool,
    pub(super) prefer_frozen_lockfile: Option<bool>,
    pub(super) ignore_manifest_check: bool,
    pub(super) skip_runtimes: bool,
    pub(super) trust_lockfile: bool,
    pub(super) update_checksums: bool,
    pub(super) mutation: crate::ProjectMutation,
    pub(super) installs_only: bool,
    pub(super) node_linker: super::NodeLinker,
    pub(super) lockfile_only: bool,
    pub(super) dry_run: bool,
    pub(super) policy_excludes: PolicyExcludes,
    pub(super) disable_optimistic_repeat_install: bool,
}

/// The install's owned inputs, each consumed by one phase.
struct InstallOwned {
    tarball_mem_cache: Arc<super::MemCache>,
    http_client_arc: Arc<super::ThrottledClient>,
    dependency_groups: Vec<DependencyGroup>,
    supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
    update_seed_policy: UpdateSeedPolicy,
    preferred_versions_override: Option<pnpm_resolving_resolver_base::PreferredVersions>,
    auth_override: Option<Arc<super::AuthHeaders>>,
    resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
    peer_issues_sink: Option<crate::PeerIssuesSink>,
    deps_requiring_build_sink: Option<crate::DepsRequiringBuildSink>,
    catalogs_override: Option<super::Catalogs>,
    pnpmfile_hook_override: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    workspace_projects_override: Option<Vec<pnpm_workspace::Project>>,
}

/// What the run's flags settle into before anything is read from disk.
struct RunMode {
    lockfile_only: bool,
    resolve_only: bool,
    prefer_frozen_lockfile: bool,
    included: IncludedDependencies,
    can_prompt: bool,
    peer_issues_sink_is_none: bool,
    effective_node_version: Option<String>,
    verified_file_integrity_baseline: VerifiedFileIntegrity,
}

impl RunMode {
    fn settle(
        install: InstallView<'_>,
        owned: &InstallOwned,
        options: &InstallRunOptions<'_, '_>,
    ) -> Result<Self, InstallError> {
        // Taken before any fetching so the store-verification figures
        // this install reports are its own — a recursive workspace run
        // and a long-lived embedder (the NAPI addon) both drive several
        // installs through the same process-global tally.
        let verified_file_integrity_baseline = VerifiedFileIntegrity::snapshot();
        // `--lockfile-only` with `lockfile: false` (pnpm's
        // `useLockfile: false`) is a config conflict: the only output the
        // flag produces is the lockfile, and that write is disabled.
        // Fail fast rather than run a resolve that writes nothing.
        reject_lockfile_only_without_lockfile(install.config, install.lockfile_only)?;
        // `enableModulesDir: false` (with the global virtual store off) is
        // "resolve and write the lockfile, materialize nothing" — the same
        // pipeline `--lockfile-only` takes, entered from config. It stays
        // outside the `lockfile: false` conflict above (pnpm accepts that
        // combination and simply writes nothing), and never turns a
        // rebuild — which runs against an already-materialized
        // `node_modules` — into a silent no-op.
        let lockfile_only = effective_lockfile_only(
            install.config,
            install.lockfile_only,
            options.rebuild.as_ref(),
        );
        reject_conflicting_store_config(install.config)?;
        Ok(Self {
            lockfile_only,
            // `--dry-run` resolves but never materializes, so it borrows the
            // lockfile-only plumbing (skip node_modules / `.modules.yaml` /
            // workspace-state) while additionally skipping the lockfile write.
            // Both lockfile-only paths must stop after writing the wanted lockfile:
            // neither may write `.modules.yaml`, the current lockfile, or workspace state.
            // The frozen path returns below; the fresh path returns in `complete_resolve_only`.
            resolve_only: lockfile_only || install.dry_run,
            prefer_frozen_lockfile: install
                .prefer_frozen_lockfile
                .unwrap_or(install.config.prefer_frozen_lockfile),
            // The same set the dependency-graph walker observes, written to
            // `.modules.yaml` as `included`.
            included: super::included_dependencies(&owned.dependency_groups),
            can_prompt: options.prompt_eligibility_override.unwrap_or_else(prompts_are_answerable),
            peer_issues_sink_is_none: owned.peer_issues_sink.is_none(),
            effective_node_version: super::effective_node_version(install.config, install.manifest),
            verified_file_integrity_baseline,
        })
    }
}

// One per-install packument cache shared with both the
// lockfile-verifier (below) and the resolver in
// `install_with_fresh_lockfile` (further down). The
// single instance lets a name the resolver fetched during this
// install short-circuit the verifier's own fetch chain, and
// vice versa.
// Resolution verifiers re-apply `minimumReleaseAge` /
// `trustPolicy='no-downgrade'` (plus the tarball-URL anti-tamper
// check) to every entry in the loaded `pnpm-lock.yaml`. They are
// built here — cheap, no I/O — but the verification fan-out itself
// is dispatched per path below: on the frozen materialization path
// it runs concurrently with the fetch (see [`InstallFrozenLockfile`])
// so the per-entry registry round trips overlap the download;
// every other path (fresh resolve, the lockfile-only / up-to-date
// short-circuits) verifies eagerly via [`verify_lockfile_eagerly`]
// before it proceeds. `trust_lockfile` (the OR of yaml's
// `trustLockfile` and the `--trust-lockfile` CLI flag, resolved in
// [`crate::cli_args::install::InstallArgs::run`]; the opt-out for
// environments that treat the on-disk lockfile as
// already-trusted) or no active resolution policy leaves the list
// empty, making every gate a no-op — fresh local resolution is
// already filtered by the resolver's own per-version gate
// (`minimumReleaseAge` via `ResolveResult::policy_violation`,
// `trustPolicy='no-downgrade'` via the npm resolver's
// `fail_if_trust_downgraded_for_pick`). The list is built whenever
// a policy could apply, independent of whether a lockfile is loaded, so the
// fresh-resolve path can record the freshly written lockfile as
// already-verified (see `record_lockfile_verified` below).
// Shared with `CreateVirtualStore`, which fills it after its
// warm/cold partition so the verifier's age gate can lean on
// this install's own canonical tarball fetches instead of a
// metadata body per entry.
pub(super) struct Verification {
    pub(super) meta_cache: Arc<InMemoryPackageMetaCache>,
    pub(super) planned_canonical_fetches: pnpm_resolving_resolver_base::PlannedCanonicalFetches,
    pub(super) resolution_verifiers: Vec<Arc<dyn super::ResolutionVerifier>>,
    pub(super) derived_lockfile_path: Option<PathBuf>,
}

impl Verification {
    fn set_up(execution: &RunExecution<'_>, has_lockfile: bool) -> Result<Self, InstallError> {
        let install = execution.install;
        let owned = &execution.owned;
        let workspace_root = &execution.workspace.workspace_root;
        let meta_cache = Arc::new(InMemoryPackageMetaCache::default());
        let planned_canonical_fetches =
            pnpm_resolving_resolver_base::PlannedCanonicalFetches::default();
        let resolution_verifiers = install_resolution_verifiers(
            install.config,
            install.trust_lockfile,
            (&owned.http_client_arc, &meta_cache, owned.auth_override.as_ref()),
            &planned_canonical_fetches,
        )?;
        Ok(Self {
            meta_cache,
            planned_canonical_fetches,
            resolution_verifiers,
            derived_lockfile_path: has_lockfile.then(|| {
                install.lockfile_path.map_or_else(
                    || workspace_root.join(install.config.wanted_lockfile_name()),
                    Path::to_path_buf,
                )
            }),
        })
    }
}

/// A prompt only reaches a person on an interactive terminal outside CI.
fn prompts_are_answerable() -> bool {
    !is_ci::cached() && std::io::stdin().is_terminal()
}

/// Resolution verifiers re-apply `minimumReleaseAge` /
/// `trustPolicy='no-downgrade'` (plus the tarball-URL anti-tamper check) to
/// every entry in the loaded `pnpm-lock.yaml`. `trust_lockfile` — the opt-out
/// for environments that treat the on-disk lockfile as already-trusted —
/// leaves the list empty, making every gate a no-op.
fn install_resolution_verifiers(
    config: &Config,
    trust_lockfile: bool,
    clients: (
        &Arc<pnpm_network::ThrottledClient>,
        &Arc<InMemoryPackageMetaCache>,
        Option<&Arc<super::AuthHeaders>>,
    ),
    planned_canonical_fetches: &pnpm_resolving_resolver_base::PlannedCanonicalFetches,
) -> Result<Vec<Arc<dyn super::ResolutionVerifier>>, InstallError> {
    if trust_lockfile {
        return Ok(Vec::new());
    }
    let (http_client_arc, meta_cache, auth_override) = clients;
    build_resolution_verifiers(
        config,
        Arc::clone(http_client_arc),
        Some(Arc::clone(meta_cache) as Arc<dyn pnpm_resolving_npm_resolver::PackageMetaCache>),
        auth_override.cloned(),
        None,
        Some(std::sync::Arc::clone(planned_canonical_fetches)),
    )
    .map_err(InstallError::BuildVerifiers)
}

fn reject_frozen_with_update_checksums(
    update_checksums: bool,
    frozen_lockfile: bool,
) -> Result<(), InstallError> {
    if update_checksums && frozen_lockfile {
        return Err(InstallError::FrozenLockfileWithUpdateChecksums);
    }
    Ok(())
}

/// `--lockfile-only` with `lockfile: false` asks for a lockfile the run is
/// forbidden to write.
fn reject_lockfile_only_without_lockfile(
    config: &Config,
    lockfile_only: bool,
) -> Result<(), InstallError> {
    if lockfile_only && !config.lockfile {
        return Err(InstallError::ConfigConflictLockfileOnlyWithNoLockfile);
    }
    Ok(())
}

/// `enableModulesDir: false` (with the global virtual store off) is "resolve
/// and write the lockfile, materialize nothing" — the same pipeline
/// `--lockfile-only` takes, entered from config. It stays outside the
/// `lockfile: false` conflict (pnpm accepts that combination and simply
/// writes nothing), and never turns a rebuild — which runs against an
/// already-materialized `node_modules` — into a silent no-op.
fn effective_lockfile_only(
    config: &Config,
    lockfile_only: bool,
    rebuild: Option<&crate::RebuildOptions>,
) -> bool {
    lockfile_only
        || (rebuild.is_none() && !config.enable_modules_dir && !config.enable_global_virtual_store)
}

fn reject_conflicting_store_config(config: &Config) -> Result<(), InstallError> {
    if config.frozen_store && config.force {
        return Err(InstallError::ConfigConflictFrozenStoreWithForce);
    }
    if config.virtual_store_only
        && !config.enable_modules_dir
        && !config.enable_global_virtual_store
    {
        return Err(InstallError::ConfigConflictVirtualStoreOnlyWithNoModulesDir);
    }
    Ok(())
}
