//! `pacquet env` — the deprecated Node.js-only front end to
//! [`super::runtime`], kept because pnpm still ships it.

use super::{global::handle_global_add, registry_client::build_registry_client};
use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_engine_runtime_node_resolver::{
    get_node_mirror, parse_node_specifier, resolve_node_versions_with_auth,
};
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::{Reporter, emit_global_warning};
use std::path::Path;

/// Manage Node.js versions.
#[derive(Debug, Args)]
pub struct EnvArgs {
    /// Manage Node.js versions globally.
    #[clap(short = 'g', long)]
    pub global: bool,

    /// Accepted for surface parity with pnpm, which declares the option
    /// but never reads it.
    #[clap(long, hide = true)]
    pub remote: bool,

    /// Subcommand (`use`, `list`) and its arguments.
    pub params: Vec<String>,
}

/// Emitted before `env use` does anything else, matching where pnpm warns.
const DEPRECATION_WARNING: &str =
    r#""pnpm env use" is deprecated. Use "pnpm runtime set node <version> -g" instead."#;

/// Errors raised by `pacquet env`.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum EnvError {
    #[display("Please specify the subcommand")]
    #[diagnostic(code(ERR_PNPM_ENV_NO_SUBCOMMAND), help("Supported subcommands are: use, list"))]
    NoSubcommand,

    #[display("This subcommand is not known")]
    #[diagnostic(
        code(ERR_PNPM_ENV_UNKNOWN_SUBCOMMAND),
        help("Supported subcommands are: use, list")
    )]
    UnknownSubcommand,

    #[display(
        "Unable to manage Node.js because pnpm was not installed using the standalone installation script"
    )]
    #[diagnostic(
        code(ERR_PNPM_CANNOT_MANAGE_NODE),
        help(
            "If you want to manage Node.js with pnpm, you need to remove any Node.js that was installed by other tools, then install pnpm using one of the standalone scripts that are provided on the installation page: https://pnpm.io/installation"
        )
    )]
    CannotManageNode,

    #[display(r#""pnpm env use <version>" can only be used with the "--global" option currently"#)]
    #[diagnostic(code(ERR_PNPM_NOT_IMPLEMENTED_YET))]
    LocalUseUnsupported,

    #[display(r#""pnpm env use --global <version>" requires a Node.js version to be specified"#)]
    #[diagnostic(code(ERR_PNPM_MISSING_NODE_VERSION))]
    MissingNodeVersion,
}

/// What [`EnvArgs`] resolved its parameters to.
///
/// The two subcommands need different resources — the global config and
/// the install pipeline versus a registry client — so parsing is split
/// from running and the dispatcher picks the path.
#[derive(Debug)]
pub enum EnvSubcommand {
    Use { package_name: String },
    List { version_spec: Option<String> },
}

impl EnvArgs {
    /// Classify the subcommand, applying the checks pnpm runs before it
    /// dispatches.
    pub fn subcommand<Reporter: self::Reporter>(
        self,
        config: &Config,
    ) -> Result<EnvSubcommand, EnvError> {
        let Some(subcommand) = self.params.first() else {
            return Err(EnvError::NoSubcommand);
        };
        if self.global && config.global_bin.is_none() {
            return Err(EnvError::CannotManageNode);
        }
        match subcommand.as_str() {
            "use" => {
                emit_global_warning::<Reporter>(DEPRECATION_WARNING);
                if !self.global {
                    return Err(EnvError::LocalUseUnsupported);
                }
                let version = self
                    .params
                    .get(1)
                    .map(|version| version.trim())
                    .filter(|version| !version.is_empty())
                    .ok_or(EnvError::MissingNodeVersion)?;
                Ok(EnvSubcommand::Use { package_name: format!("node@runtime:{version}") })
            }
            "list" | "ls" => Ok(EnvSubcommand::List {
                version_spec: self
                    .params
                    .get(1)
                    .map(|spec| spec.trim())
                    .filter(|spec| !spec.is_empty())
                    .map(ToOwned::to_owned),
            }),
            _ => Err(EnvError::UnknownSubcommand),
        }
    }

    /// Installs the runtime the same way
    /// [`super::runtime::RuntimeArgs::run_global`] does, so the deprecated
    /// spelling and its replacement cannot diverge.
    pub async fn run_use<Reporter: self::Reporter + 'static>(
        package_name: String,
        config: &'static Config,
        dir: &Path,
    ) -> miette::Result<()> {
        Box::pin(handle_global_add::<Reporter>(
            config,
            std::slice::from_ref(&package_name),
            RangeSpecStyle::Major,
            config.supported_architectures.clone(),
            // A runtime install has no user packages, so no `--allow-build`.
            &[],
            dir,
        ))
        .await
    }

    /// Oldest first, so the newest version ends up next to the prompt.
    ///
    /// An absent selector reads as the empty one, which the mirror
    /// resolver treats as `latest` — a bare `pnpm env list` prints the
    /// newest version alone, not the whole index. Verified against the
    /// TypeScript CLI, which passes `''` rather than `undefined`.
    pub async fn run_list(version_spec: Option<String>, config: &Config) -> miette::Result<String> {
        let specifier = parse_node_specifier(version_spec.as_deref().unwrap_or_default())
            .map_err(miette::Report::new)?;
        let mirror =
            get_node_mirror(Some(&config.node_download_mirrors), &specifier.release_channel);
        let http_client = build_registry_client(config)?;
        let mut versions = resolve_node_versions_with_auth(
            &http_client,
            &config.auth_headers,
            Some(specifier.version_specifier.as_str()),
            Some(&mirror),
        )
        .await
        .map_err(miette::Report::new)?;
        versions.reverse();
        Ok(versions.join("\n"))
    }
}

#[cfg(test)]
mod tests;
