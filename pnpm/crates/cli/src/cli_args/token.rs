mod registry;
#[cfg(test)]
mod tests;

use crate::{
    cli_args::registry_client::build_registry_client_with_guard, confirm_prompt::stdin_is_terminal,
};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Diagnostic, IntoDiagnostic};
use pnpm_auth_commands::login::{Host, PromptPassword};
use pnpm_config::Config;
use pnpm_network::{RetryOpts, ThrottledClient, normalize_registry_url, origins_redirect_guard};
use registry::{CreateTokenRequest, TokenItem, create_token, delete_token, fetch_token_list};
use std::fmt::Write as _;

pub(crate) const TOKEN_BODY_LIMIT: usize = 1024 * 1024;

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum TokenError {
    #[display("You must be logged in to view or change your tokens")]
    #[diagnostic(code(ERR_PNPM_TOKEN_UNAUTHORIZED))]
    Unauthorized,

    #[display("Unknown token command: {command}")]
    #[diagnostic(code(ERR_PNPM_TOKEN_UNKNOWN_COMMAND))]
    UnknownCommand {
        #[error(not(source))]
        command: String,
    },

    #[display("Token key or id is required to revoke")]
    #[diagnostic(code(ERR_PNPM_TOKEN_KEY_REQUIRED))]
    KeyRequired,

    #[display("Found multiple tokens matching \"{id}\". Specify the full token or key instead.")]
    #[diagnostic(code(ERR_PNPM_TOKEN_AMBIGUOUS))]
    Ambiguous {
        #[error(not(source))]
        id: String,
    },

    #[display("No token matching \"{id}\" found.")]
    #[diagnostic(code(ERR_PNPM_TOKEN_NOT_FOUND))]
    NotFound {
        /// The argument, cut by [`redact_token_argument`].
        #[error(not(source))]
        id: String,
    },

    #[display("Failed to read the password: {reason}")]
    #[diagnostic(code(ERR_PNPM_TOKEN_PASSWORD_READ_FAILED))]
    PasswordReadFailed {
        #[error(not(source))]
        reason: String,
    },

    #[display("Failed to {action}: {status} {status_text}. {body}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    RegistryWriteFailed {
        #[error(not(source))]
        action: String,
        status: u16,
        #[error(not(source))]
        status_text: String,
        #[error(not(source))]
        body: String,
    },

    #[display("Failed to {operation}: {reason}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    RegistryOperationFailed {
        operation: &'static str,
        #[error(not(source))]
        reason: String,
    },
}

#[derive(Debug, Default, Args)]
pub struct TokenArgs {
    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,

    /// One-time password for registries that require two-factor authentication.
    #[clap(long)]
    pub otp: Option<String>,

    /// Output results in JSON format.
    #[clap(long)]
    pub json: bool,

    /// Output results in tab-separated format.
    #[clap(long, short = 'p')]
    pub parseable: bool,

    /// Mark a created token as read-only.
    #[clap(long)]
    pub read_only: bool,

    /// Restrict a created token to these CIDR ranges.
    #[clap(long, value_delimiter = ',')]
    pub cidr: Vec<String>,

    /// The subcommand (`list`, `revoke`, or `create`) and its arguments.
    pub params: Vec<String>,
}

pub(crate) fn generate_token_ids(tokens: &mut [TokenItem]) {
    let min_len = 6;
    for i in 0..tokens.len() {
        let key = &tokens[i].key;
        let mut chosen = key.clone();
        for len in min_len..=key.len() {
            let Some(prefix) = key.get(..len) else { continue };
            let has_collision = tokens
                .iter()
                .enumerate()
                .any(|(j, other)| j != i && other.key.starts_with(prefix));
            if !has_collision {
                chosen = prefix.to_string();
                break;
            }
        }
        tokens[i].id = Some(chosen);
    }
}

fn format_single_token(token: &TokenItem) -> String {
    let created = token.created
        .as_deref()
        .map_or("", |created_at| {
            created_at
                .get(..10)
                .unwrap_or(created_at)
        });
    let id = token.id.as_deref().unwrap_or(&token.key);
    let name_part = match &token.name {
        Some(name) if !name.is_empty() => format!(" name {name}"),
        _ => String::new(),
    };
    let mut item = format!("Token {}… with id {id}{name_part} created {created}", token.token);
    if let Some(cidrs) = &token.cidr_whitelist
        && !cidrs.is_empty()
    {
        let _ = write!(item, "\nwith IP whitelist: {}", cidrs.join(","));
    }
    item
}

fn format_token_list(tokens: &[TokenItem]) -> String {
    let mut entries = Vec::new();
    for token in tokens {
        entries.push(format_single_token(token));
    }
    entries.join("\n\n")
}

fn format_token_list_parseable(tokens: &[TokenItem]) -> String {
    let mut lines = Vec::new();
    lines.push("key\ttoken\tid\tname\tcreated\treadonly\tCIDR whitelist".to_string());
    for token in tokens {
        let id = token.id.as_deref().unwrap_or(&token.key);
        let name = token.name.as_deref().unwrap_or("");
        let created = token.created.as_deref().unwrap_or("");
        let readonly = if token.readonly { "true" } else { "false" };
        let cidrs = token.cidr_whitelist
            .as_ref()
            .map(|whitelist| whitelist.join(","))
            .unwrap_or_default();
        lines.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            token.key, token.token, id, name, created, readonly, cidrs,
        ));
    }
    lines.join("\n")
}

