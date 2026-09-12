pub(crate) use pnpr_resolution::{install_selected_via_pnpr, install_via_pnpr};

use crate::{
    State,
    cli_args::{
        legacy_pnpm_field::warn_ignored_pnpm_manifest_fields, lockfile_dir::LockfileDirArg,
        override_version_references::warn_deprecated_override_version_references,
        package_manager::read_root_manifest_json, pipelines::InstallFamilySelection,
        recursive::discover_workspace_projects,
        supported_architectures::SupportedArchitecturesArgs,
        yarn_workspaces_field::warn_unsupported_workspaces_field,
    },
};
use clap::{Args, ValueEnum};
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_catalogs_config::get_catalogs_from_workspace_manifest;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::NodeLinker;
use pnpm_lockfile::{Lockfile, LockfileResolution, MaybeLazyLockfile};
use pnpm_lockfile_verification::{
    VerifyLockfileResolutionsOptions, lockfile_verification_is_cached,
    lockfile_verification_is_cached_by_content, record_lockfile_verified,
    verify_lockfile_resolutions,
};
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_package_manager::{
    Install, InstallFrozenLockfileError, LockfileVerificationOverride, PolicyExcludes,
    SkippedSnapshots, TarballPrefetcher, UpToDateFastPathCheck, UpdateSeedPolicy,
    WantedLockfileSatisfactionCheck, WorkspaceInstallSelection, build_resolution_verifiers,
    install_already_up_to_date, materialization_closure, merge_filtered_wanted_lockfile,
    wanted_lockfile_satisfies_workspace,
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_pnpr_client::{
    PnprClient, PnprClientError, ResolveProject, ResolveProjectsOptions, VerifyLockfileOptions,
};
use pnpm_reporter::Reporter;
use pnpr_lockfile::{
    LocalLockfileInstall, full_workspace_importer_ids, install_from_local_lockfile,
    link_pnpr_lockfile, merge_and_save_pnpr_lockfile, pnpr_lockfile_dir, selection_importer_ids,
};
use pnpr_request::{
    PnprBenchmarkRegistryOverride, PnprRequestInputs, pnpr_catalogs, pnpr_request_inputs,
    resolve_projects_for_pnpr, resolve_projects_options,
};
use pnpr_resolution::{
    DryRunIncompatibleWithPnpr, PnprSession, install_via_pnpr_inner, prefetch_allowed,
    resolve_project,
};

use std::path::PathBuf;

/// `--node-linker` value parser. CLI mirror of
/// [`pnpm_config::NodeLinker`] so the config crate stays free
/// of `clap` as a dependency. Converted to the canonical enum at
/// the CLI/Install boundary via [`Self::into_config`].
#[derive(Debug, Clone, Copy, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum NodeLinkerArg {
    Isolated,
    Hoisted,
    Pnp,
}

impl NodeLinkerArg {
    #[inline]
    pub(crate) fn into_config(self) -> NodeLinker {
        match self {
            NodeLinkerArg::Isolated => NodeLinker::Isolated,
            NodeLinkerArg::Hoisted => NodeLinker::Hoisted,
            NodeLinkerArg::Pnp => NodeLinker::Pnp,
        }
    }
}

#[derive(Debug, Default, Clone, Args)]
pub struct InstallDependencyOptions {
    /// Install only production dependencies. devDependencies are skipped,
    /// and removed if already installed.
    #[arg(short = 'P', long, visible_alias = "production")]
    prod: bool,
    /// Install only devDependencies. Regular dependencies are skipped, and
    /// removed if already installed.
    #[arg(short = 'D', long)]
    dev: bool,
    /// Include optionalDependencies even when the configured default excludes them.
    #[arg(long, overrides_with = "no_optional")]
    optional: bool,
    /// Don't install optionalDependencies.
    #[arg(long, overrides_with = "optional")]
    no_optional: bool,
}

