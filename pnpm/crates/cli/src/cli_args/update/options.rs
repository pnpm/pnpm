use crate::cli_args::lockfile_dir::LockfileDirArg;
use clap::Args;
use pnpm_package_manifest::DependencyGroup;

/// The `--prod`, `--dev`, and `--no-optional` flags that select which
/// dependency groups to update.
#[derive(Debug, Clone, Args)]
pub struct UpdateDependencyOptions {
    /// Update packages only in "dependencies" and "optionalDependencies".
    #[clap(short = 'P', long, visible_alias = "production")]
    pub(crate) prod: bool,
    /// Update packages only in "devDependencies".
    #[clap(short = 'D', long)]
    pub(crate) dev: bool,
    /// Update packages only in "optionalDependencies".
    #[clap(long, overrides_with = "no_optional")]
    pub(crate) optional: bool,
    /// Don't update packages in "optionalDependencies".
    #[clap(long, overrides_with = "optional")]
    pub(crate) no_optional: bool,
    /// Also update packages in "peerDependencies".
    #[clap(long)]
    pub(crate) peer: bool,
}

impl UpdateDependencyOptions {
    /// The dependency groups whose direct dependencies the update may
    /// match. Returns the groups for which the corresponding inclusion bit
    /// is set.
    ///
    /// This narrows what the update *matches*, not what the install that
    /// follows it materializes: pnpm leaves the `included` set recorded in
    /// `.modules.yaml` untouched for an update, so these flags never reach
    /// [`pnpm_config::Config::optional`] and friends.
    pub(crate) fn include_direct(&self) -> Vec<DependencyGroup> {
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

    pub(crate) fn explicit_groups(&self) -> pnpm_package_manager::UpdateExplicitGroups {
        pnpm_package_manager::UpdateExplicitGroups {
            prod: self.prod,
            dev: self.dev,
            optional: self.optional,
            no_optional: self.no_optional,
        }
    }
}

#[derive(Debug, Clone, clap::Args)]
pub struct UpdateSelectionArgs {
    /// Ignore version ranges in package.json: bump the matched packages
    /// to their latest version and rewrite the manifest ranges.
    #[clap(short = 'L', long)]
    pub latest: bool,
    /// Ignore version ranges in package.json: bump the matched packages
    /// to the version behind the given dist-tag and rewrite the manifest
    /// ranges.
    #[clap(long, conflicts_with = "latest", value_name = "tag")]
    pub tag: Option<String>,
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
