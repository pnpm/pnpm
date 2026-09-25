use crate::{
    State,
    cli_args::{
        install::resolve_bool_override, lockfile_dir::LockfileDirArg,
        pipelines::InstallFamilySelection,
    },
    state::command_lockfile,
};
use clap::Args;
use miette::Context;
use pnpm_package_manager::Remove;
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::Reporter;

#[derive(Debug, Clone, Args)]
pub struct RemoveDependencyOptions {
    /// Remove the dependency only from "dependencies".
    #[clap(short = 'P', long)]
    save_prod: bool,
    /// Remove the dependency only from "devDependencies".
    #[clap(short = 'D', long)]
    save_dev: bool,
    /// Remove the dependency only from "optionalDependencies".
    #[clap(short = 'O', long)]
    save_optional: bool,
}

impl RemoveDependencyOptions {
    /// Convert the `--save-*` flags to the targeted [`DependencyGroup`],
    /// or `None` to remove from any field.
    fn save_type(&self) -> Option<DependencyGroup> {
        let &RemoveDependencyOptions { save_prod, save_dev, save_optional } = self;
        if save_dev {
            Some(DependencyGroup::Dev)
        } else if save_optional {
            Some(DependencyGroup::Optional)
        } else if save_prod {
            Some(DependencyGroup::Prod)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct RemoveArgs {
    /// Names of the packages to remove.
    pub package_names: Vec<String>,
    /// --save-prod, --save-dev, --save-optional
    #[clap(flatten)]
    pub dependency_options: RemoveDependencyOptions,
    /// Dependencies are not removed from `node_modules`. Only the manifest
    /// and `pnpm-lock.yaml` are updated.
    #[clap(long = "lockfile-only")]
    pub lockfile_only: bool,
    #[clap(flatten)]
    pub lockfile_dir: LockfileDirArg,
    /// Skip verifying the lockfile against supply-chain policies.
    #[clap(long = "trust-lockfile", overrides_with = "no_trust_lockfile")]
    pub trust_lockfile: bool,
    /// Verify the lockfile against supply-chain policies even when the
    /// configuration trusts it.
    #[clap(long = "no-trust-lockfile", overrides_with = "trust_lockfile")]
    pub no_trust_lockfile: bool,
    /// Remove the package from the global packages directory and unlink its
    /// bins.
    #[clap(short = 'g', long)]
    pub global: bool,
}

impl RemoveArgs {
    pub(crate) fn apply_cli_config(&self, config: &mut pnpm_config::Config) {
        config.trust_lockfile = resolve_bool_override(
            self.trust_lockfile,
            self.no_trust_lockfile,
            config.trust_lockfile,
        );
    }

    fn prepare_remove<'a>(
        &'a self,
        state: &'a mut State,
        lockfile_path: &'a std::path::Path,
    ) -> miette::Result<Remove<'a>> {
        let State {
            tarball_mem_cache,
            http_client,
            config,
            manifest,
            lockfile,
            resolved_packages,
        } = state;
        let lockfile = command_lockfile(lockfile, lockfile_path)?;

        Ok(Remove {
            manifest,
            options: pnpm_package_manager::RemoveOptions {
                http_client,
                config,
                lockfile,
                package_names: &self.package_names,
                save_type: self.dependency_options.save_type(),
                resolved_packages,
                lockfile_only: self.lockfile_only,
            },
            resources: pnpm_package_manager::RemoveResources {
                tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
                http_client_arc: std::sync::Arc::clone(http_client),
                supported_architectures: config.supported_architectures.clone(),
            },
        })
    }

    /// Execute the subcommand.
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        mut state: State,
    ) -> miette::Result<()> {
        let lockfile_path = state.lockfile_path();
        self.prepare_remove(&mut state, &lockfile_path)?
            .run::<Reporter>()
            .await
            .wrap_err("removing a package")
    }

    pub(crate) async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        mut state: State,
        mut selection: InstallFamilySelection,
    ) -> miette::Result<()> {
        let lockfile_path = state.lockfile_path();
        self.prepare_remove(&mut state, &lockfile_path)?
            .run_selected::<Reporter>(pnpm_package_manager::SelectedProjects {
                projects: &mut selection.projects,
                project_dependencies: &selection.project_dependencies,
                ordered_dirs: &selection.ordered_dirs,
                selected_dirs: selection.selected_dirs.as_ref(),
                install_dirs: selection.install_dirs.as_ref(),
                active_manifest_is_standin: selection.active_manifest_is_standin,
            })
            .await
            .wrap_err("removing a package")
    }
}

#[cfg(test)]
mod tests;