impl InstallDependencyOptions {
    /// Convert the dependency options to an iterator of [`DependencyGroup`]
    /// which filters the types of dependencies to install.
    pub(crate) fn dependency_groups(
        &self,
        include_optional: bool,
    ) -> impl Iterator<Item = DependencyGroup> {
        let &InstallDependencyOptions { prod, dev, optional, no_optional } = self;
        let include_optional = resolve_bool_override(optional, no_optional, include_optional);
        // `--prod` wins over `--dev`, and a dev-only install drops optional
        // dependencies along with the production ones.
        let (has_prod, has_dev, has_optional) = if prod {
            (true, false, include_optional)
        } else if dev {
            (false, true, false)
        } else {
            (true, true, include_optional)
        };
        std::iter::empty()
            .chain(has_prod.then_some(DependencyGroup::Prod))
            .chain(has_dev.then_some(DependencyGroup::Dev))
            .chain(has_optional.then_some(DependencyGroup::Optional))
    }
}

#[derive(Debug, Default, Clone, Args)]
pub struct InstallArgs {
    #[clap(flatten)]
    pub dependency_options: InstallDependencyOptions,

    /// Restrict which optional dependencies are installed, by CPU
    /// (`--cpu`), OS (`--os`), and C library (`--libc`).
    #[clap(flatten)]
    pub supported_architectures: SupportedArchitecturesArgs,

    /// Don't generate a lockfile, and fail if an update to it is needed. This
    /// setting is enabled by default in CI when a lockfile is present.
    #[clap(long, overrides_with = "no_frozen_lockfile")]
    pub frozen_lockfile: bool,

    /// Allow the lockfile to be updated, overriding a `frozenLockfile: true`
    /// setting.
    #[clap(long = "no-frozen-lockfile", overrides_with = "frozen_lockfile")]
    pub no_frozen_lockfile: bool,

    /// Only update `pnpm-lock.yaml`. Don't download packages or write
    /// `node_modules`.
    #[clap(long = "lockfile-only")]
    pub lockfile_only: bool,

    /// Repair broken lockfile entries by re-resolving their metadata while
    /// preserving compatible locked versions.
    #[clap(long = "fix-lockfile")]
    pub fix_lockfile: bool,

    #[clap(flatten)]
    pub lockfile_dir: LockfileDirArg,

    /// Fold every per-branch lockfile (`pnpm-lock.<branch>.yaml`, written
    /// under the `gitBranchLockfile` setting) into `pnpm-lock.yaml` and
    /// delete them.
    #[clap(long = "merge-git-branch-lockfiles")]
    pub merge_git_branch_lockfiles: bool,

    /// Glob patterns naming the branches that merge the per-branch
    /// lockfiles, so a mainline branch does not have to pass
    /// `--merge-git-branch-lockfiles` by hand.
    #[clap(long = "merge-git-branch-lockfiles-branch-pattern")]
    pub merge_git_branch_lockfiles_branch_pattern: Vec<String>,

    /// Show what an install would change without writing anything to disk.
    #[clap(long = "dry-run")]
    pub dry_run: bool,

    /// Reinstall every package the lockfile names: relink packages an
    /// earlier install already materialized, and install optional
    /// dependencies whose `cpu` / `os` / `libc` / `engines` don't match
    /// the host instead of skipping them.
    #[clap(long)]
    pub force: bool,

    /// Prefer the existing lockfile over re-resolving, even when the
    /// manifest may have changed.
    #[clap(long = "prefer-frozen-lockfile", overrides_with = "no_prefer_frozen_lockfile")]
    pub prefer_frozen_lockfile: bool,

    /// Always re-resolve against the registry instead of preferring the
    /// existing lockfile.
    #[clap(long = "no-prefer-frozen-lockfile", overrides_with = "prefer_frozen_lockfile")]
    pub no_prefer_frozen_lockfile: bool,

    /// Run the install already requested by `verifyDepsBeforeRun` without
    /// independently short-circuiting it as up to date.
    #[clap(long, hide = true)]
    pub verify_deps_before_run_install: bool,

    /// Skip the check that `pnpm-lock.yaml` is up to date with
    /// `package.json` under `--frozen-lockfile`. For callers that just
    /// wrote the lockfile themselves and know the manifest is about to
    /// catch up.
    #[clap(long)]
    pub ignore_manifest_check: bool,

