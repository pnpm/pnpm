pub use arguments::{
    InstallFetchArgs, InstallLockfileArgs, InstallMaterializationArgs, LockfileUpdateArgs,
};

pub(crate) use pnpr_resolution::{install_selected_via_pnpr, install_via_pnpr};

mod arguments;

use crate::{
    State,
    cli_args::{
        legacy_pnpm_field::warn_ignored_pnpm_manifest_fields, lockfile_dir::LockfileDirArg,
        override_version_references::warn_deprecated_override_version_references,
        package_manager::read_root_manifest, pipelines::InstallFamilySelection,
        recursive::discover_workspace_projects,
        supported_architectures::SupportedArchitecturesArgs,
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
    InstallFrozenLockfileError, LockfileVerificationOverride, PolicyExcludes, SkippedSnapshots,
    TarballPrefetcher, UpToDateFastPathCheck, UpdateSeedPolicy, WantedLockfileSatisfactionCheck,
    WorkspaceInstallSelection, build_resolution_verifiers, install_already_up_to_date,
    materialization_closure, merge_filtered_wanted_lockfile, report_merged_lockfile_conflicts,
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
        included_dependency_groups(
            prod,
            dev,
            resolve_bool_override(optional, no_optional, include_optional),
        )
    }
}

/// The dependency groups an install resolves and materializes, given the
/// `--prod` / `--dev` filter and whether optional dependencies are
/// included.
///
/// Shared with `add`, which takes the same two flags through `pnpm
/// install <pkg>`.
pub(crate) fn included_dependency_groups(
    prod: bool,
    dev: bool,
    include_optional: bool,
) -> impl Iterator<Item = DependencyGroup> {
    // `--prod` wins over `--dev`.
    let (has_prod, has_dev) = if prod {
        (true, false)
    } else if dev {
        (false, true)
    } else {
        (true, true)
    };
    std::iter::empty()
        .chain(has_prod.then_some(DependencyGroup::Prod))
        .chain(has_dev.then_some(DependencyGroup::Dev))
        .chain(include_optional.then_some(DependencyGroup::Optional))
}

#[derive(Debug, Default, Clone, Args)]
pub struct InstallArgs {
    #[clap(flatten)]
    pub dependency_options: InstallDependencyOptions,
    /// Restrict which optional dependencies are installed, by CPU
    /// (`--cpu`), OS (`--os`), and C library (`--libc`).
    #[clap(flatten)]
    pub supported_architectures: SupportedArchitecturesArgs,
    #[clap(flatten)]
    pub scripts: crate::cli_args::install_options::ScriptExecutionArgs,
    #[clap(flatten)]
    pub network_cache: crate::cli_args::install_options::OfflineArgs,
    #[clap(flatten)]
    pub lockfile: InstallLockfileArgs,
    #[clap(flatten)]
    pub lockfile_updates: LockfileUpdateArgs,
    #[clap(flatten)]
    pub fetching: InstallFetchArgs,
    #[clap(flatten)]
    pub materialization: InstallMaterializationArgs,
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
        Self {
            lockfile: InstallLockfileArgs {
                no_frozen: true,
                no_prefer_frozen: true,
                ..Default::default()
            },
            ..Self::default()
        }
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
        if self.lockfile.frozen {
            Some(true)
        } else if self.lockfile.no_frozen {
            Some(false)
        } else {
            None
        }
    }

    /// Package names allowed to run lifecycle (build) scripts during this install.
    pub fn allow_build(&self) -> &[String] {
        &self.materialization.allow_build
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
            if self.materialization.dry_run {
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
            supported_architectures: self.supported_architectures.apply_to(
                config.supported_architectures.clone(),
            ),
            node_linker: self.materialization.node_linker.map_or(
                config.node_linker,
                NodeLinkerArg::into_config,
            ),
            skip_runtimes: config.skip_runtimes || self.materialization.no_runtime,
            lockfile_path: Some(lockfile_path),
            use_state_lockfile: true,
            lockfile: PnprLockfilePolicy {
                frozen: frozen_lockfile,
                prefer_frozen: self
                    .prefer_frozen_override()
                    .unwrap_or(config.prefer_frozen_lockfile),
                update_patches: false,
                fix: self.lockfile.fix,
                only: self.lockfile.only,
                ignore_manifest_check: self.lockfile.ignore_manifest_check,
                trust: resolve_bool_override(
                    self.lockfile_updates.trust_lockfile,
                    self.lockfile_updates.no_trust_lockfile,
                    config.trust_lockfile,
                ),
            },
        }
    }

    fn prefer_frozen_override(&self) -> Option<bool> {
        prefer_frozen_lockfile_override(
            self.lockfile.fix,
            self.lockfile.prefer_frozen,
            self.lockfile.no_prefer_frozen,
        )
    }

    async fn run_local<Reporter: self::Reporter + 'static>(
        &self,
        state: &State,
        link: PnprLink<'_>,
        selection: Option<&InstallFamilySelection>,
    ) -> miette::Result<()> {
        let install_lockfile = if self.lockfile.fix {
            MaybeLazyLockfile::Repair(&state.lockfile)
        } else {
            MaybeLazyLockfile::Lazy(&state.lockfile)
        };
        let install = {
            let mut base_install = state.install(link.dependency_groups);
            base_install.lockfile_policy.frozen = link.lockfile.frozen;
            base_install.lockfile_policy.prefer_frozen = self.prefer_frozen_override();
            base_install.lockfile_policy.ignore_manifest_check =
                link.lockfile.ignore_manifest_check;
            base_install.lockfile_policy.trust = link.lockfile.trust;
            base_install.lockfile_policy.update_checksums = self
                .lockfile_updates
                .update_checksums;
            base_install.lockfile_policy.excludes = PolicyExcludes::Persist;
            base_install.lockfile_policy.disable_optimistic_repeat = self
                .materialization
                .verify_deps_before_run_install;
            base_install.execution.skip_runtimes = link.skip_runtimes;
            base_install.execution.node_linker = link.node_linker;
            base_install.execution.lockfile_only = link.lockfile.only;
            base_install.execution.dry_run = self.materialization.dry_run;
            base_install.resolution.update_seed_policy = if self.lockfile.fix {
                UpdateSeedPolicy::FixLockfile
            } else {
                UpdateSeedPolicy::KeepAll
            };
            base_install.context.lockfile_path = link.lockfile_path;
            base_install.context.lockfile = install_lockfile;
            base_install.projects.supported_architectures = link.supported_architectures;
            base_install
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
        if self.lockfile.fix {
            return Ok(false);
        }
        if let Some(value) = self.configured_frozen_lockfile(state.config) {
            return Ok(value);
        }
        let ci_default = state.config.ci
            && !self.lockfile.only
            && !self.lockfile.prefer_frozen
            && !self.lockfile.no_prefer_frozen
            && !state.config.explicit_settings.contains_key("preferFrozenLockfile");
        if !ci_default {
            return Ok(false);
        }
        Ok(state.lockfile
            .get()?
            .is_some_and(|lockfile| !lockfile.is_empty()))
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
        edited_dirs: None,
        install_dirs: selection.install_dirs.as_ref(),
        active_manifest_is_standin: selection.active_manifest_is_standin,
        workspace_cycles: selection.workspace_cycles
            .as_ref()
            .map_or(pnpm_package_manager::PrecomputedWorkspaceCycles::Unknown, |cycles| {
                pnpm_package_manager::PrecomputedWorkspaceCycles::Known(
                    (!cycles.is_empty()).then_some(cycles.as_slice()),
                )
            }),
    }
}

