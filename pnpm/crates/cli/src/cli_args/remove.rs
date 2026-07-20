use crate::{State, cli_args::pipelines::InstallFamilySelection};
use clap::Args;
use miette::Context;
use pacquet_package_manager::Remove;
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;

#[derive(Debug, Args)]
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

#[derive(Debug, Args)]
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
    /// Remove the package from the global packages directory and unlink its
    /// bins.
    #[clap(short = 'g', long)]
    pub global: bool,
}

impl RemoveArgs {
    /// Execute the subcommand.
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        mut state: State,
    ) -> miette::Result<()> {
        let lockfile_path = state.lockfile_path();
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &mut state;
        let lockfile =
            lockfile.get().map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;

        Remove {
            tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: std::sync::Arc::clone(http_client),
            config,
            manifest,
            lockfile,
            lockfile_path: Some(&lockfile_path),
            package_names: &self.package_names,
            save_type: self.dependency_options.save_type(),
            resolved_packages,
            supported_architectures: config.supported_architectures.clone(),
            lockfile_only: self.lockfile_only,
        }
        .run::<Reporter>()
        .await
        .wrap_err("removing a package")
    }

    pub(crate) async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        mut state: State,
        selection: InstallFamilySelection,
    ) -> miette::Result<()> {
        let InstallFamilySelection {
            workspace_root: _,
            mut projects,
            ordered_dirs,
            selected_dirs,
            active_manifest_is_standin,
        } = selection;
        let lockfile_path = state.lockfile_path();
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &mut state;
        let lockfile =
            lockfile.get().map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;

        Remove {
            tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: std::sync::Arc::clone(http_client),
            config,
            manifest,
            lockfile,
            lockfile_path: Some(&lockfile_path),
            package_names: &self.package_names,
            save_type: self.dependency_options.save_type(),
            resolved_packages,
            supported_architectures: config.supported_architectures.clone(),
            lockfile_only: self.lockfile_only,
        }
        .run_selected::<Reporter>(
            &mut projects,
            &ordered_dirs,
            selected_dirs.as_ref(),
            active_manifest_is_standin,
        )
        .await
        .wrap_err("removing a package")
    }
}

#[cfg(test)]
mod tests;
