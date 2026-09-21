use crate::{
    State,
    cli_args::{
        lockfile_dir::LockfileDirArg,
        pipelines::InstallFamilySelection,
        recursive,
        supported_architectures::SupportedArchitecturesArgs,
        update_interactive::{InteractiveUpdateOptions, UpdatePrompt},
        workspace_option::{WorkspaceOptionError, workspace_link_root},
    },
};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic};
use pnpm_config::Config;
use pnpm_github_actions as github_actions;
use pnpm_matcher::Matcher;
use pnpm_package_manager::{Update, build_workspace_packages_map, included_direct_groups};
use pnpm_package_manifest::DependencyGroup;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::Reporter;
use std::{collections::HashSet, path::Path};

/// The `--prod`, `--dev`, and `--no-optional` flags that select which
/// dependency groups to update.
#[derive(Debug, Clone, Args)]
pub struct UpdateDependencyOptions {
    /// Update packages only in "dependencies" and "optionalDependencies".
    #[clap(short = 'P', long, visible_alias = "production")]
    prod: bool,
    /// Update packages only in "devDependencies".
    #[clap(short = 'D', long)]
    dev: bool,
    /// Update packages only in "optionalDependencies".
    #[clap(long, overrides_with = "no_optional")]
    optional: bool,
    /// Don't update packages in "optionalDependencies".
    #[clap(long, overrides_with = "optional")]
    no_optional: bool,
    /// Also update packages in "peerDependencies".
    #[clap(long)]
    peer: bool,
}

impl UpdateDependencyOptions {
    /// The dependency groups whose direct dependencies the update may
    /// match. Returns the groups for which the corresponding inclusion bit
    /// is set.
    ///
    /// This narrows what the update *matches*, not what the install that
    /// follows it materializes: pnpm leaves the `included` set recorded in
    /// `.modules.yaml` untouched for an update, so these flags never reach
    /// [`Config::optional`] and friends.
    fn include_direct(&self) -> Vec<DependencyGroup> {
        // `Some(true)` only when the flag was explicitly passed: the raw
        // CLI flags are read rather than the merged config.
        let production = self.prod.then_some(true);
        let dev = self.dev.then_some(true);
        let optional = self.optional
            .then_some(true)
            .or_else(|| self.no_optional.then_some(false));

        let ne_true = |flag: Option<bool>| flag != Some(true);
        let dependencies = production == Some(true) || (ne_true(dev) && ne_true(optional));
        let dev_dependencies = dev == Some(true) || (ne_true(production) && ne_true(optional));
        let optional_dependencies = optional == Some(true) || (ne_true(production) && ne_true(dev));

        std::iter::empty()
            .chain(dependencies.then_some(DependencyGroup::Prod))
            .chain(dev_dependencies.then_some(DependencyGroup::Dev))
            .chain(optional_dependencies.then_some(DependencyGroup::Optional))
            .chain(self.peer.then_some(DependencyGroup::Peer))
            .collect()
    }
}

/// Update package and GitHub Actions dependencies to newer compatible versions.
#[derive(Debug, Clone, Args)]
pub struct UpdateArgs {
    /// Dependencies to update. Package names (`foo`, `@scope/bar`), GitHub
    /// Actions (`actions/checkout`, with `--include-github-actions`), glob
    /// patterns (`@scope/bar-*`), and versioned selectors (`foo@2`) are
    /// accepted. With no arguments, every direct dependency in the
    /// included groups is updated.
    pub packages: Vec<String>,
    /// --prod, --dev, and --no-optional.
    #[clap(flatten)]
    pub dependency_options: UpdateDependencyOptions,
    /// The `--cpu`, `--os`, and `--libc` flags that select which platforms'
    /// optional dependencies to install.
    #[clap(flatten)]
    pub supported_architectures: SupportedArchitecturesArgs,
    #[clap(skip)]
    pub(crate) prompt: UpdatePrompt,
    #[clap(flatten)]
    pub scripts: crate::cli_args::install_options::ScriptExecutionArgs,
    #[clap(flatten)]
    pub selection: UpdateSelectionArgs,
    #[clap(flatten)]
    pub save: UpdateSaveArgs,
    #[clap(flatten)]
    pub install: UpdateInstallArgs,
}

