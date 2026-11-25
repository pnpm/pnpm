//! `pacquet env`: the deprecated Node.js-only front end to
//! [`super::runtime`], kept because pnpm still ships it.

use super::registry_client::build_registry_client;
use clap::Args;
use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use pnpm_config::{
    Config,
    Tool,
};
use pnpm_engine_runtime_node_resolver::{
    get_node_mirror,
    parse_node_specifier,
    resolve_node_versions_with_auth,
};

/// Manage Node.js versions.
#[derive(Debug, Args)]
pub struct EnvArgs {
    /// Compatibility subcommand (`list`) and its arguments.
    pub params: Vec<String>,
}

/// Errors raised by `pacquet env`.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum EnvError {
    #[display("Please specify the subcommand")]
    #[diagnostic(code(ERR_PNPM_ENV_NO_SUBCOMMAND), help("Supported subcommand: list"))]
    NoSubcommand,

    #[display("This subcommand is not known")]
    #[diagnostic(code(ERR_PNPM_ENV_UNKNOWN_SUBCOMMAND), help("Supported subcommand: list"))]
    UnknownSubcommand,
}

/// What [`EnvArgs`] resolved its parameters to.
///
/// The subcommands need different resources (the global config and
/// the install pipeline versus a registry client) so parsing is split
/// from running and the dispatcher picks the path.
#[derive(Debug)]
pub enum EnvSubcommand {
    List { version_spec: Option<String> },
}

impl EnvArgs {
    pub fn subcommand(self) -> Result<EnvSubcommand, EnvError> {
        let Some(subcommand) = self.params.first() else {
            return Err(EnvError::NoSubcommand);
        };
        match subcommand.as_str() {
            "list" | "ls" => Ok(EnvSubcommand::List {
                version_spec: self.params
                    .get(1)
                    .map(|spec| spec.trim())
                    .filter(|spec| !spec.is_empty())
                    .map(ToOwned::to_owned),
            }),
            _ => Err(EnvError::UnknownSubcommand),
        }
    }

    /// Oldest first, so the newest version ends up next to the prompt.
    ///
    /// An absent selector reads as the empty one, which the mirror
    /// resolver treats as `latest`, a bare `pnpm env list` prints the
    /// newest version alone, not the whole index. Verified against the
    /// TypeScript CLI, which passes `''` rather than `undefined`.
    pub async fn run_list(version_spec: Option<String>, config: &Config) -> miette::Result<String> {
        let specifier = parse_node_specifier(version_spec.as_deref().unwrap_or_default())
            .map_err(miette::Report::new)?;
        let channels = config.tool_channel_mirrors(Tool::Node);
        let mirror = get_node_mirror(
            config.tool_mirror(Tool::Node),
            channels.get(&specifier.release_channel).map(String::as_str),
            Some(&config.node_download_mirrors),
            &specifier.release_channel,
        );
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
