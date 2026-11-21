pub(crate) mod options;

pub use options::{
    UpdateDependencyOptions,
    UpdateInstallArgs,
    UpdateSaveArgs,
    UpdateSelectionArgs,
};

use crate::{
    State,
    cli_args::{
        outdated::TargetVersion,
        pipelines::InstallFamilySelection,
        recursive,
        supported_architectures::SupportedArchitecturesArgs,
        update_interactive::{
            InteractiveUpdateOptions,
            UpdateDetails,
            UpdatePrompt,
        },
        workspace_option::{
            WorkspaceOptionError,
            workspace_link_root,
        },
    },
};
use clap::Args;
use derive_more::{
    Display,
    Error,
};
use miette::{
    Context,
    Diagnostic,
};
use pnpm_config::Config;
use pnpm_github_actions as github_actions;
use pnpm_lockfile_preferred_versions::get_version_selector_type;
use pnpm_matcher::Matcher;
use pnpm_package_manager::{
    Update,
    build_workspace_packages_map,
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::Reporter;
use pnpm_resolving_resolver_base::VersionSelectorType;
use std::{
    collections::HashSet,
    path::Path,
};

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

#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "--patches cannot be combined with package selectors, --latest, --tag, --interactive, or --global"
)]
#[diagnostic(code(ERR_PNPM_PATCHES_WITH_SELECTOR))]
struct PatchesWithSelectorError;

#[derive(Debug, Display, Error, Diagnostic)]
#[display("--peer cannot be combined with --interactive")]
#[diagnostic(code(ERR_PNPM_INTERACTIVE_PEER_UNSUPPORTED))]
struct InteractivePeerUnsupportedError;

#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "Invalid dist-tag: {raw}. A dist-tag is a name that is not a version or a range, made of URI-safe characters (letters, digits, and -_.!~*'())"
)]
#[diagnostic(code(ERR_PNPM_UPDATE_INVALID_TAG))]
struct InvalidTagError {
    #[error(not(source))]
    raw: String,
}

impl UpdateArgs {
    pub(crate) fn apply_cli_config(&self, config: &mut Config) {
        self.scripts.apply(config);
        if let Some(pnpr_server) = self.install.pnpr_server.clone() {
            config.cli_settings.insert("pnprServer".to_string());
            config.pnpr_server = Some(pnpr_server);
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

    fn interactive_options<'a>(
        &'a self,
        include_direct: &'a [DependencyGroup],
        update_actions: bool,
        packages: &'a [String],
    ) -> InteractiveUpdateOptions<'a> {
        // `--no-save` keeps the manifest ranges, so the update stays inside
        // them whatever `--latest` or `--tag` asks for.
        let target_version = if self.save.no_save {
            TargetVersion::WithinRange
        } else {
            TargetVersion::from_update_flags(self.selection.latest, self.selection.tag.as_deref())
        };
        let package_selectors = (!update_actions
            || !crate::github_actions::selects_actions_only(&self.packages, packages))
        .then_some(packages);
        InteractiveUpdateOptions {
            target_version,
            include_direct,
            package_selectors,
            include_github_actions: update_actions,
            action_matcher: update_actions
                .then(|| github_actions::selector_matcher(&self.packages))
                .flatten(),
            latest_github_actions: self.selection.latest,
            prompt: self.prompt,
            details: if self.selection.display.long {
                UpdateDetails::Long
            } else {
                UpdateDetails::Compact
            },
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
        if !self.selection.display.interactive {
            return Ok(Some(given));
        }
        pnpm_package_manager::reject_versioned_selectors(
            &self.version_options(state.config),
            &given,
        )?;
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
        if !self.selection.display.interactive {
            return Ok(Some(given));
        }
        pnpm_package_manager::reject_versioned_selectors(
            &self.version_options(state.config),
            &given,
        )?;
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

    fn version_options(&self, config: &Config) -> pnpm_package_manager::UpdateVersionOptions<'_> {
        pnpm_package_manager::UpdateVersionOptions {
            latest: self.selection.latest,
            tag: self.selection.tag.as_deref(),
            patches: self.selection.patches,
            range_spec_style: self.range_spec_style(config),
            save: !self.save.no_save,
        }
    }

    /// The style a rewritten specifier falls back to: `--save-exact` layered
    /// over the `saveExact` and `savePrefix` settings.
    fn range_spec_style(&self, config: &Config) -> RangeSpecStyle {
        RangeSpecStyle::from_save_options(
            self.save.exact || config.save_exact,
            config.save_prefix.as_deref(),
        )
    }

    /// `pnpm update -g`: reinstall each matching global package group,
    /// within its existing range or to the version `--latest` / `--tag`
    /// names. Delegates to [`crate::cli_args::global::handle_global_update`].
    pub async fn run_global<Reporter: self::Reporter + 'static>(
        self,
        config: &'static Config,
    ) -> miette::Result<()> {
        self.check_global_options()?;
        pnpm_package_manager::reject_versioned_selectors(
            &self.version_options(config),
            &self.packages,
        )?;
        let mode = crate::cli_args::global::GlobalUpdateMode::from_flags(
            self.selection.latest,
            self.selection.tag.as_deref(),
        );
        let supported_architectures =
            self.supported_architectures.apply_to(config.supported_architectures.clone());
        let range_spec_style = self.range_spec_style(config);
        // Before the interactive selection, so the migrated groups are
        // offered too.
        Box::pin(crate::cli_args::global::migrate_legacy_global_packages::<Reporter>(
            config,
            range_spec_style,
            supported_architectures.clone(),
        ))
        .await?;
        let selected_hashes = if self.selection.display.interactive {
            let Some(selected) =
                self.select_global_groups::<Reporter>(config, mode.target_version()).await?
            else {
                return Ok(());
            };
            Some(selected)
        } else {
            None
        };
        Box::pin(crate::cli_args::global::handle_global_update::<Reporter>(
            config,
            &self.packages,
            selected_hashes.as_ref(),
            mode,
            range_spec_style,
            supported_architectures,
        ))
        .await
    }

