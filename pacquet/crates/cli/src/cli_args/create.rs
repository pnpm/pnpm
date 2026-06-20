use crate::cli_args::dlx::DlxArgs;
use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_reporter::Reporter;
use std::path::Path;

/// Creates a project from a `create-*` starter kit.
///
/// Ports pnpm's `create` command from
/// <https://github.com/pnpm/pnpm/blob/3687b0e180/exec/commands/src/create.ts>.
/// The handler converts the user-provided name to a `create-*` package name
/// and delegates to the existing `dlx` infrastructure.
#[derive(Debug, Args)]
pub struct CreateArgs {
    /// The template name (e.g., `vite`, `create-vite`, `@scope/foo`),
    /// followed by any arguments forwarded to the created package.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,

    /// Package names allowed to run lifecycle (build) scripts during
    /// the install. May be repeated.
    #[clap(long = "allow-build")]
    pub allow_build: Vec<String>,

    /// Run the command inside of a shell. Uses `/bin/sh` on UNIX and
    /// `cmd.exe` on Windows.
    #[clap(long, short = 'c')]
    pub shell_mode: bool,

    /// CPU architectures whose platform-tagged optional dependencies the
    /// install should keep. Repeat or comma-separate for multiple.
    #[clap(long, value_delimiter = ',')]
    pub cpu: Vec<String>,

    /// Operating systems whose platform-tagged optional dependencies the
    /// install should keep.
    #[clap(long, value_delimiter = ',')]
    pub os: Vec<String>,

    /// libc families (`glibc`, `musl`) whose platform-tagged optional
    /// dependencies the install should keep.
    #[clap(long, value_delimiter = ',')]
    pub libc: Vec<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum CreateError {
    #[display(
        "Missing the template package name.\nThe correct usage is `pacquet create <name>` with <name> substituted for a package name."
    )]
    #[diagnostic(code(ERR_PNPM_MISSING_ARGS))]
    MissingArgs,
}

const CREATE_PREFIX: &str = "create-";

/// Resolves the npm package name for `create-*` packages.
///
/// Mirrors the naming algorithm in pnpm's `convertToCreateName`
/// (<https://github.com/pnpm/pnpm/blob/3687b0e180/exec/commands/src/create.ts#L80-L98>).
pub fn convert_to_create_name(package_name: &str) -> String {
    if let Some(rest) = package_name.strip_prefix('@') {
        let preferred_version_position = rest.find('@');
        let (without_version, preferred_version) = match preferred_version_position {
            Some(pos) => (&rest[..pos], &rest[pos..]),
            None => (rest, ""),
        };
        let (scope, scoped_package) = match without_version.split_once('/') {
            Some((scope, pkg)) => (scope, Some(pkg)),
            None => (without_version, None),
        };

        match scoped_package {
            Some("") | None => format!("@{scope}/create{preferred_version}"),
            Some(pkg) => format!("@{scope}/{}{preferred_version}", ensure_create_prefixed(pkg)),
        }
    } else {
        ensure_create_prefixed(package_name)
    }
}

fn ensure_create_prefixed(package_name: &str) -> String {
    if package_name.starts_with(CREATE_PREFIX) {
        package_name.to_string()
    } else {
        format!("{CREATE_PREFIX}{package_name}")
    }
}

impl CreateArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        dir: &Path,
        config: &'static mut Config,
    ) -> miette::Result<()> {
        let CreateArgs { command, allow_build, shell_mode, cpu, os, libc } = self;
        let mut command_iter = command.into_iter();
        let name = command_iter.next().ok_or(CreateError::MissingArgs)?;
        let args: Vec<String> = command_iter.collect();
        let create_name = convert_to_create_name(&name);
        let dlx_args = DlxArgs {
            command: std::iter::once(create_name).chain(args).collect(),
            package: vec![],
            allow_build,
            shell_mode,
            cpu,
            os,
            libc,
        };
        config.strict_dep_builds = false;
        dlx_args.run::<Reporter>(dir, config).await
    }
}

#[cfg(test)]
mod tests;
