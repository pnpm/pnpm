use self::unset::{
    ensure_project_runtime_matches,
    run_unset,
};
use crate::{
    State,
    cli_args::{
        add::{
            AddRequest,
            add_package,
        },
        global::handle_global_add,
        remove::RemoveArgs,
    },
};
use clap::Args;
use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use pnpm_config::{
    Config,
    GlobalShims,
};
use pnpm_package_manifest::{
    DependencyGroup,
    is_runtime_alias,
};
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::{
    LogEvent,
    LogLevel,
    PnpmLog,
    Reporter,
};
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

    /// Subcommand and its arguments: `set <name> [<version>]` or
    /// `unset <name> [<version>]`, where `<name>` is `node`, `deno`, or `bun`;
    /// `use <version>` aliases `set node <version>`.
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

    #[display("Unexpected argument: {argument}")]
    #[diagnostic(code(ERR_PNPM_RUNTIME_UNEXPECTED_ARGUMENT))]
    UnexpectedArgument {
        #[error(not(source))]
        argument: String,
    },

    #[display(r#"Cannot unset "{name}" version {version}: no matching runtime is declared in the project"#)]
    #[diagnostic(code(ERR_PNPM_RUNTIME_NOT_DECLARED))]
    RuntimeNotDeclared {
        #[error(not(source))]
        name: String,
        #[error(not(source))]
        version: String,
    },

    #[display(r#"Cannot unset "{name}": "{name}" is not declared as a runtime in the project"#)]
    #[diagnostic(code(ERR_PNPM_RUNTIME_NOT_DECLARED))]
    NotARuntimeDependency {
        #[error(not(source))]
        name: String,
    },
}

#[derive(Debug)]
struct RuntimeSetRequest {
    runtime_name: String,
    package_name: String,
    dependency_group: DependencyGroup,
}

#[derive(Debug)]
struct RuntimeUnsetRequest {
    runtime_name: String,
    version_spec: Option<String>,
    dependency_group: DependencyGroup,
}

#[derive(Debug)]
enum RuntimeCommand {
    Set(RuntimeSetRequest),
    Unset(RuntimeUnsetRequest),
}

