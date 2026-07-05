use crate::{
    State,
    cli_args::{add::add_package, global::handle_global_add},
};
use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_package_manifest::{DependencyGroup, is_runtime_alias};
use pacquet_registry::PinnedVersion;
use pacquet_reporter::Reporter;
use std::path::Path;

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

    #[display(r#""{name}" is not a supported runtime. Supported runtimes are: node, deno, bun"#)]
    #[diagnostic(code(ERR_PNPM_INVALID_RUNTIME_NAME))]
    InvalidRuntimeName {
        #[error(not(source))]
        name: String,
    },

    #[display(r#"Invalid runtime version "{version}": a version cannot contain a comma"#)]
    #[diagnostic(code(ERR_PNPM_INVALID_RUNTIME_VERSION))]
    InvalidRuntimeVersion {
        #[error(not(source))]
        version: String,
    },
}

#[derive(Debug)]
struct RuntimeSetRequest {
    package_name: String,
    dependency_group: DependencyGroup,
}

impl RuntimeArgs {
    /// Execute the subcommand, installing the runtime into the current
    /// project. Mirrors pnpm's `runtime set`, which runs
    /// `pnpm add <name>@runtime:<version>` in the project directory.
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

    /// `pnpm runtime set <name> <version> -g`: install the runtime into the
    /// global packages directory and link its binary into the global bin
    /// directory. Mirrors pnpm's `runtime set … -g`, which runs
    /// `pnpm add <name>@runtime:<version> --global` against the pnpm home
    /// directory. `--save-dev` / `--save-prod` are ignored for a global
    /// install — like every `pnpm add -g`, the global group always saves to
    /// `dependencies`.
    pub async fn run_global<Reporter: self::Reporter + 'static>(
        self,
        config: &'static Config,
        dir: &Path,
    ) -> miette::Result<()> {
        let request = self.set_request()?;
        Box::pin(handle_global_add::<Reporter>(
            config,
            std::slice::from_ref(&request.package_name),
            PinnedVersion::Major,
            config.supported_architectures.clone(),
            dir,
        ))
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
        // The runtime name is interpolated into an `add` selector, so reject
        // anything that isn't a known runtime before it can be misread as a
        // comma-separated package list or a local path by the global-add
        // pipeline.
        if !is_runtime_alias(runtime_name) {
            return Err(RuntimeError::InvalidRuntimeName { name: runtime_name.to_string() });
        }
        let version_spec = self.params.get(2).map_or("", |version| version.trim());
        // The version is interpolated into the same `<name>@runtime:<version>`
        // selector, which the global-add pipeline splits on commas. Reject a
        // comma so `runtime set node 22,evil -g` can't smuggle in a second
        // install target. No valid runtime version (semver, dist-tag,
        // channel) contains one.
        if version_spec.contains(',') {
            return Err(RuntimeError::InvalidRuntimeVersion { version: version_spec.to_string() });
        }
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