/// Per-invocation install knobs forwarded to the frozen link pass,
/// already resolved from the CLI flags + config by [`InstallArgs::run`].
pub(crate) struct PnprLink<'a> {
    pub(crate) dependency_groups: Vec<DependencyGroup>,
    pub(crate) supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
    pub(crate) node_linker: NodeLinker,
    pub(crate) skip_runtimes: bool,
    pub(crate) lockfile_path: Option<&'a std::path::Path>,
    pub(crate) use_state_lockfile: bool,
    pub(crate) lockfile: PnprLockfilePolicy,
}

pub(crate) struct PnprLockfilePolicy {
    /// Governs the *server's* resolution behavior (frozen vs
    /// reuse-and-update); forwarded to `/-/pnpr/v0/resolve`. The local
    /// materialization always runs frozen against the server-produced
    /// lockfile.
    pub(crate) frozen: bool,
    /// The *effective* `preferFrozenLockfile` (the CLI tri-state already
    /// resolved against `config.prefer_frozen_lockfile`, exactly as the
    /// local `Install` resolves it); forwarded to `/-/pnpr/v0/resolve`. `false`
    /// forces the server to re-resolve. Resolving here — rather than
    /// sending the raw CLI override — keeps a yaml `preferFrozenLockfile:
    /// false` honored on the pnpr path without `--no-prefer-frozen-lockfile`.
    pub(crate) prefer_frozen: bool,
    /// Refresh registry artifacts while retaining every locked package
    /// version. This disables the exchange-free satisfied-lockfile path and
    /// is forwarded to `/-/pnpr/v0/resolve`.
    pub(crate) update_patches: bool,
    /// Regenerate derived lockfile metadata while retaining compatible pins.
    pub(crate) fix: bool,
    /// `--lockfile-only`. Forwarded to `/-/pnpr/v0/resolve` so the server
    /// resolves only — returning the lockfile without fetching files —
    /// after which [`install_via_pnpr`] writes the lockfile and skips
    /// materialization, mirroring pnpm's resolve + write, fetch nothing,
    /// link nothing. See
    /// [pnpm/pnpm#12146](https://github.com/pnpm/pnpm/issues/12146).
    pub(crate) only: bool,
    /// `--ignore-manifest-check`; forwarded so the server's frozen
    /// freshness check and the local materialization both skip the
    /// manifest ↔ lockfile comparison.
    pub(crate) ignore_manifest_check: bool,
    /// The effective `trustLockfile` (yaml `trustLockfile` OR
    /// `--trust-lockfile`); forwarded so the server skips verifying the
    /// input lockfile when the user opted out, mirroring the local path.
    pub(crate) trust: bool,
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
