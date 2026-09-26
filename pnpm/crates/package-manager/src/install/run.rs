mod fast_path;
pub(super) use fast_path::register_workspace_in_store;
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
mod mode;
use mode::{RunMode, WorkspaceManifestRollbackGuard};
mod frozen_local_tarballs;
mod time_machine_capture;
mod uninstall_hooks;
use uninstall_hooks::run_pre_uninstall_hooks;

use std::fs;

use super::{
    Arc, DependencyGroup, InMemoryPackageMetaCache, Install, InstallError, InstallRunOptions,
    Lockfile, Path, PathBuf, Reporter, UpdateSeedPolicy, build_resolution_verifiers,
    configured_or_discovered_workspace_dir, lockfile_root_dir,
};
use pnpm_config::Config;

use crate::{
    PolicyExcludes, ProjectMutation,
    catalog_cleanup::{
        post_install_prune, write_workspace_catalogs, write_workspace_catalogs_selected,
    },
};

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
        let store_lock = if self.context.config.frozen_store {
            self.context.config.store_dir.lock_for_frozen_use()
        } else {
            self.context.config.store_dir.lock_for_use()
        }
        .map_err(InstallError::StoreLock)?;
        let mut time_machine_exclusions = super::TimeMachineExclusions::empty();
        // The branch lockfiles become disposable only once the merge has
        // been written for good. An install that neither reads nor saves a
        // lockfile never merged them, and one that only reports what it
        // would do has its lockfile taken back afterwards — deleting them
        // in either case drops resolutions no file is left holding.
        let merge_will_be_saved = self.context.config.merge_git_branch_lockfiles
            && self.context.config.lockfile
            && options.save.lockfile
            && !options.lockfile_check
            && !self.execution.dry_run;
        let branch_lockfiles_to_clean = merge_will_be_saved
            .then(|| {
                let manifest_dir = self.context.manifest
                    .path()
                    .parent()
                    .expect("manifest path always has a parent dir");
                lockfile_root_dir(self.context.config, manifest_dir)
                    .map_err(InstallError::FindWorkspaceDir)
            })
            .transpose()?;
        let prune_excludes = self.prunes_workspace_excludes(&options);
        let result = Box::pin(self.run_inner_and_cleanup::<Reporter>(
            options,
            branch_lockfiles_to_clean,
            prune_excludes,
            &mut time_machine_exclusions,
        ))
        .await;
        drop(store_lock);
        time_machine_exclusions.apply::<Reporter>().await;
        result
    }

    async fn run_inner_and_cleanup<Reporter: self::Reporter + 'static>(
        self,
        options: InstallRunOptions<'a, '_>,
        branch_lockfiles_to_clean: Option<PathBuf>,
        prune_excludes: bool,
        time_machine_exclusions: &mut super::TimeMachineExclusions,
    ) -> Result<(), InstallError> {
        let (config, manifest) = (self.context.config, self.context.manifest);
        let outcome =
            Box::pin(self.run_inner_impl::<Reporter>(options, time_machine_exclusions)).await?;
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
        self.lockfile_policy.excludes == PolicyExcludes::Persist
            && matches!(self.execution.mutation, ProjectMutation::InstallWorkspace)
            && self.context.config.lockfile
            && options.save.lockfile
            && !options.lockfile_check
            && !self.execution.dry_run
            && (self.context.config.minimum_release_age_exclude_prune
                || self.context.config.trust_policy_exclude_prune)
    }

    /// Separate what every phase reads from what one of them consumes.
    fn split(self) -> (InstallView<'a>, InstallOwned) {
        (
            InstallView {
                context: self.context,
                lockfile_policy: self.lockfile_policy,
                execution: self.execution,
            },
            InstallOwned {
                tarball_mem_cache: self.fetching.tarball_mem_cache,
                http_client_arc: self.fetching.http_client_arc,
                projects: super::InstallProjects {
                    dependency_groups: self.projects.dependency_groups.into_iter().collect(),
                    supported_architectures: self.projects.supported_architectures,
                    catalogs_override: self.projects.catalogs_override,
                    pnpmfile_hook_override: self.projects.pnpmfile_hook_override,
                    workspace_projects_override: self.projects.workspace_projects_override,
                },
                resolution: self.resolution,
            },
        )
    }

    async fn run_inner_impl<Reporter: self::Reporter + 'static>(
        self,
        options: InstallRunOptions<'a, '_>,
        time_machine_exclusions: &mut super::TimeMachineExclusions,
    ) -> Result<InstallRunOutcome, InstallError> {
        let (install, mut owned) = self.split();
        install.context.http_client.set_warning_handler(
            pnpm_reporter::emit_global_warning::<Reporter>,
        );
        owned.http_client_arc.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
        let mode = RunMode::settle(install, &owned, &options)?;
        owned.projects.dependency_groups =
            super::project_dependency_groups(std::mem::take(&mut owned.projects.dependency_groups));
        let rollback_guard = if install.should_prune_catalogs(&options) {
            install.prune_workspace_catalogs(
                &options,
                owned.projects.workspace_projects_override.as_deref(),
            )?
        } else {
            None
        };
        let mut workspace = InstallWorkspace::discover::<Reporter>(install, &mut owned, &options)?;
        let loaded_workspace_projects = workspace.loaded_workspace_projects.take();
        let outcome = Box::pin(
            RunExecution {
                install,
                owned,
                mode,
                workspace,
                options,
                loaded_workspace_projects: loaded_workspace_projects.as_deref(),
            }
            .run::<Reporter>(time_machine_exclusions),
        )
        .await?;
        if let Some(guard) = rollback_guard {
            guard.commit();
        }
        Ok(outcome)
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
    pub(super) context: super::InstallInvocation<'a>,
    pub(super) lockfile_policy: InstallLockfilePolicy,
    pub(super) execution: InstallExecution,
}