#[derive(Debug, Clone, clap::Args)]
pub struct UpdateSelectionArgs {
    /// Ignore version ranges in package.json: bump the matched packages
    /// to their latest version and rewrite the manifest ranges.
    #[clap(short = 'L', long)]
    pub latest: bool,
    /// Refresh registry revisions without changing package versions.
    #[clap(long)]
    pub patches: bool,
    /// How deep to inspect dependencies. `0` means top-level
    /// dependencies only. Defaults to unlimited.
    #[clap(long)]
    pub depth: Option<usize>,
    /// Show outdated dependencies and select which ones to update.
    #[clap(short = 'i', long)]
    pub interactive: bool,
    /// Also update GitHub Actions dependencies in workflow and action files.
    #[clap(long = "include-github-actions")]
    pub include_github_actions: bool,
    /// Update globally installed packages.
    #[clap(short = 'g', long)]
    pub global: bool,
    /// Tries to link all packages from the workspace, updating versions
    /// to match the workspace packages.
    #[clap(long)]
    pub workspace: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct UpdateSaveArgs {
    /// Write the resolved version without a range operator when
    /// rewriting the manifest under `--latest`.
    #[clap(short = 'E', long = "save-exact")]
    #[clap(id = "save_exact")]
    pub exact: bool,
    /// Do not write the updated ranges back to package.json. The
    /// lockfile is still updated (the `--no-save` flag).
    #[clap(long = "no-save")]
    pub no_save: bool,
    /// Generate a changeset file declaring a patch bump for every workspace
    /// package whose production dependencies were changed by the update.
    #[clap(long, overrides_with = "no_changeset")]
    pub changeset: bool,
    /// Do not generate a changeset, even when `updateConfig.changeset` enables
    /// changeset generation by default.
    #[clap(long = "no-changeset", overrides_with = "changeset")]
    pub no_changeset: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct UpdateInstallArgs {
    /// Dependencies are not downloaded; only `pnpm-lock.yaml` is updated.
    #[clap(long = "lockfile-only")]
    pub lockfile_only: bool,
    #[clap(flatten)]
    pub lockfile_dir: LockfileDirArg,
    /// URL of a pnpr server to offload revision refresh resolution to.
    #[clap(long = "pnpr-server")]
    pub pnpr_server: Option<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "--patches cannot be combined with package selectors, --latest, --interactive, or --global"
)]
#[diagnostic(code(ERR_PNPM_PATCHES_WITH_SELECTOR))]
struct PatchesWithSelectorError;

impl UpdateArgs {
    pub(crate) fn apply_cli_config(&self, config: &mut Config) {
        self.scripts.apply(config);
        if let Some(pnpr_server) = self.install.pnpr_server.clone() {
            config.pnpr_server = Some(pnpr_server);
        }
    }

    pub async fn run<Reporter: self::Reporter + 'static>(self, state: State) -> miette::Result<()> {
        self.run_inner::<Reporter>(state, None).await
    }

