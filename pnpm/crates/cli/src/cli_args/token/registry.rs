use crate::cli_args::{
    registry_client::{apply_auth_and_otp, join_registry_endpoint, join_registry_url},
    sanitize::{DEFAULT_ERROR_BODY_LIMIT, read_sanitized_error_body},
    token::{TOKEN_BODY_LIMIT, TokenError, redact_token_argument},
};
use miette::{Context, IntoDiagnostic};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use pnpm_network::{RetryOpts, ThrottledClient, read_limited_body, send_with_retry};
use serde::{Deserialize, Serialize};

/// Bounds the `urls.next` chain of `GET /-/npm/v1/tokens`, so a registry that
/// keeps returning a next page cannot hold the command forever.
const MAX_TOKEN_LIST_PAGES: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenItem {
    pub key: String,
    pub token: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub cidr_whitelist: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TokenListEnvelope {
    Paged {
        objects: Vec<TokenItem>,
        #[serde(default)]
        urls: TokenListUrls,
    },
    List(Vec<TokenItem>),
}

#[derive(Debug, Default, Deserialize)]
struct TokenListUrls {
    #[serde(default)]
    next: Option<String>,
}

#[derive(Serialize)]
pub struct CreateTokenRequest<'a> {
    pub password: &'a str,
    pub readonly: bool,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub cidr_whitelist: &'a [String],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTokenResponse {
    pub token: String,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub cidr_whitelist: Option<Vec<String>>,
    #[serde(default)]
    pub expires: Option<serde_json::Value>,
}

/// Fetch every page of the token list, following `urls.next` while it stays
/// on the registry's origin.
pub(super) async fn fetch_token_list(
    url: &str,
    http_client: &ThrottledClient,
    auth_header: &str,
    retry_opts: RetryOpts,
) -> miette::Result<Vec<TokenItem>> {
    let first_page = join_registry_url(url, "-/npm/v1/tokens")
        .into_diagnostic()
        .wrap_err("building the token list URL")?;
    let mut tokens = Vec::new();
    let mut next_page = Some(first_page);
    for _ in 0..MAX_TOKEN_LIST_PAGES {
        let Some(page_url) = next_page.take() else { return Ok(tokens) };
        let envelope =
            fetch_token_list_page(page_url.as_str(), http_client, auth_header, retry_opts).await?;
        match envelope {
            TokenListEnvelope::Paged { objects, urls } => {
                tokens.extend(objects);
                next_page =
                    urls.next.map(|next| resolve_next_page(&page_url, &next)).transpose()?;
            }
            TokenListEnvelope::List(list) => tokens.extend(list),
        }
    }
    if next_page.is_none() {
        return Ok(tokens);
    }
    Err(TokenError::RegistryOperationFailed {
        operation: "list tokens",
        reason: format!("the registry returned more than {MAX_TOKEN_LIST_PAGES} pages"),
    }
    .into())
}

fn resolve_next_page(current: &reqwest::Url, next: &str) -> miette::Result<reqwest::Url> {
    let next_url = current
        .join(next)
        .into_diagnostic()
        .wrap_err("parsing the next token page URL")?;
    if next_url.origin() != current.origin() {
        return Err(TokenError::RegistryOperationFailed {
            operation: "list tokens",
            reason: "the next page is on a different origin than the registry".to_string(),
        }
        .into());
    }
    Ok(next_url)
}

async fn fetch_token_list_page(
    endpoint: &str,
    http_client: &ThrottledClient,
    auth_header: &str,
    retry_opts: RetryOpts,
) -> miette::Result<TokenListEnvelope> {
    let (_client, response) = send_with_retry(http_client, endpoint, retry_opts, |cli| {
        apply_auth_and_otp(cli.get(endpoint), Some(auth_header), None)
    })
    .await
    .into_diagnostic()
    .wrap_err("requesting tokens")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(TokenError::Unauthorized.into());
    }
    if !response.status().is_success() {
        let (status, status_text, body) =
            read_sanitized_error_body(response, DEFAULT_ERROR_BODY_LIMIT).await;
        return Err(TokenError::RegistryWriteFailed {
            action: "list tokens".to_string(),
            status: status.as_u16(),
            status_text,
            body,
        }
        .into());
    }
    let body = read_limited_body(response, TOKEN_BODY_LIMIT).await
        .map_err(|err| TokenError::RegistryOperationFailed {
            operation: "reading token list",
            reason: err.to_string(),
        })?;
    serde_json::from_slice(&body.bytes).into_diagnostic().wrap_err("parsing tokens response")
}

pub(super) async fn delete_token(
    url: &str,
    http_client: &ThrottledClient,
    auth_header: &str,
    otp: Option<&str>,
    key: &str,
    retry_opts: RetryOpts,
) -> miette::Result<()> {
    let path = format!("-/npm/v1/tokens/token/{}", utf8_percent_encode(key, NON_ALPHANUMERIC));
    let endpoint = join_registry_endpoint(url, &path).unwrap_or_else(|_| format!("{url}{path}"));
    let (_client, response) = send_with_retry(http_client, &endpoint, retry_opts, |cli| {
        apply_auth_and_otp(cli.delete(&endpoint), Some(auth_header), otp)
    })
    .await
    .into_diagnostic()
    .wrap_err("revoking token")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(TokenError::Unauthorized.into());
    }
    if !response.status().is_success() {
        let (status, status_text, body) =
            read_sanitized_error_body(response, DEFAULT_ERROR_BODY_LIMIT).await;
        return Err(TokenError::RegistryWriteFailed {
            action: format!("revoke token {}", redact_token_argument(key)),
            status: status.as_u16(),
            status_text,
            body,
        }
        .into());
    }
    Ok(())
}

pub(super) async fn create_token(
    url: &str,
    http_client: &ThrottledClient,
    auth_header: &str,
    otp: Option<&str>,
    request: &CreateTokenRequest<'_>,
    retry_opts: RetryOpts,
) -> miette::Result<CreateTokenResponse> {
    let endpoint = join_registry_endpoint(url, "-/npm/v1/tokens")
        .unwrap_or_else(|_| format!("{url}-/npm/v1/tokens"));
    // Every POST that reaches the registry mints a token, so a retry after a
    // lost response would leave an extra live token nobody was shown.
    let single_attempt = RetryOpts { retries: 0, ..retry_opts };
    let (_client, response) = send_with_retry(http_client, &endpoint, single_attempt, |cli| {
        apply_auth_and_otp(cli.post(&endpoint).json(request), Some(auth_header), otp)
    })
    .await
    .into_diagnostic()
    .wrap_err("creating token")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(TokenError::Unauthorized.into());
    }
    if !response.status().is_success() {
        let (status, status_text, body) =
            read_sanitized_error_body(response, DEFAULT_ERROR_BODY_LIMIT).await;
        return Err(TokenError::RegistryWriteFailed {
            action: "create token".to_string(),
            status: status.as_u16(),
            status_text,
            body,
        }
        .into());
    }
    let body = read_limited_body(response, TOKEN_BODY_LIMIT).await
        .map_err(|err| TokenError::RegistryOperationFailed {
            operation: "reading create token response",
            reason: err.to_string(),
        })?;
    serde_json::from_slice(&body.bytes).into_diagnostic().wrap_err("parsing create token response")
}