    /// The groups `--interactive` picked, or `None` when the prompt was
    /// cancelled.
    async fn select_global_groups<Reporter: self::Reporter + 'static>(
        &self,
        config: &'static Config,
        target_version: crate::cli_args::outdated::TargetVersion<'_>,
    ) -> miette::Result<Option<HashSet<String>>> {
        crate::cli_args::update_interactive::select_global_package_groups::<Reporter>(
            config,
            &self.packages,
            target_version,
            self.prompt,
        )
        .await
    }

    fn check_global_options(&self) -> miette::Result<()> {
        self.check_flag_combinations()?;
        self.check_workspace_option(None)?;
        if crate::cli_args::global::selects_pnpm_cli(&self.packages) {
            return Err(crate::cli_args::global::GlobalError::GlobalPnpmInstall.into());
        }
        Ok(())
    }

    /// Validate `--workspace` against the rest of the invocation,
    /// returning the workspace root the link targets are read from.
    /// `Ok(None)` means the flag was not passed. Every dispatch path
    /// calls this before any resolution happens, `--global` included,
    /// the global directory is never a workspace.
    fn check_workspace_option<'root>(
        &self,
        workspace_root: Option<&'root Path>,
    ) -> miette::Result<Option<&'root Path>> {
        if self.selection.workspace && self.selection.latest {
            return Err(WorkspaceOptionError::WithLatest.into());
        }
        if self.selection.workspace && self.selection.tag.is_some() {
            return Err(WorkspaceOptionError::WithTag.into());
        }
        workspace_link_root(self.selection.workspace, workspace_root)
    }

    fn check_patches_options(&self) -> miette::Result<()> {
        if self.selection.patches
            && (!self.packages.is_empty()
                || self.selection.latest
                || self.selection.tag.is_some()
                || self.selection.display.interactive
                || self.selection.global)
        {
            return Err(PatchesWithSelectorError.into());
        }
        Ok(())
    }

    fn check_interactive_peer_options(&self) -> miette::Result<()> {
        if self.selection.display.interactive && self.dependency_options.peer {
            return Err(InteractivePeerUnsupportedError.into());
        }
        Ok(())
    }

    /// `--tag` names a dist-tag, so the value must be one the resolver reads
    /// as a tag. A protocol-like value (`file:../x`, a URL) resolves through
    /// no resolver in the tag chain, and the rewrite would then write the raw
    /// string into the manifest as the dependency's new specifier. A version
    /// or range (`1.2.3`, `*`) would resolve as one, so a dependency would
    /// move without the registry publishing a tag of that name.
    fn check_tag_value(&self) -> miette::Result<()> {
        let Some(tag) = self.selection.tag.as_deref() else {
            return Ok(());
        };
        if !pnpm_resolving_npm_resolver::is_valid_dist_tag(tag)
            || get_version_selector_type(tag) != Some(VersionSelectorType::Tag)
        {
            return Err(InvalidTagError { raw: tag.to_string() }.into());
        }
        Ok(())
    }

    /// The flag-combination checks every dispatch path runs before any
    /// resolution happens.
    fn check_flag_combinations(&self) -> miette::Result<()> {
        self.check_patches_options()?;
        self.check_interactive_peer_options()?;
        self.check_tag_value()?;
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
            && self.selection.depth.is_none_or(|depth| depth == usize::MAX)
            && !update_actions
            && !self.dependency_options.no_optional
            && all_dependency_groups
                .iter()
                .all(|group| include_direct.contains(group))
    }

    fn patch_refresh_dependency_groups(&self, state: &State) -> Vec<DependencyGroup> {
        let prior_included = pnpm_modules_yaml::read_modules_layout::<pnpm_modules_yaml::Host>(
            &state.config.modules_dir,
        )
        .ok()
        .flatten()
        .map(|layout| layout.included);

        let explicit = self.dependency_options.explicit_groups();
        let (prod, dev, optional) = if let Some(included) = prior_included {
            (
                included.dependencies || explicit.prod,
                included.dev_dependencies || explicit.dev,
                !explicit.no_optional
                    && (explicit.optional
                        || (included.optional_dependencies && state.config.optional)),
            )
        } else {
            (
                true,
                !explicit.prod || explicit.dev,
                !explicit.no_optional && (explicit.optional || state.config.optional),
            )
        };

        std::iter::empty()
            .chain(prod.then_some(DependencyGroup::Prod))
            .chain(dev.then_some(DependencyGroup::Dev))
            .chain(optional.then_some(DependencyGroup::Optional))
            .collect()
    }

    fn pnpr_patch_link<'path>(
        &self,
        state: &State,
        lockfile_path: &'path Path,
    ) -> super::install::PnprLink<'path> {
        super::install::PnprLink {
            dependency_groups: self.patch_refresh_dependency_groups(state),
            supported_architectures: self.supported_architectures.apply_to(
                state.config.supported_architectures.clone(),
            ),
            node_linker: state.config.node_linker,
            skip_runtimes: state.config.skip_runtimes,
            lockfile_path: Some(lockfile_path),
            use_state_lockfile: true,
            save_workspace_manifest: !self.save.no_save,
            lockfile: crate::cli_args::install::PnprLockfilePolicy {
                frozen: false,
                prefer_frozen: false,
                update_patches: true,
                update_checksums: false,
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

mod config_dependencies;
mod execution;