impl InstallView<'_> {
    fn should_prune_catalogs(&self, options: &InstallRunOptions<'_, '_>) -> bool {
        self.context.config.catalog_prune
            && options.save.lockfile
            && !options.lockfile_check
            && !self.execution.dry_run
            && self.execution.mutation.is_full_install()
    }

    fn prune_workspace_catalogs(
        &self,
        options: &InstallRunOptions<'_, '_>,
        workspace_projects_override: Option<&[pnpm_workspace::Project]>,
    ) -> Result<Option<WorkspaceManifestRollbackGuard>, InstallError> {
        let manifest_dir = self.context.manifest
            .path()
            .parent()
            .expect("manifest path always has a parent dir");
        let Some(workspace_dir) =
            configured_or_discovered_workspace_dir(self.context.config, manifest_dir)
                .map_err(InstallError::FindWorkspaceDir)?
        else {
            return Ok(None);
        };
        let workspace_manifest_path = workspace_dir.join("pnpm-workspace.yaml");
        let original_content = fs::read_to_string(&workspace_manifest_path).ok();
        let selected_projects = workspace_projects_override.or_else(|| {
            options.selection.as_ref().map(|s| s.all_projects)
        });
        write_pruned_catalogs(
            self.context.config,
            self.context.manifest,
            &workspace_dir,
            selected_projects,
        )?;
        Ok(WorkspaceManifestRollbackGuard::new(workspace_manifest_path, original_content))
    }
}

fn write_pruned_catalogs(
    config: &pnpm_config::Config,
    manifest: &pnpm_package_manifest::PackageManifest,
    workspace_dir: &Path,
    selected_projects: Option<&[pnpm_workspace::Project]>,
) -> Result<(), InstallError> {
    if let Some(projects) = selected_projects {
        write_workspace_catalogs_selected(
            config,
            workspace_dir,
            &pnpm_catalogs_types::Catalogs::new(),
            projects,
        )
    } else {
        write_workspace_catalogs(
            config,
            Some(workspace_dir),
            &pnpm_catalogs_types::Catalogs::new(),
            manifest,
        )
    }
    .map_err(InstallError::WriteWorkspaceManifest)
}

#[derive(Clone, Copy)]
pub struct InstallLockfilePolicy {
    pub frozen: bool,
    pub prefer_frozen: Option<bool>,
    pub ignore_manifest_check: bool,
    pub trust: bool,
    pub update_checksums: bool,
    pub excludes: PolicyExcludes,
    /// Turns off both repeat-install short-circuits: the workspace-state
    /// check before any install setup, and the "nothing to materialize"
    /// return once the lockfile has been verified against the manifests.
    pub disable_optimistic_repeat: bool,
    /// How the workspace-state check learns whether a project manifest
    /// changed: from its `package.json` mtime, or by content when the
    /// caller supplied the manifests in memory.
    pub manifest_freshness: crate::ManifestFreshness,
}

impl InstallLockfilePolicy {
    /// The starting point a plain `install` and the install a `remove` runs
    /// share; the specialized entry points override what they change.
    #[must_use]
    pub fn plain(config: &Config) -> Self {
        Self {
            frozen: false,
            prefer_frozen: None,
            ignore_manifest_check: false,
            trust: config.trust_lockfile,
            update_checksums: false,
            excludes: PolicyExcludes::Skip,
            disable_optimistic_repeat: false,
            manifest_freshness: crate::ManifestFreshness::Mtime,
        }
    }
}

#[derive(Clone, Copy)]
pub struct InstallExecution {
    pub skip_runtimes: bool,
    pub mutation: crate::ProjectMutation,
    pub installs_only: bool,
    pub node_linker: super::NodeLinker,
    pub lockfile_only: bool,
    pub dry_run: bool,
}

/// The install's owned inputs, each consumed by one phase.
struct InstallOwned {
    tarball_mem_cache: Arc<super::MemCache>,
    http_client_arc: Arc<super::ThrottledClient>,
    projects: super::InstallProjects<Vec<DependencyGroup>>,
    resolution: crate::install::run::ResolutionInputs,
}

#[derive(Default)]
pub struct ResolutionInputs {
    pub update_seed_policy: UpdateSeedPolicy,
    pub preferred_versions_override: Option<pnpm_resolving_resolver_base::PreferredVersions>,
    pub auth_override: Option<Arc<super::AuthHeaders>>,
    pub observer: Option<Arc<dyn crate::ResolutionObserver>>,
    pub peer_issues_sink: Option<crate::PeerIssuesSink>,
    pub deps_requiring_build_sink: Option<crate::DepsRequiringBuildSink>,
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
        let workspace_root = &execution.workspace.dirs.workspace_root;
        let meta_cache = Arc::new(InMemoryPackageMetaCache::default());
        let planned_canonical_fetches =
            pnpm_resolving_resolver_base::PlannedCanonicalFetches::default();
        let resolution_verifiers = install_resolution_verifiers(
            install.context.config,
            install.lockfile_policy.trust,
            (&owned.http_client_arc, &meta_cache, owned.resolution.auth_override.as_ref()),
            &planned_canonical_fetches,
        )?;
        Ok(Self {
            meta_cache,
            planned_canonical_fetches,
            resolution_verifiers,
            derived_lockfile_path: has_lockfile.then(|| {
                install.context.lockfile_path.map_or_else(
                    || workspace_root.join(install.context.config.wanted_lockfile_name()),
                    Path::to_path_buf,
                )
            }),
        })
    }
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