    pub(crate) async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        selection: InstallFamilySelection,
    ) -> miette::Result<()> {
        self.run_inner::<Reporter>(state, Some(selection)).await
    }

    fn interactive_options<'a>(
        &self,
        include_direct: &'a [DependencyGroup],
        update_actions: bool,
    ) -> InteractiveUpdateOptions<'a> {
        InteractiveUpdateOptions {
            latest: self.selection.latest,
            include_direct,
            include_github_actions: update_actions,
            prompt: self.prompt,
        }
    }

    /// The packages to update: the prompt's picks when interactive, else
    /// the selectors given. `None` when nothing was outdated or the user
    /// picked nothing, since an empty selector list would mean a full
    /// update.
    async fn prompted_or_given<Reporter: self::Reporter + 'static>(
        &self,
        state: &State,
        lockfile: Option<&pnpm_lockfile::Lockfile>,
        actions_root: &Path,
        prompt: InteractiveUpdateOptions<'_>,
        given: Vec<String>,
    ) -> miette::Result<Option<Vec<String>>> {
        if !self.selection.interactive {
            return Ok(Some(given));
        }
        crate::cli_args::update_interactive::select_packages::<Reporter>(
            actions_root,
            &state.manifest,
            lockfile,
            &state.active_importer_id(),
            state.config,
            &state.http_client,
            prompt,
        )
        .await
    }

    /// [`Self::prompted_or_given`] over the selected projects.
    async fn prompted_or_given_for_projects<Reporter: self::Reporter + 'static>(
        &self,
        state: &State,
        selection: &InstallFamilySelection,
        lockfile: Option<&pnpm_lockfile::Lockfile>,
        actions_root: &Path,
        prompt: InteractiveUpdateOptions<'_>,
        given: Vec<String>,
    ) -> miette::Result<Option<Vec<String>>> {
        if !self.selection.interactive {
            return Ok(Some(given));
        }
        crate::cli_args::update_interactive::select_packages_for_projects::<Reporter>(
            actions_root,
            selection,
            lockfile,
            state.config,
            &state.http_client,
            prompt,
        )
        .await
    }

    /// `pnpm update -g`: reinstall each matching global package group,
    /// within its existing range or (with `--latest`) to the newest
    /// version. Delegates to [`crate::cli_args::global::handle_global_update`].
    pub async fn run_global<Reporter: self::Reporter + 'static>(
        self,
        config: &'static Config,
    ) -> miette::Result<()> {
        self.check_patches_options()?;
        self.check_workspace_option(None)?;
        if crate::cli_args::global::selects_pnpm_cli(&self.packages) {
            return Err(crate::cli_args::global::GlobalError::GlobalPnpmInstall.into());
        }
        let selected_hashes: Option<HashSet<String>> = if self.selection.interactive {
            match crate::cli_args::update_interactive::select_global_package_groups::<Reporter>(
                config,
                &self.packages,
                self.selection.latest,
                self.prompt,
            )
            .await?
            {
                Some(selected) => Some(selected),
                None => return Ok(()),
            }
        } else {
            None
        };
        let supported_architectures =
            self.supported_architectures.apply_to(config.supported_architectures.clone());
        let range_spec_style = RangeSpecStyle::from_save_options(
            self.save.exact || config.save_exact,
            config.save_prefix.as_deref(),
        );
        Box::pin(crate::cli_args::global::handle_global_update::<Reporter>(
            config,
            &self.packages,
            selected_hashes.as_ref(),
            self.selection.latest,
            range_spec_style,
            supported_architectures,
        ))
        .await
    }

    /// Validate `--workspace` against the rest of the invocation,
    /// returning the workspace root the link targets are read from.
    /// `Ok(None)` means the flag was not passed. Every dispatch path
    /// calls this before any resolution happens, `--global` included —
    /// the global directory is never a workspace.
    fn check_workspace_option<'root>(
        &self,
        workspace_root: Option<&'root Path>,
    ) -> miette::Result<Option<&'root Path>> {
        if self.selection.workspace && self.selection.latest {
            return Err(WorkspaceOptionError::LatestWithWorkspace.into());
        }
        workspace_link_root(self.selection.workspace, workspace_root)
    }

    fn check_patches_options(&self) -> miette::Result<()> {
        if self.selection.patches
            && (!self.packages.is_empty()
                || self.selection.latest
                || self.selection.interactive
                || self.selection.global)
        {
            return Err(PatchesWithSelectorError.into());
        }
        Ok(())
    }

    fn can_delegate_patch_refresh(
        &self,
        update_actions: bool,
        include_direct: &[DependencyGroup],
    ) -> bool {
        let all_dependency_groups =
            [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
        self.selection.patches
            && self.selection.depth.is_none()
            && !update_actions
            && all_dependency_groups
                .iter()
                .all(|group| include_direct.contains(group))
    }

    fn pnpr_patch_link<'path>(
        &self,
        state: &State,
        lockfile_path: &'path Path,
    ) -> super::install::PnprLink<'path> {
        super::install::PnprLink {
            dependency_groups: included_direct_groups(state.config.optional).collect(),
            supported_architectures: self.supported_architectures.apply_to(
                state.config.supported_architectures.clone(),
            ),
            node_linker: state.config.node_linker,
            skip_runtimes: state.config.skip_runtimes,
            lockfile_path: Some(lockfile_path),
            use_state_lockfile: true,
            lockfile: crate::cli_args::install::PnprLockfilePolicy {
                frozen: false,
                prefer_frozen: false,
                update_patches: true,
                fix: false,
                only: self.install.lockfile_only,
                ignore_manifest_check: false,
                trust: state.config.trust_lockfile,
            },
        }
    }

    fn should_update_github_actions(
        &self,
        config: &Config,
        include_direct: &[DependencyGroup],
    ) -> bool {
        include_direct.contains(&DependencyGroup::Dev)
            && !self.save.no_save
            && !self.install.lockfile_only
            && crate::github_actions::opted_in(self.selection.include_github_actions, config)
    }
}

#[cfg(test)]
mod tests;

mod execution;