    /// Don't install runtime dependencies (`node`, `deno`, `bun`). Their
    /// archives aren't fetched and their bins aren't linked; the rest of
    /// the install proceeds normally.
    #[clap(long = "no-runtime")]
    pub no_runtime: bool,

    /// Don't run lifecycle scripts of the project or its dependencies.
    /// Packages are still installed; only their build scripts are skipped,
    /// and the install won't fail because of it.
    #[clap(long = "ignore-scripts", overrides_with = "no_ignore_scripts")]
    pub ignore_scripts: bool,

    /// Run lifecycle scripts even when the configuration disables them.
    #[clap(long = "no-ignore-scripts", overrides_with = "ignore_scripts")]
    pub no_ignore_scripts: bool,

    /// Disable pnpm hooks defined in `.pnpmfile.cjs`, including the
    /// pnpmfiles of config dependencies.
    #[clap(long = "ignore-pnpmfile")]
    pub ignore_pnpmfile: bool,

    /// Which node linker to use: `isolated` (the default, a symlinked
    /// store), `hoisted` (a flat `node_modules`), or `pnp` (Plug'n'Play).
    /// Overrides the configured value.
    #[clap(long = "node-linker", value_enum)]
    pub node_linker: Option<NodeLinkerArg>,

    /// Fail on a cache miss instead of fetching from the registry, using
    /// only packages already in the store.
    #[clap(long, overrides_with = "no_offline")]
    pub offline: bool,

    /// Allow network fetches even when the configuration enables offline
    /// mode.
    #[clap(long = "no-offline", overrides_with = "offline")]
    pub no_offline: bool,

    /// Open the store read-only and skip all store writes. For installing
    /// against a store on a read-only filesystem (e.g. a Nix store); pair
    /// with `--offline --frozen-lockfile`.
    #[clap(long = "frozen-store", overrides_with = "no_frozen_store")]
    pub frozen_store: bool,

    /// Allow store writes even when the configuration enables the
    /// read-only store.
    #[clap(long = "no-frozen-store", overrides_with = "frozen_store")]
    pub no_frozen_store: bool,

    /// Prefer packages already in the cache over the network, even past
    /// their freshness window.
    #[clap(long, overrides_with = "no_prefer_offline")]
    pub prefer_offline: bool,

    /// Don't prefer cached packages even when the configuration enables
    /// it.
    #[clap(long = "no-prefer-offline", overrides_with = "prefer_offline")]
    pub no_prefer_offline: bool,

    /// Skip verifying the lockfile against supply-chain policies.
    #[clap(long = "trust-lockfile", overrides_with = "no_trust_lockfile")]
    pub trust_lockfile: bool,

    /// Verify the lockfile against supply-chain policies even when the
    /// configuration trusts it.
    #[clap(long = "no-trust-lockfile", overrides_with = "trust_lockfile")]
    pub no_trust_lockfile: bool,

    /// Refresh the integrity checksums in `pnpm-lock.yaml` from the
    /// registry. Cannot be combined with `--frozen-lockfile`.
    #[clap(long = "update-checksums")]
    pub update_checksums: bool,

    /// Maximum number of concurrent network requests during install.
    #[clap(long = "network-concurrency")]
    pub network_concurrency: Option<usize>,

    /// Per-request network timeout, in milliseconds.
    #[clap(long = "fetch-timeout")]
    pub fetch_timeout: Option<u64>,

    /// Warn when a registry metadata request takes longer than this many
    /// milliseconds.
    #[clap(long = "fetch-warn-timeout-ms")]
    pub fetch_warn_timeout_ms: Option<u64>,

    /// Warn when a tarball download's average speed is below this many KiB/s.
    #[clap(long = "fetch-min-speed-ki-bps")]
    pub fetch_min_speed_ki_bps: Option<u64>,

    /// `User-Agent` header to send on registry requests.
    #[clap(long = "user-agent")]
    pub user_agent: Option<String>,

