use crate::{State, cli_args::supported_architectures::SupportedArchitecturesArgs};
use clap::Args;
use miette::Context;
use pacquet_package_manager::Update;
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;

/// `--prod` / `--dev` / `--no-optional` for `pacquet update`.
///
/// Ports pnpm's
/// [`makeIncludeDependenciesFromCLI`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/commands/src/update/index.ts#L330-L340),
/// which reads the *raw* CLI flags (not the rc-merged config) so an
/// absent flag is `undefined` rather than a default.
#[derive(Debug, Args)]
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
    /// match (pnpm's `includeDirect`). Returns the groups for which the
    /// corresponding inclusion bit is set.
    fn include_direct(&self) -> Vec<DependencyGroup> {
        // `Some(true)` only when the flag was explicitly passed, mirroring
        // pnpm reading `opts.cliOptions` rather than the merged config.
        let production = self.prod.then_some(true);
        let dev = self.dev.then_some(true);
        // pnpm has no positive `--optional` flag for update; `--no-optional`
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

/// `pacquet update` (alias `up` / `upgrade`).
#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Packages to update. Bare names (`foo`, `@scope/bar`), glob
    /// patterns (`@scope/bar-*`), and versioned selectors (`foo@2`) are
    /// accepted. With no arguments, every direct dependency in the
    /// included groups is updated.
    pub packages: Vec<String>,

    /// --prod, --dev, and --no-optional.
    #[clap(flatten)]
    pub dependency_options: UpdateDependencyOptions,

    /// `--cpu` / `--os` / `--libc` overrides for the optional-dep
    /// platform filter.
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
    /// lockfile is still updated. Mirrors pnpm's `--no-save`.
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

    /// Update globally installed packages.
    #[clap(short = 'g', long)]
    pub global: bool,

    /// Tries to link all packages from the workspace, updating versions
    /// to match the workspace packages.
    #[clap(long)]
    pub workspace: bool,
}

impl UpdateArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        mut state: State,
    ) -> miette::Result<()> {
        // Global and workspace-link updates depend on subsystems pacquet
        // hasn't ported yet (global-dir / `@pnpm/global.commands`, and
        // workspace version linking). Refuse rather than silently doing
        // a plain update.
        if self.global {
            return Err(miette::miette!(
                "`pacquet update --global` is not supported yet; global package management has not been ported to pacquet."
            ));
        }
        if self.workspace {
            return Err(miette::miette!(
                "`pacquet update --workspace` is not supported yet; workspace-protocol version linking has not been ported to pacquet."
            ));
        }

        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &mut state;
        let lockfile =
            lockfile.get().map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;

        let supported_architectures =
            self.supported_architectures.apply_to(config.supported_architectures.clone());

        let lockfile_path = manifest
            .path()
            .parent()
            .map(|parent| parent.join(pacquet_lockfile::Lockfile::FILE_NAME));

        let packages = if self.interactive {
            match crate::cli_args::update_interactive::select_packages(
                manifest,
                lockfile,
                config,
                http_client,
                self.latest,
                &self.dependency_options.include_direct(),
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
            self.packages.clone()
        };

        Update {
            tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
            resolved_packages,
            http_client,
            http_client_arc: std::sync::Arc::clone(http_client),
            config,
            manifest,
            lockfile,
            lockfile_path: lockfile_path.as_deref(),
            packages: &packages,
            latest: self.latest,
            save_exact: self.save_exact,
            save: !self.no_save,
            include_direct: self.dependency_options.include_direct(),
            depth: self.depth.unwrap_or(usize::MAX),
            supported_architectures,
            lockfile_only: self.lockfile_only,
        }
        .run::<Reporter>()
        .await
        .wrap_err("updating dependencies")
    }
}

#[cfg(test)]
mod tests;
