use clap::Args;
use derive_more::{Display, Error};
use miette::{Diagnostic, IntoDiagnostic, WrapErr};
use pacquet_config::Config;
use pacquet_network::{
    NetworkSettings, RetryOpts, ThrottledClient, encode_package_name, encode_uri_component,
    read_limited_body, redact_url_credentials, send_with_retry,
};
use pacquet_resolving_npm_resolver::pick_registry_for_package;
use reqwest::Response;
use serde::Deserialize;
use std::{collections::HashMap, time::Duration};

const OWNER_BODY_LIMIT: usize = 1024 * 1024;
const OWNER_ERROR_BODY_LIMIT: usize = 64 * 1024;

#[derive(Debug, Args)]
pub struct OwnerArgs {
    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,

    /// One-time password for registries that require two-factor authentication.
    #[clap(long)]
    pub otp: Option<String>,

    /// Subcommand and arguments.
    pub params: Vec<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum OwnerError {
    #[display("Package name is required")]
    #[diagnostic(code(ERR_PNPM_OWNER_LS_PACKAGE_REQUIRED))]
    LsPackageRequired,

    #[display("Package name and owner are required (e.g., pnpm owner add pkg username)")]
    #[diagnostic(code(ERR_PNPM_OWNER_ADD_ARGS_REQUIRED))]
    AddArgsRequired,

    #[display("Package name and owner are required (e.g., pnpm owner rm pkg username)")]
    #[diagnostic(code(ERR_PNPM_OWNER_RM_ARGS_REQUIRED))]
    RmArgsRequired,

    #[display(r#"Package "{package_name}" not found in registry"#)]
    #[diagnostic(code(ERR_PNPM_PACKAGE_NOT_FOUND))]
    PackageNotFound {
        #[error(not(source))]
        package_name: String,
    },

    #[display("Package not found in registry. {body}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_NOT_FOUND))]
    WritePackageNotFound {
        #[error(not(source))]
        body: String,
    },

    #[display("You must be logged in to {action} packages. {body}")]
    #[diagnostic(code(ERR_PNPM_UNAUTHORIZED))]
    Unauthorized {
        #[error(not(source))]
        action: String,
        #[error(not(source))]
        body: String,
    },

    #[display("You do not have permission to {action} this package. {body}")]
    #[diagnostic(code(ERR_PNPM_FORBIDDEN))]
    Forbidden {
        #[error(not(source))]
        action: String,
        #[error(not(source))]
        body: String,
    },

    #[display("Failed to {action} package: {status} {status_text}. {body}")]
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
        #[error(not(source))]
        operation: &'static str,
        #[error(not(source))]
        reason: String,
    },
}

struct OwnerContext<'a> {
    config: &'a Config,
    http_client: ThrottledClient,
    retry_opts: RetryOpts,
    registries: HashMap<String, String>,
    otp: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OwnerEntry {
    username: String,
    email: String,
}

impl OwnerArgs {
    pub async fn run(&self, config: &Config) -> miette::Result<Option<String>> {
        let subcommand = self.params.first().map(String::as_str);
        let context = self.context(config)?;

        match subcommand {
            Some("ls" | "list") => owner_ls(&context, &self.params[1..]).await.map(Some),
            Some("add") => owner_add(&context, &self.params[1..]).await.map(Some),
            Some("rm") => owner_rm(&context, &self.params[1..]).await.map(Some),
            _ => owner_ls(&context, &self.params).await.map(Some),
        }
    }

    fn context<'a>(&'a self, config: &'a Config) -> miette::Result<OwnerContext<'a>> {
        let mut registries: HashMap<String, String> =
            config.resolved_registries().into_iter().collect();
        if let Some(registry) = &self.registry {
            registries.insert("default".to_string(), normalize_registry_url(registry));
        }
        Ok(OwnerContext {
            config,
            http_client: build_http_client(config)?,
            retry_opts: RetryOpts {
                retries: config.fetch_retries,
                factor: config.fetch_retry_factor,
                min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
                max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
            },
            registries,
            otp: self.otp.clone(),
        })
    }
}

async fn owner_ls(context: &OwnerContext<'_>, params: &[String]) -> miette::Result<String> {
    let package_name = params.first().ok_or(OwnerError::LsPackageRequired)?;

    let registry_url = pick_registry_for_package(&context.registries, package_name, None);
    let auth_header =
        context.config.auth_headers.for_url_with_package(&registry_url, Some(package_name));

    let escaped = encode_package_name(package_name);
    let url = format!("{registry_url}-/package/{escaped}/owners");

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let mut builder = client.get(&url);
            if let Some(auth) = auth_header.as_deref() {
                builder = builder.header("authorization", auth);
            }
            builder
        })
        .await
        .map_err(|source| registry_operation_error("fetching owners", source))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(OwnerError::PackageNotFound { package_name: package_name.clone() }.into());
    }
    if !response.status().is_success() {
        return Err(write_error_from_response(response, "fetch owners of".to_string()).await);
    }

    let body = read_limited_body(response, OWNER_BODY_LIMIT)
        .await
        .map_err(|source| registry_operation_error("reading owners response", source))?;
    let owners: Vec<OwnerEntry> = serde_json::from_slice(&body.bytes)
        .into_diagnostic()
        .map_err(|source| registry_operation_error("parsing owners response", source))?;

    let lines: Vec<String> =
        owners.iter().map(|o| format!("{} <{}>", o.username, o.email)).collect();
    Ok(lines.join("\n"))
}