    /// URL of a pnpr server to offload resolution and file fetching to.
    /// `node_modules` is still linked locally from the server-produced
    /// lockfile.
    #[clap(long = "pnpr-server")]
    pub pnpr_server: Option<String>,
}

/// Resolve a boolean whose CLI surface is a `--flag` / `--no-flag` pair
/// against the yaml/`.npmrc` `config` value. The pair's mutual
/// `overrides_with` collapses both spellings in one argv to the
/// last-specified, so at most one of `force_on` / `force_off` is ever
/// set: a set flag wins over `config` in its own direction and an unset
/// pair falls through to it. Mirrors pnpm, where a CLI boolean overrides
/// the workspace/`.npmrc` value either way (nopt's `--no-` negation).
pub(crate) fn resolve_bool_override(force_on: bool, force_off: bool, config: bool) -> bool {
    force_on || (config && !force_off)
}

impl InstallArgs {
    /// The install args for a reinstall triggered by an out-of-band change to
    /// the install inputs: `patch-commit` / `patch-remove` (which rewrite the
    /// manifest's `patchedDependencies`) and `unlink` (which removes `link:`
    /// overrides from `pnpm-workspace.yaml`). Forces a fresh resolution
    /// (`preferFrozenLockfile: false`, via `no_prefer_frozen_lockfile`, and
    /// `frozenLockfile: false`, via `no_frozen_lockfile`) so the changed
    /// inputs re-resolve rather than reusing — or failing against — the
    /// stale lockfile.
    pub(crate) fn for_reresolving_install() -> Self {
        Self { no_frozen_lockfile: true, no_prefer_frozen_lockfile: true, ..Self::default() }
    }

    /// `--frozen-lockfile` / `--no-frozen-lockfile` layered over the
    /// `frozenLockfile` setting.
    pub(crate) fn effective_frozen_lockfile(&self, config: &pnpm_config::Config) -> bool {
        self.configured_frozen_lockfile(config).unwrap_or(false)
    }

    fn configured_frozen_lockfile(&self, config: &pnpm_config::Config) -> Option<bool> {
        self.frozen_lockfile_flag().or(config.frozen_lockfile)
    }

    /// `--frozen-lockfile` / `--no-frozen-lockfile` as typed on the command
    /// line, before the `frozenLockfile` setting is layered under it.
    pub(crate) fn frozen_lockfile_flag(&self) -> Option<bool> {
        if self.frozen_lockfile {
            Some(true)
        } else if self.no_frozen_lockfile {
            Some(false)
        } else {
            None
        }
    }

