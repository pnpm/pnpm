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
    /// The template name (e.g., `vite`, `create-vite`, `@scope/foo`).
    pub name: Option<String>,

    /// Arguments forwarded to the created package.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,

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
        let CreateArgs { name, args, allow_build, shell_mode, cpu, os, libc } = self;
        let name = name.ok_or(CreateError::MissingArgs)?;
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
        dlx_args.run::<Reporter>(dir, config).await
    }
}

#[cfg(test)]
mod tests {
    use super::convert_to_create_name;

    #[test]
    fn unscoped_unprefixed_gets_create_prefix() {
        assert_eq!(convert_to_create_name("foo"), "create-foo");
    }

    #[test]
    fn unscoped_already_prefixed_is_unchanged() {
        assert_eq!(convert_to_create_name("create-foo"), "create-foo");
    }

    #[test]
    fn unscoped_empty_prefix_is_unchanged() {
        assert_eq!(convert_to_create_name("create-"), "create-");
    }

    #[test]
    fn unscoped_underscore_prefix_gets_double_prefix() {
        assert_eq!(convert_to_create_name("create_no_dash"), "create-create_no_dash");
    }

    #[test]
    fn scoped_unprefixed_gets_create_prefix() {
        assert_eq!(convert_to_create_name("@scope/foo"), "@scope/create-foo");
    }

    #[test]
    fn scoped_already_prefixed_is_unchanged() {
        assert_eq!(convert_to_create_name("@scope/create-foo"), "@scope/create-foo");
    }

    #[test]
    fn scoped_empty_prefix_is_unchanged() {
        assert_eq!(convert_to_create_name("@scope/create-"), "@scope/create-");
    }

    #[test]
    fn scoped_underscore_prefix_gets_double_prefix() {
        assert_eq!(convert_to_create_name("@scope/create_no_dash"), "@scope/create-create_no_dash");
    }

    #[test]
    fn plain_scope_gets_create() {
        assert_eq!(convert_to_create_name("@scope"), "@scope/create");
    }

    #[test]
    fn unscoped_with_version() {
        assert_eq!(convert_to_create_name("foo@2.0.0"), "create-foo@2.0.0");
        assert_eq!(convert_to_create_name("foo@latest"), "create-foo@latest");
    }

    #[test]
    fn scoped_with_version() {
        assert_eq!(convert_to_create_name("@scope/foo@2.0.0"), "@scope/create-foo@2.0.0");
    }

    #[test]
    fn scoped_already_prefixed_with_version() {
        assert_eq!(convert_to_create_name("@scope/create-a@2.0.0"), "@scope/create-a@2.0.0");
    }

    #[test]
    fn plain_scope_with_version() {
        assert_eq!(convert_to_create_name("@scope@2.0.0"), "@scope/create@2.0.0");
        assert_eq!(convert_to_create_name("@scope@next"), "@scope/create@next");
    }
}