async fn owner_add(context: &OwnerContext<'_>, params: &[String]) -> miette::Result<String> {
    if params.len() < 2 {
        return Err(OwnerError::AddArgsRequired.into());
    }
    let package_name = &params[0];
    let owner = &params[1];

    let registry_url = pick_registry_for_package(&context.registries, package_name, None);
    let auth_header =
        context.config.auth_headers.for_url_with_package(&registry_url, Some(package_name));

    let escaped = encode_package_name(package_name);
    let url = format!("{registry_url}-/package/{escaped}/owners");
    let body = serde_json::json!({ "user": owner }).to_string();

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let mut builder =
                client.put(&url).header("content-type", "application/json").body(body.clone());
            if let Some(auth) = auth_header.as_deref() {
                builder = builder.header("authorization", auth);
            }
            if let Some(otp) = &context.otp {
                builder = builder.header("npm-otp", otp.as_str());
            }
            builder
        })
        .await
        .map_err(|source| registry_operation_error("adding owner", source))?;

    if response.status().is_success() {
        return Ok(format!("+{owner}: {package_name}"));
    }
    Err(write_error_from_response(response, format!(r#"add owner "{owner}" to"#)).await)
}

async fn owner_rm(context: &OwnerContext<'_>, params: &[String]) -> miette::Result<String> {
    if params.len() < 2 {
        return Err(OwnerError::RmArgsRequired.into());
    }
    let package_name = &params[0];
    let owner = &params[1];

    let registry_url = pick_registry_for_package(&context.registries, package_name, None);
    let auth_header =
        context.config.auth_headers.for_url_with_package(&registry_url, Some(package_name));

    let escaped = encode_package_name(package_name);
    let encoded_owner = encode_uri_component(owner);
    let url = format!("{registry_url}-/package/{escaped}/owners/{encoded_owner}");

    let (_guard, response) =
        send_with_retry(&context.http_client, &url, context.retry_opts, |client| {
            let mut builder = client.delete(&url);
            if let Some(auth) = auth_header.as_deref() {
                builder = builder.header("authorization", auth);
            }
            if let Some(otp) = &context.otp {
                builder = builder.header("npm-otp", otp.as_str());
            }
            builder
        })
        .await
        .map_err(|source| registry_operation_error("removing owner", source))?;

    if response.status().is_success() {
        return Ok(format!("-{owner}: {package_name}"));
    }
    Err(write_error_from_response(response, format!(r#"remove owner "{owner}" from"#)).await)
}

fn build_http_client(config: &Config) -> miette::Result<ThrottledClient> {
    ThrottledClient::for_installs(
        &config.proxy,
        &config.tls,
        &config.tls_by_uri,
        &NetworkSettings {
            network_concurrency: config.network_concurrency,
            fetch_timeout: Duration::from_millis(config.fetch_timeout),
            user_agent: config.user_agent.clone(),
        },
    )
    .into_diagnostic()
    .wrap_err("create the network client for owner command")
}

fn registry_operation_error<ErrorType>(operation: &'static str, error: ErrorType) -> miette::Report
where
    ErrorType: std::fmt::Display,
{
    OwnerError::RegistryOperationFailed {
        operation,
        reason: redact_url_credentials(&error.to_string()),
    }
    .into()
}

async fn write_error_from_response(response: Response, action: String) -> miette::Report {
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_string();
    let body = match read_limited_body(response, OWNER_ERROR_BODY_LIMIT).await {
        Ok(body) => super::sanitize::body_display_string(&body),
        Err(_) => String::new(),
    };

    match status {
        reqwest::StatusCode::UNAUTHORIZED => OwnerError::Unauthorized { action, body }.into(),
        reqwest::StatusCode::FORBIDDEN => OwnerError::Forbidden { action, body }.into(),
        reqwest::StatusCode::NOT_FOUND => OwnerError::WritePackageNotFound { body }.into(),
        _ => OwnerError::RegistryWriteFailed { action, status: status.as_u16(), status_text, body }
            .into(),
    }
}

fn normalize_registry_url(registry_url: &str) -> String {
    if registry_url.ends_with('/') { registry_url.to_string() } else { format!("{registry_url}/") }
}

#[cfg(test)]
mod tests;
