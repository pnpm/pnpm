use crate::{State, cli_args::add::add_package};
use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_package_manifest::DependencyGroup;
use pacquet_registry::PinnedVersion;
use pacquet_reporter::Reporter;

/// Manage runtimes.
#[derive(Debug, Args)]
pub struct RuntimeArgs {
    /// Install the runtime globally.
    #[clap(short = 'g', long)]
    pub global: bool,

    /// Save the runtime to `devEngines.runtime`. This is the default.
    #[clap(short = 'D', long = "save-dev")]
    pub save_dev: bool,

    /// Save the runtime to `engines.runtime`.
    #[clap(short = 'P', long = "save-prod")]
    pub save_prod: bool,

    /// Runtime subcommand and arguments.
    pub params: Vec<String>,
}

#[derive(Debug, Display, Error, Diagnostic, PartialEq, Eq)]
#[non_exhaustive]
pub enum RuntimeError {
    #[display("Please specify the subcommand")]
    #[diagnostic(code(ERR_PNPM_RUNTIME_NO_SUBCOMMAND))]
    NoSubcommand,

    #[display("Unknown subcommand: {subcommand}")]
    #[diagnostic(code(ERR_PNPM_RUNTIME_UNKNOWN_SUBCOMMAND))]
    UnknownSubcommand {
        #[error(not(source))]
        subcommand: String,
    },

    #[display(
        r#""pnpm runtime set <name> <version>" requires a runtime name (e.g. node, deno, bun)"#
    )]
    #[diagnostic(code(ERR_PNPM_MISSING_RUNTIME_NAME))]
    MissingRuntimeName,

    #[display(
        "`pacquet runtime set --global` is not supported yet; global package management has not been ported to pacquet."
    )]
    #[diagnostic(code(pacquet_cli::runtime_global_unsupported))]
    GlobalUnsupported,
}

#[derive(Debug)]
struct RuntimeSetRequest {
    package_name: String,
    dependency_group: DependencyGroup,
}

impl RuntimeArgs {
    pub fn reject_unsupported_global(&self) -> Result<(), RuntimeError> {
        if self.global {
            return Err(RuntimeError::GlobalUnsupported);
        }
        Ok(())
    }

    /// Execute the subcommand.
    pub async fn run<Reporter: self::Reporter + 'static>(self, state: State) -> miette::Result<()> {
        let request = self.set_request()?;
        add_package::<Reporter, _, _>(
            state,
            &request.package_name,
            PinnedVersion::Major,
            None,
            false,
            None,
            || std::iter::once(request.dependency_group),
        )
        .await
    }

    fn set_request(&self) -> Result<RuntimeSetRequest, RuntimeError> {
        let Some(subcommand) = self.params.first() else {
            return Err(RuntimeError::NoSubcommand);
        };
        if subcommand != "set" {
            return Err(RuntimeError::UnknownSubcommand { subcommand: subcommand.clone() });
        }
        let runtime_name = self
            .params
            .get(1)
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .ok_or(RuntimeError::MissingRuntimeName)?;
        let version_spec = self.params.get(2).map_or("", |version| version.trim());
        let dependency_group = if self.save_dev || !self.save_prod {
            DependencyGroup::Dev
        } else {
            DependencyGroup::Prod
        };
        Ok(RuntimeSetRequest {
            package_name: format!("{runtime_name}@runtime:{version_spec}"),
            dependency_group,
        })
    }
}

#[cfg(test)]
mod tests;