fn resolve_key_to_remove(id: &str, tokens: &[TokenItem]) -> miette::Result<String> {
    let matches: Vec<&TokenItem> = tokens
        .iter()
        .filter(|tok| tok.key.starts_with(id))
        .collect();
    if matches.len() == 1 {
        Ok(matches[0].key.clone())
    } else if matches.len() > 1 {
        Err(TokenError::Ambiguous { id: id.to_string() }.into())
    } else if tokens
        .iter()
        .any(|tok| !tok.token.is_empty() && id.starts_with(&tok.token))
    {
        Ok(id.to_string())
    } else {
        Err(TokenError::NotFound { id: redact_token_argument(id) }.into())
    }
}

/// Shorten a `revoke` argument for diagnostics, because it may be a full,
/// still-valid token.
pub(crate) fn redact_token_argument(id: &str) -> String {
    const VISIBLE_CHARS: usize = 8;
    match id.char_indices().nth(VISIBLE_CHARS) {
        Some((cut, _)) => format!("{}…", &id[..cut]),
        None => id.to_string(),
    }
}

async fn read_password() -> miette::Result<String> {
    tokio::task::spawn_blocking(|| -> Result<String, String> {
        if stdin_is_terminal().map_err(|err| err.to_string())? {
            return Host::prompt_password("Password:").map_err(|err| err.to_string());
        }
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).map_err(|err| err.to_string())?;
        Ok(line
            .trim_end_matches(['\r', '\n'])
            .to_string())
    })
    .await
    .map_err(|err| err.to_string())
    .flatten()
    .map_err(|reason| TokenError::PasswordReadFailed { reason }.into())
}

impl TokenArgs {
    pub async fn run(&self, config: &Config) -> miette::Result<Option<String>> {
        let registry_url = self.registry.as_deref().unwrap_or(&config.registry);
        let normalized = normalize_registry_url(registry_url);
        let auth_header =
            config.auth_headers.for_url(&normalized).ok_or(TokenError::Unauthorized)?;
        let redirect_guard = origins_redirect_guard([normalized.as_ref()]);
        let http_client = build_registry_client_with_guard(config, Some(&redirect_guard))?;
        let retry_opts = config.retry_opts();

        let subcmd = self.params.first().map_or("list", String::as_str);
        match subcmd {
            "list" | "ls" => {
                self.run_list(&normalized, &http_client, &auth_header, retry_opts).await
            }
            "revoke" | "rm" | "delete" | "remove" => {
                self.run_revoke(&normalized, &http_client, &auth_header, retry_opts).await
            }
            "create" => self.run_create(&normalized, &http_client, &auth_header, retry_opts).await,
            other => Err(TokenError::UnknownCommand { command: other.to_string() }.into()),
        }
    }

    async fn run_list(
        &self,
        url: &str,
        client: &ThrottledClient,
        auth: &str,
        retry: RetryOpts,
    ) -> miette::Result<Option<String>> {
        let mut tokens = fetch_token_list(url, client, auth, retry).await?;
        generate_token_ids(&mut tokens);
        if self.json {
            let json_tokens = serde_json::to_string_pretty(&tokens).into_diagnostic()?;
            return Ok(Some(json_tokens));
        }
        if self.parseable {
            return Ok(Some(format_token_list_parseable(&tokens)));
        }
        Ok(Some(format_token_list(&tokens)))
    }

    async fn run_revoke(
        &self,
        url: &str,
        client: &ThrottledClient,
        auth: &str,
        retry: RetryOpts,
    ) -> miette::Result<Option<String>> {
        let ids = self.params.get(1..).unwrap_or_default();
        if ids.is_empty() {
            return Err(TokenError::KeyRequired.into());
        }
        let mut tokens = fetch_token_list(url, client, auth, retry).await?;
        generate_token_ids(&mut tokens);
        let keys_to_remove = ids
            .iter()
            .map(|id| resolve_key_to_remove(id, &tokens))
            .collect::<miette::Result<Vec<_>>>()?;
        for key in &keys_to_remove {
            delete_token(url, client, auth, self.otp.as_deref(), key, retry).await?;
        }
        if self.json {
            return Ok(Some(serde_json::to_string_pretty(&keys_to_remove).into_diagnostic()?));
        }
        if self.parseable {
            return Ok(Some(keys_to_remove.join("\t")));
        }
        let count = keys_to_remove.len();
        let plural = if count == 1 { "" } else { "s" };
        Ok(Some(format!("Removed {count} token{plural}")))
    }

    async fn run_create(
        &self,
        url: &str,
        client: &ThrottledClient,
        auth: &str,
        retry: RetryOpts,
    ) -> miette::Result<Option<String>> {
        let password = read_password().await?;
        let request = CreateTokenRequest {
            password: &password,
            readonly: self.read_only,
            cidr_whitelist: &self.cidr,
        };
        let created = create_token(url, client, auth, self.otp.as_deref(), &request, retry).await?;

        if self.json {
            return Ok(Some(serde_json::to_string_pretty(&created).into_diagnostic()?));
        }
        if self.parseable {
            return Ok(Some(format!("token\t{}\nreadonly\t{}", created.token, created.readonly)));
        }
        let mut text = format!("Created token {}", created.token);
        if let Some(cidrs) = &created.cidr_whitelist
            && !cidrs.is_empty()
        {
            let _ = write!(text, "\nwith IP whitelist: {}", cidrs.join(","));
        }
        Ok(Some(text))
    }
}