    pub async fn run<Reporter: self::Reporter + 'static>(self, state: State) -> miette::Result<()> {
        Box::pin(self.run_inner::<Reporter>(state, None)).await
    }

    pub(crate) async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        selection: InstallFamilySelection,
    ) -> miette::Result<()> {
        Box::pin(self.run_inner::<Reporter>(state, Some(selection))).await
    }

    async fn run_inner<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        selection: Option<InstallFamilySelection>,
    ) -> miette::Result<()> {
        state.http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
        let frozen_lockfile = self.resolve_frozen_lockfile(&state)?;
        let lockfile_path = state.lockfile_path();
        let link = self.resolve_link_options(state.config, &lockfile_path, frozen_lockfile);
        if let Some(pnpr_server) = state.config.pnpr_server.as_deref() {
            if self.dry_run {
                return Err(DryRunIncompatibleWithPnpr.into());
            }
            return Box::pin(install_via_pnpr_inner::<Reporter>(
                &state,
                pnpr_server,
                selection.as_ref(),
                link,
            ))
            .await;
        }
        self.run_local::<Reporter>(&state, link, selection.as_ref()).await?;
        // Resolution watch senders must close inline; only inert manifest and lockfile data
        // may outlive the command through background destruction.
        pnpm_fs::background_drop((state.lockfile, state.manifest, selection));
        Ok(())
    }

    fn resolve_link_options<'a>(
        &self,
        config: &pnpm_config::Config,
        lockfile_path: &'a std::path::Path,
        frozen_lockfile: bool,
    ) -> PnprLink<'a> {
        PnprLink {
            dependency_groups: self.dependency_options.dependency_groups(config.optional).collect(),
            supported_architectures: self
                .supported_architectures
                .apply_to(config.supported_architectures.clone()),
            node_linker: self.node_linker.map_or(config.node_linker, NodeLinkerArg::into_config),
            skip_runtimes: config.skip_runtimes || self.no_runtime,
            frozen_lockfile,
            prefer_frozen_lockfile: self
                .prefer_frozen_override()
                .unwrap_or(config.prefer_frozen_lockfile),
            update_patches: false,
            fix_lockfile: self.fix_lockfile,
            lockfile_only: self.lockfile_only,
            ignore_manifest_check: self.ignore_manifest_check,
            trust_lockfile: resolve_bool_override(
                self.trust_lockfile,
                self.no_trust_lockfile,
                config.trust_lockfile,
            ),
            lockfile_path: Some(lockfile_path),
            use_state_lockfile: true,
        }
    }

    fn prefer_frozen_override(&self) -> Option<bool> {
        prefer_frozen_lockfile_override(
            self.fix_lockfile,
            self.prefer_frozen_lockfile,
            self.no_prefer_frozen_lockfile,
        )
    }

    async fn run_local<Reporter: self::Reporter + 'static>(
        &self,
        state: &State,
        link: PnprLink<'_>,
        selection: Option<&InstallFamilySelection>,
    ) -> miette::Result<()> {
        let install_lockfile = if self.fix_lockfile {
            MaybeLazyLockfile::Repair(&state.lockfile)
        } else {
            MaybeLazyLockfile::Lazy(&state.lockfile)
        };
        let install = Install {
            lockfile_path: link.lockfile_path,
            frozen_lockfile: link.frozen_lockfile,
            prefer_frozen_lockfile: self.prefer_frozen_override(),
            ignore_manifest_check: link.ignore_manifest_check,
            skip_runtimes: link.skip_runtimes,
            trust_lockfile: link.trust_lockfile,
            update_checksums: self.update_checksums,
            supported_architectures: link.supported_architectures,
            node_linker: link.node_linker,
            lockfile_only: link.lockfile_only,
            dry_run: self.dry_run,
            policy_excludes: PolicyExcludes::Persist,
            update_seed_policy: if self.fix_lockfile {
                UpdateSeedPolicy::FixLockfile
            } else {
                UpdateSeedPolicy::KeepAll
            },
            disable_optimistic_repeat_install: self.verify_deps_before_run_install,
            lockfile: install_lockfile,
            ..state.install(link.dependency_groups)
        };
        match selection {
            Some(selection) => {
                install.run_selected::<Reporter>(workspace_install_selection(selection)).await
            }
            None => install.run::<Reporter>().await,
        }
        .wrap_err("installing dependencies")
    }

    /// Whether this install runs frozen.
    ///
    /// `--fix-lockfile` rewrites the lockfile, so it is never frozen. On
    /// CI a project that already has a non-empty lockfile installs frozen
    /// by default, unless the run said otherwise through
    /// `--lockfile-only`, either `preferFrozenLockfile` flag, or the
    /// setting itself.
    fn resolve_frozen_lockfile(&self, state: &State) -> miette::Result<bool> {
        if self.fix_lockfile {
            return Ok(false);
        }
        if let Some(value) = self.configured_frozen_lockfile(state.config) {
            return Ok(value);
        }
        let ci_default = state.config.ci
            && !self.lockfile_only
            && !self.prefer_frozen_lockfile
            && !self.no_prefer_frozen_lockfile
            && !state.config.explicit_settings.contains_key("preferFrozenLockfile");
        if !ci_default {
            return Ok(false);
        }
        Ok(state.lockfile.get()?.is_some_and(|lockfile| !lockfile.is_empty()))
    }
}

