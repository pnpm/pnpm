use crate::{
    State,
    cli_args::{
        pipelines::InstallFamilySelection, supported_architectures::SupportedArchitecturesArgs,
        update_interactive::InteractiveUpdateOptions,
    },
    github_actions,
};
use clap::Args;
use miette::Context;
use pacquet_config::Config;
use pacquet_package_manager::Update;
use pacquet_package_manifest::DependencyGroup;
use pacquet_registry::PinnedVersion;
use pacquet_reporter::Reporter;

/// The `--prod`, `--dev`, and `--no-optional` flags that select which
/// dependency groups to update.
#[derive(Debug, Clone, Args)]
pub struct UpdateDependencyOptions {
    /// Update packages only in "dependencies" and "optionalDependencies".
    #[clap(short = 'P', long)]
    prod: bool,
    /// Update packages only in "devDependencies".
    #[clap(short = 'D', long)]
    dev: bool,
    /// Don't update packages in "optionalDependencies".
    #[clap(long)]
    no_optional: bool,
}

impl UpdateDependencyOptions {
    /// The dependency groups whose direct dependencies the update may
    /// match. Returns the groups for which the corresponding inclusion bit
    /// is set.
    fn include_direct(&self) -> Vec<DependencyGroup> {
        // `Some(true)` only when the flag was explicitly passed: the raw
        // CLI flags are read rather than the merged config.
        let production = self.prod.then_some(true);
        let dev = self.dev.then_some(true);
        // There is no positive `--optional` flag for update; `--no-optional`
        // sets it to `false`, otherwise it stays unset.
        let optional = self.no_optional.then_some(false);

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
}

/// The `pnpm update --workspace` rejection message. `--workspace` needs
/// workspace-protocol version linking, which has not been ported yet, so
/// every dispatch path — plain, selected, and global — refuses it with the
/// same wording rather than silently doing a plain update.
const WORKSPACE_UPDATE_UNSUPPORTED: &str = "`pnpm update --workspace` is not supported yet.";

impl UpdateArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        mut state: State,
    ) -> miette::Result<()> {
        // Workspace-link updates depend on workspace-protocol version
        // linking, which pacquet hasn't ported yet. Refuse rather than
        // silently doing a plain update. (`--global` is routed to
        // [`Self::run_global`] before `run` is reached.)
        if self.workspace {
            return Err(miette::miette!("{WORKSPACE_UPDATE_UNSUPPORTED}"));
        }

        let actions_root =
            state.config.workspace_dir.clone().unwrap_or_else(|| manifest_root(&state.manifest));
        let include_direct = self.dependency_options.include_direct();
        let update_actions = self.should_update_github_actions(state.config, &include_direct);
        let action_matcher =
            if update_actions { github_actions::selector_matcher(&self.packages) } else { None };
        let package_selectors = filter_package_selectors(&self.packages, update_actions);
        if !self.interactive && !self.packages.is_empty() && package_selectors.is_empty() {
            if update_actions {
                github_actions::update(&actions_root, self.latest, action_matcher.as_ref()).await?;
            }
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
            match crate::cli_args::update_interactive::select_packages(
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
                },
            )
            .await?
            {
                Some(selected) => selected,
                // Nothing outdated, or the user picked nothing — there
                // is nothing to update, so don't fall through to a
                // full update (which an empty selector list would mean).
                None => return Ok(()),
            }
        } else {
            package_selectors
        };

        let selected_action_matcher = if self.interactive {
            github_actions::selector_matcher(&packages)
        } else {
            action_matcher
        };
        let package_selectors = filter_package_selectors(&packages, update_actions);
        let run_package_update = !self.interactive || !package_selectors.is_empty();

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
                save_exact: self.save_exact,
                save: !self.no_save,
                include_direct,
                depth: self.depth.unwrap_or(usize::MAX),
                supported_architectures,
                lockfile_only: self.lockfile_only,
                resolution_observer: None,
            }
            .run::<Reporter>()
            .await
            .wrap_err("updating dependencies")?;
        }
        if update_actions {
            github_actions::update(&actions_root, self.latest, selected_action_matcher.as_ref())
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        mut state: State,
        selection: InstallFamilySelection,
    ) -> miette::Result<()> {
        if self.workspace {
            return Err(miette::miette!("{WORKSPACE_UPDATE_UNSUPPORTED}"));
        }

        let actions_root = selection.workspace_root.clone();
        let include_direct = self.dependency_options.include_direct();
        let update_actions = self.should_update_github_actions(state.config, &include_direct);
        let action_matcher =
            if update_actions { github_actions::selector_matcher(&self.packages) } else { None };
        let package_selectors = filter_package_selectors(&self.packages, update_actions);
        if !self.interactive && !self.packages.is_empty() && package_selectors.is_empty() {
            if update_actions {
                github_actions::update(&actions_root, self.latest, action_matcher.as_ref()).await?;
            }
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
            match crate::cli_args::update_interactive::select_packages_for_projects(
                &actions_root,
                &selection,
                lockfile,
                config,
                http_client,
                InteractiveUpdateOptions {
                    latest: self.latest,
                    include_direct: &include_direct,
                    include_github_actions: update_actions,
                },
            )
            .await?
            {
                Some(selected) => selected,
                None => return Ok(()),
            }
        } else {
            package_selectors
        };
        let selected_action_matcher = if self.interactive {
            github_actions::selector_matcher(&packages)
        } else {
            action_matcher
        };
        let package_selectors = filter_package_selectors(&packages, update_actions);
        let run_package_update = !self.interactive || !package_selectors.is_empty();
        let InstallFamilySelection {
            workspace_root: _,
            mut projects,
            ordered_groups,
            ordered_dirs,
            selected_dirs,
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
                save_exact: self.save_exact,
                save: !self.no_save,
                include_direct,
                depth: self.depth.unwrap_or(usize::MAX),
                supported_architectures,
                lockfile_only: self.lockfile_only,
                resolution_observer: None,
            }
            .run_selected::<Reporter>(
                &mut projects,
                &ordered_groups,
                &ordered_dirs,
                selected_dirs.as_ref(),
                active_manifest_is_standin,
            )
            .await
            .wrap_err("updating dependencies")?;
        }
        if update_actions {
            github_actions::update(&actions_root, self.latest, selected_action_matcher.as_ref())
                .await?;
        }
        Ok(())
    }

    /// `pnpm update -g`: reinstall each matching global package group,
    /// within its existing range or (with `--latest`) to the newest
    /// version. Delegates to [`crate::cli_args::global::handle_global_update`].
    pub async fn run_global<Reporter: self::Reporter + 'static>(
        self,
        config: &'static Config,
    ) -> miette::Result<()> {
        if self.workspace {
            return Err(miette::miette!("{WORKSPACE_UPDATE_UNSUPPORTED}"));
        }
        if self.interactive {
            return Err(miette::miette!(
                "`pnpm update --global --interactive` is not supported yet."
            ));
        }
        let supported_architectures =
            self.supported_architectures.apply_to(config.supported_architectures.clone());
        let pinned_version = PinnedVersion::from_save_options(self.save_exact, None);
        Box::pin(crate::cli_args::global::handle_global_update::<Reporter>(
            config,
            &self.packages,
            self.latest,
            pinned_version,
            supported_architectures,
        ))
        .await
    }

    fn should_update_github_actions(
        &self,
        config: &Config,
        include_direct: &[DependencyGroup],
    ) -> bool {
        include_direct.contains(&DependencyGroup::Dev)
            && !self.no_save
            && !self.lockfile_only
            && (self.interactive
                || self.include_github_actions
                || config.update_config.github_actions == Some(true))
    }
}

fn manifest_root(manifest: &pacquet_package_manifest::PackageManifest) -> std::path::PathBuf {
    manifest.path().parent().expect("manifest path always has a parent directory").to_path_buf()
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
