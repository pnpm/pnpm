use crate::State;
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
    /// and `pnpm-lock.yaml` are updated. Mirrors pnpm's `--lockfile-only`.
    #[clap(long = "lockfile-only")]
    pub lockfile_only: bool,
}

impl RemoveArgs {
    /// Execute the subcommand.
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        mut state: State,
    ) -> miette::Result<()> {
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &mut state;
        let lockfile =
            lockfile.get().map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;

        let lockfile_path = manifest
            .path()
            .parent()
            .map(|parent| parent.join(pacquet_lockfile::Lockfile::FILE_NAME));
        Remove {
            tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: std::sync::Arc::clone(http_client),
            config,
            manifest,
            lockfile,
            lockfile_path: lockfile_path.as_deref(),
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
}

#[cfg(test)]
mod tests;