impl RuntimeArgs {
    /// Execute the subcommand in the current project.
    pub async fn run<Reporter: self::Reporter + 'static>(self, state: State) -> miette::Result<()> {
        match self.command()? {
            RuntimeCommand::Set(request) => Self::run_set::<Reporter>(state, request).await,
            RuntimeCommand::Unset(request) => {
                ensure_project_runtime_matches(&state, &request)?;
                RemoveArgs::for_runtime(request.runtime_name, request.dependency_group)
                    .run::<Reporter>(state)
                    .await
            }
        }
    }

    /// Execute the subcommand against the global packages directory.
    pub async fn run_global<Reporter: self::Reporter + 'static>(
        self,
        config: &'static Config,
        dir: &Path,
    ) -> miette::Result<()> {
        match self.command()? {
            RuntimeCommand::Set(request) => {
                Self::run_global_set::<Reporter>(config, dir, request).await
            }
            RuntimeCommand::Unset(request) => run_unset::<Reporter>(&request, config),
        }
    }

    async fn run_set<Reporter: self::Reporter + 'static>(
        state: State,
        request: RuntimeSetRequest,
    ) -> miette::Result<()> {
        let config = state.config;
        let prefix = state.manifest
            .path()
            .parent()
            .expect("manifest path has a parent")
            .to_path_buf();
        add_package::<Reporter, _>(
            state,
            &request.package_name,
            RangeSpecStyle::Major,
            None,
            false,
            None,
            [request.dependency_group],
        )
        .await?;
        if let Some(message) = runtime_shim_hint(
            config,
            &request.runtime_name,
            &crate::shim_dispatch::global_shims_setting(),
        ) {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Info,
                message,
                prefix: prefix.to_string_lossy().into_owned(),
            }));
        }
        Ok(())
    }

    async fn run_global_set<Reporter: self::Reporter + 'static>(
        config: &'static Config,
        dir: &Path,
        request: RuntimeSetRequest,
    ) -> miette::Result<()> {
        let global_request = AddRequest::from(request.package_name.as_str());
        Box::pin(handle_global_add::<Reporter>(
            config,
            std::slice::from_ref(&global_request),
            RangeSpecStyle::Major,
            config.supported_architectures.clone(),
            // `pnpm runtime` installs a runtime, not user packages; it has
            // no `--allow-build`.
            &[],
            dir,
        ))
        .await
    }

    fn command(&self) -> Result<RuntimeCommand, RuntimeError> {
        let Some(subcommand) = self.params.first() else {
            return Err(RuntimeError::NoSubcommand);
        };
        match subcommand.as_str() {
            "set" => Ok(RuntimeCommand::Set(self.set_request()?)),
            "unset" => Ok(RuntimeCommand::Unset(self.unset_request()?)),
            _ => Err(RuntimeError::UnknownSubcommand { subcommand: subcommand.clone() }),
        }
    }

    fn set_request(&self) -> Result<RuntimeSetRequest, RuntimeError> {
        let Some(subcommand) = self.params.first() else {
            return Err(RuntimeError::NoSubcommand);
        };
        if subcommand != "set" && subcommand != "use" {
            return Err(RuntimeError::UnknownSubcommand { subcommand: subcommand.clone() });
        }
        let (runtime_name, version_index) = if subcommand == "use" {
            ("node", 1)
        } else {
            let runtime_name = self.params
                .get(1)
                .map(|name| name.trim())
                .filter(|name| !name.is_empty())
                .ok_or(RuntimeError::MissingRuntimeName)?;
            (runtime_name, 2)
        };
        // The runtime name is interpolated into an `add` selector, so reject
        // anything that isn't a known runtime before it can be misread as a
        // comma-separated package list or a local path by the global-add
        // pipeline.
        if !is_runtime_alias(runtime_name) {
            return Err(RuntimeError::InvalidRuntimeName { name: runtime_name.to_string() });
        }
        let version_spec = self.params.get(version_index).map_or("", |version| version.trim());
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
            runtime_name: runtime_name.to_string(),
            package_name: format!("{runtime_name}@runtime:{version_spec}"),
            dependency_group,
        })
    }

    fn unset_request(&self) -> Result<RuntimeUnsetRequest, RuntimeError> {
        let Some(runtime_name) = self.params
            .get(1)
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
        else {
            return Err(RuntimeError::MissingRuntimeName);
        };
        if !is_runtime_alias(runtime_name) {
            return Err(RuntimeError::InvalidRuntimeName { name: runtime_name.to_string() });
        }
        let version_spec = self.params
            .get(2)
            .map(|version| version.trim())
            .filter(|version| !version.is_empty())
            .map(str::to_string);
        if let Some(version) = version_spec.as_deref()
            && version.contains(',')
        {
            return Err(RuntimeError::InvalidRuntimeVersion { version: version.to_string() });
        }
        if let Some(argument) = self.params.get(3) {
            return Err(RuntimeError::UnexpectedArgument { argument: argument.clone() });
        }
        let dependency_group = if self.save_dev || !self.save_prod {
            DependencyGroup::Dev
        } else {
            DependencyGroup::Prod
        };
        Ok(RuntimeUnsetRequest {
            runtime_name: runtime_name.to_string(),
            version_spec,
            dependency_group,
        })
    }
}

fn runtime_shim_hint(
    config: &Config,
    runtime_name: &str,
    global_shims: &GlobalShims,
) -> Option<String> {
    if !global_shims.is_enabled(runtime_name)
        || project_aware_runtime_shim_exists(config, runtime_name)
    {
        return None;
    }
    let setup = if config.global_bin.is_none() { r#"run "pnpm setup", then "# } else { "" };
    Some(format!(
        r#"To make the bare "{runtime_name}" command project-aware, {setup}run "pnpm shim add {runtime_name}"."#,
    ))
}

fn project_aware_runtime_shim_exists(config: &Config, runtime_name: &str) -> bool {
    let Some(bin_dir) = config.global_bin.as_deref() else {
        return false;
    };
    crate::shim_dispatch::native_shim_is_installed(bin_dir, runtime_name)
        || crate::shim_dispatch::is_legacy_context_aware_shim(&bin_dir.join(runtime_name))
}

mod unset;

#[cfg(test)]
mod tests;
