use crate::cli_args::{registry_client::build_registry_client, whoami::fetch_whoami};
use clap::Parser;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_network::{RetryOpts, ThrottledClient, encode_package_name, send_with_retry};
use serde_json::{Map, Value, json};
use std::time::Duration;

#[derive(Debug, Parser)]
pub struct StarArgs {
    pub package_name: String,
}

/// Errors shared by `pacquet star` and `pacquet unstar`. `action` is
/// `"star"` or `"unstar"` so a single set of variants renders the right verb
/// for either command, matching the shared TypeScript `performStarAction`.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum StarError {
    #[display("You must be logged in to {action} packages")]
    #[diagnostic(code(ERR_PNPM_STAR_UNAUTHORIZED))]
    Unauthorized { action: &'static str },

    #[display("Failed to {action} package: {status} {status_text}. {body}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    Failed { action: &'static str, status: u16, status_text: String, body: String },

    #[display("Package \"{package}\" not found in registry")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_NOT_FOUND))]
    PackageNotFound { package: String },

    #[display("Failed to fetch package info: {status} {status_text}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    FetchPackageInfo { status: u16, status_text: String },

    #[display("Failed to {action} package (legacy): {status} {status_text}. {body}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    LegacyFailed { action: &'static str, status: u16, status_text: String, body: String },
}

impl StarArgs {
    pub async fn run(&self, config: &Config) -> miette::Result<()> {
        star_action(config, &self.package_name, true).await
    }
}

/// Resolve credentials and drive [`fetch_star`] for either command. Fails with
/// `ERR_PNPM_STAR_UNAUTHORIZED` before any request when no credentials are
/// configured for the registry.
pub(crate) async fn star_action(
    config: &Config,
    package_name: &str,
    is_star: bool,
) -> miette::Result<()> {
    let action = action_word(is_star);
    let auth_header =
        config.auth_headers.for_url(&config.registry).ok_or(StarError::Unauthorized { action })?;
    let http_client = build_registry_client(config)?;
    let retry_opts = RetryOpts {
        retries: config.fetch_retries,
        factor: config.fetch_retry_factor,
        min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
        max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
    };
    fetch_star(&config.registry, &http_client, &auth_header, retry_opts, package_name, is_star)
        .await
}

pub(crate) async fn fetch_star(
    registry_url: &str,
    http_client: &ThrottledClient,
    auth_header: &str,
    retry_opts: RetryOpts,
    package_name: &str,
    is_star: bool,
) -> miette::Result<()> {
    let action = action_word(is_star);
    let method = if is_star { reqwest::Method::PUT } else { reqwest::Method::DELETE };
    let star_url = format!("{registry_url}-/user/v1/star");
    let body = json!({ "name": package_name, "package": package_name }).to_string();

    let (client, response) = send_with_retry(http_client, &star_url, retry_opts, |client| {
        client
            .request(method.clone(), &star_url)
            .header("authorization", auth_header)
            .header("content-type", "application/json")
            .body(body.clone())
    })
    .await
    .into_diagnostic()
    .wrap_err("requesting the registry star endpoint")?;

    if response.status().is_success() {
        drop(client);
        return Ok(());
    }
    drop(client);

    let escaped_name = encode_package_name(package_name);
    let alt_star_url = format!("{registry_url}-/user/package/{escaped_name}/star");
    let (client2, response2) = send_with_retry(http_client, &alt_star_url, retry_opts, |client| {
        client
            .request(method.clone(), &alt_star_url)
            .header("authorization", auth_header)
            .header("content-type", "application/json")
    })
    .await
    .into_diagnostic()
    .wrap_err("requesting the alt registry star endpoint")?;

    if response2.status().is_success() {
        drop(client2);
        return Ok(());
    }

    let status = response2.status();
    // Registries that don't implement the star endpoints answer with one of
    // these statuses; fall back to editing the packument's `users` map, as the
    // TypeScript CLI does.
    if matches!(status.as_u16(), 400 | 404 | 405 | 500) {
        drop(client2);
        return perform_legacy_star_action(
            registry_url,
            http_client,
            auth_header,
            retry_opts,
            package_name,
            &escaped_name,
            is_star,
        )
        .await;
    }
    let body = response2.text().await.unwrap_or_default();
    Err(StarError::Failed {
        action,
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or_default().to_string(),
        body,
    }
    .into())
}

/// Star/unstar a package on a registry without the star endpoints by fetching
/// the full packument, editing its `users` map, and writing it back — the
/// legacy path the TypeScript CLI keeps for old registries.
async fn perform_legacy_star_action(
    registry_url: &str,
    http_client: &ThrottledClient,
    auth_header: &str,
    retry_opts: RetryOpts,
    package_name: &str,
    escaped_name: &str,
    is_star: bool,
) -> miette::Result<()> {
    let action = action_word(is_star);
    let username = fetch_whoami(registry_url, http_client, auth_header, retry_opts).await?;
    let pkg_url = format!("{registry_url}{escaped_name}");

    let (client, response) = send_with_retry(http_client, &pkg_url, retry_opts, |client| {
        client
            .get(&pkg_url)
            .header("authorization", auth_header)
            .header("accept", "application/json")
    })
    .await
    .into_diagnostic()
    .wrap_err("requesting the package metadata")?;

    if !response.status().is_success() {
        let status = response.status();
        drop(client);
        if status.as_u16() == 404 {
            return Err(StarError::PackageNotFound { package: package_name.to_string() }.into());
        }
        return Err(StarError::FetchPackageInfo {
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or_default().to_string(),
        }
        .into());
    }
    let mut pkg_data: Value =
        response.json().await.into_diagnostic().wrap_err("parsing the package metadata")?;
    drop(client);

    apply_star_to_users(&mut pkg_data, &username, is_star);

    let rev = pkg_data.get("_rev").and_then(Value::as_str);
    let update_url = match rev {
        Some(rev) => format!("{pkg_url}/-rev/{rev}"),
        None => pkg_url.clone(),
    };
    let update_body = pkg_data.to_string();

    let (client2, update_response) =
        send_with_retry(http_client, &update_url, retry_opts, |client| {
            client
                .put(&update_url)
                .header("authorization", auth_header)
                .header("content-type", "application/json")
                .body(update_body.clone())
        })
        .await
        .into_diagnostic()
        .wrap_err("updating the package metadata")?;

    if !update_response.status().is_success() {
        let status = update_response.status();
        let body = update_response.text().await.unwrap_or_default();
        return Err(StarError::LegacyFailed {
            action,
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or_default().to_string(),
            body,
        }
        .into());
    }
    drop(client2);
    Ok(())
}

/// Set or clear `pkg_data.users[username]`, creating the `users` map when it is
/// missing or not an object, mirroring `pkgData.users = pkgData.users || {}`.
fn apply_star_to_users(pkg_data: &mut Value, username: &str, is_star: bool) {
    let Some(obj) = pkg_data.as_object_mut() else { return };
    let users = obj.entry("users").or_insert_with(|| Value::Object(Map::new()));
    if !users.is_object() {
        *users = Value::Object(Map::new());
    }
    let users = users.as_object_mut().expect("users was just ensured to be an object");
    if is_star {
        users.insert(username.to_string(), Value::Bool(true));
    } else {
        users.remove(username);
    }
}

fn action_word(is_star: bool) -> &'static str {
    if is_star { "star" } else { "unstar" }
}