pub(crate) fn workspace_install_selection(
    selection: &InstallFamilySelection,
) -> WorkspaceInstallSelection<'_> {
    WorkspaceInstallSelection {
        all_projects: &selection.projects,
        project_dependencies: &selection.project_dependencies,
        ordered_dirs: &selection.ordered_dirs,
        selected_dirs: selection.selected_dirs.as_ref(),
        install_dirs: selection.install_dirs.as_ref(),
        active_manifest_is_standin: selection.active_manifest_is_standin,
        workspace_cycles: selection.workspace_cycles.as_ref().map_or(
            pnpm_package_manager::PrecomputedWorkspaceCycles::Unknown,
            |cycles| {
                pnpm_package_manager::PrecomputedWorkspaceCycles::Known(
                    (!cycles.is_empty()).then_some(cycles.as_slice()),
                )
            },
        ),
    }
}

/// Per-invocation install knobs forwarded to the frozen link pass,
/// already resolved from the CLI flags + config by [`InstallArgs::run`].
pub(crate) struct PnprLink<'a> {
    pub(crate) dependency_groups: Vec<DependencyGroup>,
    pub(crate) supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
    pub(crate) node_linker: NodeLinker,
    pub(crate) skip_runtimes: bool,
    /// Governs the *server's* resolution behavior (frozen vs
    /// reuse-and-update); forwarded to `/-/pnpr/v0/resolve`. The local
    /// materialization always runs frozen against the server-produced
    /// lockfile.
    pub(crate) frozen_lockfile: bool,
    /// The *effective* `preferFrozenLockfile` (the CLI tri-state already
    /// resolved against `config.prefer_frozen_lockfile`, exactly as the
    /// local `Install` resolves it); forwarded to `/-/pnpr/v0/resolve`. `false`
    /// forces the server to re-resolve. Resolving here — rather than
    /// sending the raw CLI override — keeps a yaml `preferFrozenLockfile:
    /// false` honored on the pnpr path without `--no-prefer-frozen-lockfile`.
    pub(crate) prefer_frozen_lockfile: bool,
    /// Refresh registry artifacts while retaining every locked package
    /// version. This disables the exchange-free satisfied-lockfile path and
    /// is forwarded to `/-/pnpr/v0/resolve`.
    pub(crate) update_patches: bool,
    /// Regenerate derived lockfile metadata while retaining compatible pins.
    pub(crate) fix_lockfile: bool,
    /// `--lockfile-only`. Forwarded to `/-/pnpr/v0/resolve` so the server
    /// resolves only — returning the lockfile without fetching files —
    /// after which [`install_via_pnpr`] writes the lockfile and skips
    /// materialization, mirroring pnpm's resolve + write, fetch nothing,
    /// link nothing. See
    /// [pnpm/pnpm#12146](https://github.com/pnpm/pnpm/issues/12146).
    pub(crate) lockfile_only: bool,
    /// `--ignore-manifest-check`; forwarded so the server's frozen
    /// freshness check and the local materialization both skip the
    /// manifest ↔ lockfile comparison.
    pub(crate) ignore_manifest_check: bool,
    /// The effective `trustLockfile` (yaml `trustLockfile` OR
    /// `--trust-lockfile`); forwarded so the server skips verifying the
    /// input lockfile when the user opted out, mirroring the local path.
    pub(crate) trust_lockfile: bool,
    pub(crate) lockfile_path: Option<&'a std::path::Path>,
    pub(crate) use_state_lockfile: bool,
}

/// `--prefer-frozen-lockfile` / `--no-prefer-frozen-lockfile` map to
/// `Option<bool>`: `Some` when either flag is set, `None` otherwise (use
/// config). The pair's mutual `overrides_with` collapses both spellings
/// to the last-specified, so at most one is set. `--fix-lockfile`
/// rewrites the lockfile, so it never prefers the frozen one.
fn prefer_frozen_lockfile_override(
    fix_lockfile: bool,
    prefer_frozen_lockfile: bool,
    no_prefer_frozen_lockfile: bool,
) -> Option<bool> {
    if fix_lockfile {
        return Some(false);
    }
    if prefer_frozen_lockfile {
        return Some(true);
    }
    no_prefer_frozen_lockfile.then_some(false)
}

#[cfg(test)]
mod tests;

mod fast_path;

mod pnpr_request;

mod pnpr_lockfile;

mod pnpr_resolution;
