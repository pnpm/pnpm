use crate::{
    State,
    cli_args::{
        install::resolve_bool_override,
        lockfile_dir::LockfileDirArg,
        pipelines::InstallFamilySelection,
        recursive,
        supported_architectures::SupportedArchitecturesArgs,
        update_interactive::{InteractiveUpdateOptions, UpdatePrompt},
        workspace_option::{WorkspaceOptionError, workspace_link_root},
    },
    github_actions,
};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic};
use pnpm_config::{Config, matcher::Matcher};
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
        let optional = self.optional.then_some(true).or_else(|| self.no_optional.then_some(false));

        let ne_true = |flag: Option<bool>| flag != Some(true);
        let dependencies = production == Some(true) || (ne_true(dev) && ne_true(optional));
        let dev_dependencies = dev == Some(true) || (ne_true(production) && ne_true(optional));
        let optional_dependencies = optional == Some(true) || (ne_true(production) && ne_true(dev));

        std::iter::empty()
            .chain(dependencies.then_some(DependencyGroup::Prod))
            .chain(dev_dependencies.then_some(DependencyGroup::Dev))
            .chain(optional_dependencies.then_some(DependencyGroup::Optional))
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

    /// Ignore version ranges in package.json: bump the matched packages
    /// to their latest version and rewrite the manifest ranges.
    #[clap(short = 'L', long)]
    pub latest: bool,

    /// Refresh registry revisions without changing package versions.
    #[clap(long)]
    pub patches: bool,

    /// Write the resolved version without a range operator when
    /// rewriting the manifest under `--latest`.
    #[clap(short = 'E', long = "save-exact")]
    pub save_exact: bool,

    /// Do not write the updated ranges back to package.json. The
    /// lockfile is still updated (the `--no-save` flag).
    #[clap(long = "no-save")]
    pub no_save: bool,

    /// How deep to inspect dependencies. `0` means top-level
    /// dependencies only. Defaults to unlimited.
    #[clap(long)]
    pub depth: Option<usize>,

    /// Dependencies are not downloaded; only `pnpm-lock.yaml` is updated.
    #[clap(long = "lockfile-only")]
    pub lockfile_only: bool,

    #[clap(flatten)]
    pub lockfile_dir: LockfileDirArg,

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

    /// Generate a changeset file declaring a patch bump for every workspace
    /// package whose production dependencies were changed by the update.
    #[clap(long, overrides_with = "no_changeset")]
    pub changeset: bool,

    /// Do not generate a changeset, even when `updateConfig.changeset` enables
    /// changeset generation by default.
    #[clap(long = "no-changeset", overrides_with = "changeset")]
    pub no_changeset: bool,

    /// Disable pnpm hooks defined in `.pnpmfile.cjs`, including the
    /// pnpmfiles of config dependencies.
    #[clap(long = "ignore-pnpmfile")]
    pub ignore_pnpmfile: bool,

    /// Don't run lifecycle scripts of the project or its dependencies.
    #[clap(long = "ignore-scripts", overrides_with = "no_ignore_scripts")]
    pub ignore_scripts: bool,

    /// Run lifecycle scripts even when the configuration disables them.
    #[clap(long = "no-ignore-scripts", overrides_with = "ignore_scripts")]
    pub no_ignore_scripts: bool,

    /// URL of a pnpr server to offload revision refresh resolution to.
    #[clap(long = "pnpr-server")]
    pub pnpr_server: Option<String>,

    #[clap(skip)]
    pub(crate) prompt: UpdatePrompt,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "--patches cannot be combined with package selectors, --latest, --interactive, or --global"
)]
#[diagnostic(code(ERR_PNPM_PATCHES_WITH_SELECTOR))]
struct PatchesWithSelectorError;

