use super::LockfileDirArg;

/// One selector an `add` was given.
///
/// A selector a Package URL was rewritten into is marked, because the
/// spelling alone no longer says where it came from, and where it came from
/// decides what it means: `pnpm add node@22.0.0` records a runtime and
/// `pnpm add npm@11.0.0` the project's package manager, while a Package URL
/// names a package in a registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddRequest {
    selector: String,
    from_purl: bool,
}

impl AddRequest {
    /// A selector a Package URL was rewritten into.
    pub(crate) fn from_package_url(selector: String) -> Self {
        Self { selector, from_purl: true }
    }

    /// The selector as the ecosystem's add path reads it.
    #[must_use]
    pub fn selector(&self) -> &str {
        &self.selector
    }

    /// Whether this request may name a package manager or a runtime rather
    /// than a package to install.
    pub(crate) fn may_name_a_tool(&self) -> bool {
        !self.from_purl
    }
}

impl From<&str> for AddRequest {
    /// A selector as the command line carries it, which names a package to
    /// install until the dispatch routes it.
    fn from(selector: &str) -> Self {
        Self { selector: selector.to_string(), from_purl: false }
    }
}

impl std::str::FromStr for AddRequest {
    type Err = std::convert::Infallible;

    fn from_str(selector: &str) -> Result<Self, Self::Err> {
        Ok(selector.into())
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
    /// Add available `@types` packages to `devDependencies` for packages without bundled types.
    #[clap(long = "save-types", overrides_with = "no_save_types", conflicts_with_all = ["global", "config"])]
    pub types: bool,
    /// Do not add `@types` packages automatically.
    #[clap(long = "no-save-types", overrides_with = "types")]
    pub no_save_types: bool,
    /// Saved dependencies will be configured with an exact version rather than using
    /// the default semver range operator.
    #[clap(short = 'E', long = "save-exact")]
    #[clap(id = "save_exact")]
    pub exact: bool,
    /// Save the resolved version with a `~` range prefix. Equivalent to `--save-prefix=~`.
    #[clap(long = "tilde", overrides_with = "save_prefix")]
    #[clap(id = "tilde")]
    pub tilde: bool,
    /// The prefix of the saved version range: `^` (default), `~`, `=` for an explicit exact pin, or empty for a bare exact version.
    #[clap(long = "save-prefix", value_name = "prefix", overrides_with = "tilde")]
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
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "CLI argument group for add install options"
    )
)]
pub struct AddInstallArgs {
    #[clap(flatten)]
    pub dedupe: crate::cli_args::install_options::AutoDedupeArgs,
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
    /// Re-materialize every package slot the lockfile names, relinking
    /// packages an earlier install already materialized. In pnpm v12,
    /// `--force` does not bypass platform compatibility checks unless
    /// configured via `forceIgnoresPlatform: true`; use
    /// `--ignore-platform-checks` to bypass platform checks directly.
    #[clap(long)]
    pub force: bool,
    /// Bypass per-snapshot installability checks (`cpu`, `os`, `libc`,
    /// `engines`) so packages for foreign platforms are materialized instead
    /// of skipped.
    #[clap(long = "ignore-platform-checks")]
    pub ignore_platform_checks: bool,
    /// Re-materialize every package slot, bypassing repeat-install fast
    /// paths, up-to-date checks, and recorded skip sets.
    #[clap(long = "reinstall")]
    pub reinstall: bool,
}
