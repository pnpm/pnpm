use super::LockfileDirArg;

/// The packages an `add` was asked for.
#[derive(Debug, Clone, clap::Args)]
pub struct AddRequests {
    /// Names of the packages to add.
    #[clap(required = true)]
    pub package_names: Vec<String>,
    /// The members of `package_names` a Package URL was rewritten into.
    /// The command line carries none of these: the dispatch fills them in
    /// when it routes the selectors it was given.
    #[clap(skip)]
    pub(crate) purl_selectors: Vec<String>,
}

impl AddRequests {
    /// One package to install, for a command that builds its own request
    /// instead of parsing one, such as `pnpm runtime use`.
    pub(crate) fn one(package_name: String) -> Self {
        Self { package_names: vec![package_name], purl_selectors: Vec::new() }
    }

    /// Whether `request` may name a package manager or a runtime rather
    /// than a package to install.
    ///
    /// A Package URL names a package in a registry, so the selector it was
    /// rewritten into keeps naming one even when it reads like a request
    /// for the tool that shares its name.
    pub(crate) fn may_name_a_tool(&self, request: &str) -> bool {
        !self.purl_selectors
            .iter()
            .any(|purl_selector| purl_selector == request)
    }
}

/// Which dependency groups the install that follows the manifest edit
/// materializes.
///
/// `pnpm install <pkg>` is a spelling of `pnpm add <pkg>`, so `add` has to
/// take the `--prod` / `--dev` filter `install` takes. Long-only: on `add`,
/// `-P` and `-D` are `--save-prod` and `--save-dev`, as they are in pnpm.
#[derive(Debug, Clone, clap::Args)]
pub struct AddIncludeArgs {
    /// Leave devDependencies out of `node_modules`.
    #[clap(long, visible_alias = "production")]
    pub prod: bool,
    /// Leave dependencies and optionalDependencies out of `node_modules`.
    #[clap(long)]
    pub dev: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct AddSaveArgs {
    /// Saved dependencies will be configured with an exact version rather than using
    /// the default semver range operator.
    #[clap(short = 'E', long = "save-exact")]
    #[clap(id = "save_exact")]
    pub exact: bool,
    /// The prefix of the saved version range: `^` (default), `~`, `=` for an explicit exact pin, or empty for a bare exact version.
    #[clap(long = "save-prefix", value_name = "prefix")]
    #[clap(id = "save_prefix")]
    pub prefix: Option<String>,
    /// Save the new dependency to the default catalog. Shorthand for `--save-catalog-name=default`.
    #[clap(long = "save-catalog")]
    #[clap(id = "save_catalog")]
    pub catalog: bool,
    /// Save the new dependency to the named catalog `<name>`.
    #[clap(long = "save-catalog-name", value_name = "name")]
    #[clap(id = "save_catalog_name")]
    pub catalog_name: Option<String>,
}

#[derive(Debug, Clone, clap::Args)]
pub struct AddTargetArgs {
    /// Add the package as a configuration dependency.
    #[clap(long = "config")]
    pub config: bool,
    /// Only add the dependency if a workspace project provides it. The
    /// dependency is saved under the `workspace:` protocol and linked to
    /// that project.
    #[clap(long)]
    pub workspace: bool,
    /// Install the package globally, linking its bins into the global bin directory.
    #[clap(short = 'g', long)]
    pub global: bool,
    /// Permit adding dependencies to a multi-package workspace root without `-w`.
    #[clap(
        long = "ignore-workspace-root-check",
        overrides_with = "no_ignore_workspace_root_check"
    )]
    pub ignore_workspace_root_check: bool,
    /// Keep the workspace-root safety check enabled.
    #[clap(
        long = "no-ignore-workspace-root-check",
        hide = true,
        overrides_with = "ignore_workspace_root_check"
    )]
    pub no_ignore_workspace_root_check: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct AddInstallArgs {
    /// Package names allowed to run lifecycle (build) scripts during this
    /// install, appended to `allowBuilds`. Prefix a name with `!` to deny
    /// its scripts instead. May be repeated.
    #[clap(long = "allow-build")]
    pub allow_build: Vec<String>,
    /// Dependencies are not downloaded. Only `pnpm-lock.yaml` is updated.
    #[clap(long = "lockfile-only")]
    pub lockfile_only: bool,
    #[clap(flatten)]
    pub lockfile_dir: LockfileDirArg,
    /// Include optionalDependencies while materializing the updated project.
    #[clap(long, overrides_with = "no_optional")]
    pub optional: bool,
    /// Exclude optionalDependencies while materializing the updated project.
    #[clap(long = "no-optional", overrides_with = "optional")]
    pub no_optional: bool,
    /// Reinstall every package the lockfile names: relink packages an
    /// earlier install already materialized, and install optional
    /// dependencies whose `cpu` / `os` / `libc` / `engines` don't match
    /// the host instead of skipping them.
    #[clap(long)]
    pub force: bool,
}