impl UpdateArgs {
    pub(crate) fn apply_cli_config(&self, config: &mut Config) {
        config.ignore_scripts = resolve_bool_override(
            self.ignore_scripts,
            self.no_ignore_scripts,
            config.ignore_scripts,
        );
        config.ignore_pnpmfile = self.ignore_pnpmfile || config.ignore_pnpmfile;
        if let Some(pnpr_server) = self.pnpr_server.clone() {
            config.pnpr_server = Some(pnpr_server);
        }
    }

    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        mut state: State,
    ) -> miette::Result<()> {
        self.check_patches_options()?;
        state.http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
        let workspace_root = self.check_workspace_option(state.config.workspace_dir.as_deref())?;
        let include_direct = self.dependency_options.include_direct();
        let update_actions = self.should_update_github_actions(state.config, &include_direct);
        if let Some(pnpr_server) =
            self.delegated_pnpr_server(state.config, update_actions, &include_direct)
        {
            let lockfile_path = state.lockfile_path();
            return super::install::install_via_pnpr::<Reporter>(
                &state,
                pnpr_server,
                self.pnpr_patch_link(&state, &lockfile_path),
            )
            .await;
        }
        let workspace_packages = workspace_root
            .map(|workspace_root| {
                recursive::discover_workspace_projects(workspace_root, state.config)
                    .map(|(projects, _)| build_workspace_packages_map(Some(&projects)))
            })
            .transpose()?
            .flatten();

        let actions_root =
            state.config.workspace_dir.clone().unwrap_or_else(|| manifest_root(&state.manifest));
        let action_matcher = actions_selector_matcher(update_actions, &self.packages);
        let package_selectors = filter_package_selectors(&self.packages, update_actions);
        if !self.interactive && !self.packages.is_empty() && package_selectors.is_empty() {
            self.update_github_actions::<Reporter>(
                update_actions,
                &actions_root,
                action_matcher.as_ref(),
                state.config,
            )
            .await?;
            return Ok(());
        }

        let lockfile_path = state.lockfile_path();
        let active_importer_id = state.active_importer_id();
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &mut state;
        let lockfile =
            lockfile.get().map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;

        let supported_architectures =
            self.supported_architectures.apply_to(config.supported_architectures.clone());

        let packages = if self.interactive {
            // Nothing outdated, or the user picked nothing — there is
            // nothing to update, so don't fall through to a full update
            // (which an empty selector list would mean).
            let Some(selected) = crate::cli_args::update_interactive::select_packages::<Reporter>(
                &actions_root,
                manifest,
                lockfile,
                &active_importer_id,
                config,
                http_client,
                InteractiveUpdateOptions {
                    latest: self.latest,
                    include_direct: &include_direct,
                    include_github_actions: update_actions,
                    prompt: self.prompt,
                },
            )
            .await?
            else {
                return Ok(());
            };
            selected
        } else {
            package_selectors
        };

        let selected_action_matcher = if self.interactive {
            github_actions::selector_matcher(&packages)
        } else {
            action_matcher
        };
        let package_selectors = filter_package_selectors(&packages, update_actions);
        let run_package_update = self.updates_packages(&package_selectors);

        if run_package_update {
            Update {
                tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
                resolved_packages,
                http_client,
                http_client_arc: std::sync::Arc::clone(http_client),
                config,
                manifest,
                lockfile,
                lockfile_path: Some(&lockfile_path),
                packages: &package_selectors,
                latest: self.latest,
                patches: self.patches,
                save_exact: self.save_exact || config.save_exact,
                save: !self.no_save,
                include_direct,
                depth: self.depth.unwrap_or(usize::MAX),
                workspace_packages: workspace_packages.as_ref(),
                supported_architectures,
                lockfile_only: self.lockfile_only,
                resolution_observer: None,
            }
            .run::<Reporter>()
            .await
            .wrap_err("updating dependencies")?;
        }
        self.update_github_actions::<Reporter>(
            update_actions,
            &actions_root,
            selected_action_matcher.as_ref(),
            config,
        )
        .await?;
        Ok(())
    }

    /// Run the GitHub Actions half of the update, if this run covers it.
    async fn update_github_actions<Reporter: self::Reporter + 'static>(
        &self,
        update_actions: bool,
        actions_root: &Path,
        matcher: Option<&Matcher>,
        config: &Config,
    ) -> miette::Result<()> {
        if !update_actions {
            return Ok(());
        }
        github_actions::update::<Reporter>(
            actions_root,
            self.latest,
            matcher,
            config.update_config.github_actions_server.as_deref(),
        )
        .await?;
        Ok(())
    }

    /// Whether the package half of the update runs. An interactive run that
    /// ended with no package selected updates only workflow files.
    fn updates_packages(&self, package_selectors: &[String]) -> bool {
        !self.interactive || !package_selectors.is_empty()
    }

    /// The pnpr server this run may delegate to, when it has nothing to do
    /// beyond refreshing patches.
    fn delegated_pnpr_server<'config>(
        &self,
        config: &'config Config,
        update_actions: bool,
        include_direct: &[DependencyGroup],
    ) -> Option<&'config str> {
        self.can_delegate_patch_refresh(update_actions, include_direct)
            .then_some(config.pnpr_server.as_deref())
            .flatten()
    }

    pub(crate) async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        mut state: State,
        selection: InstallFamilySelection,
    ) -> miette::Result<()> {
        self.check_patches_options()?;
        state.http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
        let workspace_root = self.check_workspace_option(state.config.workspace_dir.as_deref())?;
        let include_direct = self.dependency_options.include_direct();
        let update_actions = self.should_update_github_actions(state.config, &include_direct);
        if let Some(pnpr_server) =
            self.delegated_pnpr_server(state.config, update_actions, &include_direct)
        {
            let lockfile_path = state.lockfile_path();
            return super::install::install_selected_via_pnpr::<Reporter>(
                &state,
                pnpr_server,
                &selection,
                self.pnpr_patch_link(&state, &lockfile_path),
            )
            .await;
        }
        let workspace_packages =
            workspace_root.and_then(|_| build_workspace_packages_map(Some(&selection.projects)));

        let actions_root = selection.workspace_root.clone();
        let action_matcher = actions_selector_matcher(update_actions, &self.packages);
        let package_selectors = filter_package_selectors(&self.packages, update_actions);
        if !self.interactive && !self.packages.is_empty() && package_selectors.is_empty() {
            self.update_github_actions::<Reporter>(
                update_actions,
                &actions_root,
                action_matcher.as_ref(),
                state.config,
            )
            .await?;
            return Ok(());
        }

        let lockfile_path = state.lockfile_path();
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &mut state;
        let lockfile =
            lockfile.get().map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;
        let supported_architectures =
            self.supported_architectures.apply_to(config.supported_architectures.clone());
        let packages = if self.interactive {
            let Some(selected) =
                crate::cli_args::update_interactive::select_packages_for_projects::<Reporter>(
                    &actions_root,
                    &selection,
                    lockfile,
                    config,
                    http_client,
                    InteractiveUpdateOptions {
                        latest: self.latest,
                        include_direct: &include_direct,
                        include_github_actions: update_actions,
                        prompt: self.prompt,
                    },
                )
                .await?
            else {
                return Ok(());
            };
            selected
        } else {
            package_selectors
        };
        let selected_action_matcher = if self.interactive {
            github_actions::selector_matcher(&packages)
        } else {
            action_matcher
        };
        let package_selectors = filter_package_selectors(&packages, update_actions);
        let run_package_update = self.updates_packages(&package_selectors);
        let InstallFamilySelection {
            workspace_root: _,
            workspace_cycles: _,
            mut projects,
            project_dependencies,
            ordered_dirs,
            selected_dirs,
            install_dirs,
            active_manifest_is_standin,
        } = selection;

        if run_package_update {
            Update {
                tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
                resolved_packages,
                http_client,
                http_client_arc: std::sync::Arc::clone(http_client),
                config,
                manifest,
                lockfile,
                lockfile_path: Some(&lockfile_path),
                packages: &package_selectors,
                latest: self.latest,
                patches: self.patches,
                save_exact: self.save_exact || config.save_exact,
                save: !self.no_save,
                include_direct,
                depth: self.depth.unwrap_or(usize::MAX),
                workspace_packages: workspace_packages.as_ref(),
                supported_architectures,
                lockfile_only: self.lockfile_only,
                resolution_observer: None,
            }
            .run_selected::<Reporter>(pnpm_package_manager::SelectedProjects {
                projects: &mut projects,
                project_dependencies: &project_dependencies,
                ordered_dirs: &ordered_dirs,
                selected_dirs: selected_dirs.as_ref(),
                install_dirs: install_dirs.as_ref(),
                active_manifest_is_standin,
            })
            .await
            .wrap_err("updating dependencies")?;
        }
        self.update_github_actions::<Reporter>(
            update_actions,
            &actions_root,
            selected_action_matcher.as_ref(),
            config,
        )
        .await?;
        Ok(())
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
        let selected_hashes: Option<HashSet<String>> = if self.interactive {
            match crate::cli_args::update_interactive::select_global_package_groups::<Reporter>(
                config,
                &self.packages,
                self.latest,
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
            self.save_exact || config.save_exact,
            config.save_prefix.as_deref(),
        );
        Box::pin(crate::cli_args::global::handle_global_update::<Reporter>(
            config,
            &self.packages,
            selected_hashes.as_ref(),
            self.latest,
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
        if self.workspace && self.latest {
            return Err(WorkspaceOptionError::LatestWithWorkspace.into());
        }
        workspace_link_root(self.workspace, workspace_root)
    }

    fn check_patches_options(&self) -> miette::Result<()> {
        if self.patches
            && (!self.packages.is_empty() || self.latest || self.interactive || self.global)
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
        self.patches
            && self.depth.is_none()
            && !update_actions
            && all_dependency_groups.iter().all(|group| include_direct.contains(group))
    }

    fn pnpr_patch_link<'path>(
        &self,
        state: &State,
        lockfile_path: &'path Path,
    ) -> super::install::PnprLink<'path> {
        super::install::PnprLink {
            dependency_groups: included_direct_groups(state.config.optional).collect(),
            supported_architectures: self
                .supported_architectures
                .apply_to(state.config.supported_architectures.clone()),
            node_linker: state.config.node_linker,
            skip_runtimes: state.config.skip_runtimes,
            frozen_lockfile: false,
            prefer_frozen_lockfile: false,
            update_patches: true,
            fix_lockfile: false,
            lockfile_only: self.lockfile_only,
            ignore_manifest_check: false,
            trust_lockfile: state.config.trust_lockfile,
            lockfile_path: Some(lockfile_path),
            use_state_lockfile: true,
        }
    }

    fn should_update_github_actions(
        &self,
        config: &Config,
        include_direct: &[DependencyGroup],
    ) -> bool {
        include_direct.contains(&DependencyGroup::Dev)
            && !self.no_save
            && !self.lockfile_only
            && github_actions::opted_in(self.include_github_actions, config)
    }
}

fn manifest_root(manifest: &pnpm_package_manifest::PackageManifest) -> std::path::PathBuf {
    manifest.path().parent().expect("manifest path always has a parent directory").to_path_buf()
}

/// The matcher for the workflow selectors, when this run updates
/// workflow files at all.
fn actions_selector_matcher(update_actions: bool, selectors: &[String]) -> Option<Matcher> {
    update_actions.then(|| github_actions::selector_matcher(selectors)).flatten()
}

fn filter_package_selectors(packages: &[String], include_github_actions: bool) -> Vec<String> {
    packages
        .iter()
        .filter(|selector| !include_github_actions || !github_actions::is_selector(selector))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests;
